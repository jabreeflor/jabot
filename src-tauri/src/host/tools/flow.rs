//! One in-flight connect: discovery, consent in the browser, token exchange.
//!
//! It runs on its own thread because none of it is fast and the host answers
//! JSON-RPC on one. Discovery is two or three round trips; the consent step is
//! however long the human takes. A `tools/connect` that blocked until a user
//! finished signing in would freeze every other thread in the app.
//!
//! So the flow publishes its progress into a shared slot and the host drains
//! it — the same shape as the ACP adapter's wake, and for the same reason. The
//! commit into SQLite and the vault happens back on the host thread, where the
//! store lives.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::catalog::Provider;
use super::clients;
use super::http::{CurlClient, HttpClient};
use super::loopback::Loopback;
use super::oauth::{self, GrantContext, OAuthClient, Pkce, TokenBundle};

/// How long a consent window stays open. Long enough to find a password, short
/// enough that a forgotten tab does not hold a socket for the session.
const CONSENT_DEADLINE: Duration = Duration::from_secs(300);

/// What a connect flow is doing right now.
#[derive(Debug)]
pub enum FlowState {
    /// Asking the MCP server who authorises it.
    Discovering,
    /// Waiting for the human. The URL is the one the UI opens.
    AwaitingUser {
        authorize_url: String,
    },
    /// The browser came back; trading the code for tokens.
    Exchanging,
    Done(Box<TokenBundle>),
    Failed(String),
}

impl FlowState {
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Done(_) | Self::Failed(_))
    }
}

/// What the flow needs to know, resolved from the catalog by the caller so
/// this module never reaches back into it.
#[derive(Debug, Clone)]
pub struct ConnectRequest {
    pub tool_id: String,
    pub provider: Provider,
    /// The MCP server this grant is for — also the RFC 8707 `resource`.
    pub server_url: String,
    pub scopes: Vec<String>,
    /// Where `oauth_clients.json` lives, for providers without dynamic
    /// registration. `None` on an ephemeral host: nowhere for one to be.
    pub config_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub struct ConnectFlow {
    pub tool_id: String,
    pub redirect_uri: String,
    pub started_at: Instant,
    state: Arc<Mutex<FlowState>>,
    cancel: Arc<AtomicBool>,
}

impl ConnectFlow {
    /// Start the flow. The loopback socket is bound here, on the host thread,
    /// so a port that cannot be had is an error on the call rather than a
    /// failure that shows up seconds later on a chip.
    pub fn start(request: ConnectRequest) -> Result<Self, String> {
        Self::start_with(request, Arc::new(CurlClient))
    }

    pub fn start_with(request: ConnectRequest, http: Arc<dyn HttpClient>) -> Result<Self, String> {
        let loopback = Loopback::bind().map_err(|e| e.to_string())?;
        let redirect_uri = loopback.redirect_uri().to_string();
        let state = Arc::new(Mutex::new(FlowState::Discovering));
        let cancel = Arc::new(AtomicBool::new(false));

        let thread_state = Arc::clone(&state);
        let thread_cancel = Arc::clone(&cancel);
        let thread_request = request.clone();
        let thread_redirect = redirect_uri.clone();
        std::thread::Builder::new()
            .name(format!("oauth-{}", request.provider.id))
            .spawn(move || {
                let outcome = run(
                    http.as_ref(),
                    &thread_request,
                    &loopback,
                    &thread_redirect,
                    &thread_state,
                    &thread_cancel,
                );
                let mut slot = thread_state.lock().expect("flow state");
                *slot = match outcome {
                    Ok(bundle) => FlowState::Done(Box::new(bundle)),
                    Err(message) => FlowState::Failed(message),
                };
            })
            .map_err(|e| format!("could not start the sign-in flow: {e}"))?;

        Ok(Self {
            tool_id: request.tool_id,
            redirect_uri,
            started_at: Instant::now(),
            state,
            cancel,
        })
    }

    /// The authorize URL once discovery has produced one.
    pub fn authorize_url(&self) -> Option<String> {
        match &*self.state.lock().expect("flow state") {
            FlowState::AwaitingUser { authorize_url } => Some(authorize_url.clone()),
            _ => None,
        }
    }

    /// Take the outcome if the flow has finished; leave it running otherwise.
    pub fn take_outcome(&self) -> Option<Result<TokenBundle, String>> {
        let mut slot = self.state.lock().expect("flow state");
        if !slot.is_finished() {
            return None;
        }
        match std::mem::replace(&mut *slot, FlowState::Discovering) {
            FlowState::Done(bundle) => Some(Ok(*bundle)),
            FlowState::Failed(message) => Some(Err(message)),
            _ => None,
        }
    }

    /// Ask the flow to stop. The listener notices within a poll interval.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// A flow whose thread died without publishing anything would otherwise
    /// leave a chip saying "connecting" forever.
    pub fn is_abandoned(&self) -> bool {
        self.started_at.elapsed() > CONSENT_DEADLINE + Duration::from_secs(30)
    }
}

fn run(
    http: &dyn HttpClient,
    request: &ConnectRequest,
    loopback: &Loopback,
    redirect_uri: &str,
    state: &Arc<Mutex<FlowState>>,
    cancel: &Arc<AtomicBool>,
) -> Result<TokenBundle, String> {
    let server = oauth::discover(http, &request.server_url).map_err(|e| e.to_string())?;
    let client = resolve_client(http, request, &server, redirect_uri)?;

    let pkce = Pkce::generate();
    let csrf_state = super::crypto::random_token();
    let extra: Vec<(&str, &str)> = request.provider.authorize_params.to_vec();
    let ctx = GrantContext {
        client: &client,
        redirect_uri,
        scopes: &request.scopes,
        resource: &request.server_url,
    };
    let authorize_url = oauth::authorize_url(&server, &ctx, &csrf_state, &pkce, &extra);
    {
        let mut slot = state.lock().expect("flow state");
        *slot = FlowState::AwaitingUser {
            authorize_url: authorize_url.clone(),
        };
    }

    let code = loopback
        .wait(&csrf_state, Instant::now() + CONSENT_DEADLINE, cancel)
        .map_err(|e| e.to_string())?;
    {
        let mut slot = state.lock().expect("flow state");
        *slot = FlowState::Exchanging;
    }

    oauth::exchange_code(http, &server, &ctx, &code, &pkce).map_err(|e| e.to_string())
}

/// The user's own registration first, then dynamic registration.
///
/// That order is deliberate: someone who registered a client with tighter
/// scopes than the provider's default should get theirs, not one we minted
/// behind their back.
fn resolve_client(
    http: &dyn HttpClient,
    request: &ConnectRequest,
    server: &oauth::AuthServer,
    redirect_uri: &str,
) -> Result<OAuthClient, String> {
    if let Some(client) = clients::registered(request.config_dir.as_deref(), request.provider.id)? {
        return Ok(client);
    }
    if server.registration_endpoint.is_some() {
        return oauth::register_client(http, server, redirect_uri, &request.scopes)
            .map_err(|e| e.to_string());
    }
    Err(clients::missing_client_hint(
        request.provider.label,
        request.config_dir.as_deref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::catalog::GOOGLE;
    use super::*;
    use crate::host::tools::testing::LocalAuthServer;

    fn request(server_url: &str, config_dir: Option<PathBuf>) -> ConnectRequest {
        ConnectRequest {
            tool_id: "gmail".into(),
            provider: GOOGLE,
            server_url: server_url.into(),
            scopes: vec!["gmail.compose".into()],
            config_dir,
        }
    }

    fn await_authorize_url(flow: &ConnectFlow) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(url) = flow.authorize_url() {
                return url;
            }
            if let Some(outcome) = flow.take_outcome() {
                panic!("flow finished before consent: {outcome:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("flow never produced an authorize URL");
    }

    fn await_outcome(flow: &ConnectFlow) -> Result<TokenBundle, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(outcome) = flow.take_outcome() {
                return outcome;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("flow never finished");
    }

    /// The whole grant, end to end, against a real authorization server on
    /// loopback: discovery, dynamic registration, the browser redirect, and
    /// the code exchange — over real sockets, with real HTTP, and with the
    /// PKCE verifier checked by the server rather than by us.
    #[test]
    fn a_full_grant_is_discovered_registered_consented_and_exchanged() {
        let auth = LocalAuthServer::start();
        let flow = ConnectFlow::start(request(&auth.mcp_url(), None)).expect("flow started");

        let authorize_url = await_authorize_url(&flow);
        assert!(authorize_url.starts_with(&auth.authorize_endpoint()));
        assert!(authorize_url.contains("code_challenge_method=S256"));
        // Google will not issue a refresh token without this, and the flow
        // takes it from the provider's card rather than inventing it.
        assert!(authorize_url.contains("access_type=offline"));

        // Stand in for the browser: follow the authorize URL, which redirects
        // to the loopback listener with a code.
        auth.consent(&authorize_url);

        let bundle = await_outcome(&flow).expect("a grant");
        assert_eq!(bundle.access_token, auth.issued_access_token());
        assert_eq!(bundle.refresh_token.as_deref(), Some("refresh-1"));
        assert_eq!(bundle.resource, auth.mcp_url());
        assert_eq!(bundle.account.as_deref(), Some("you@example.com"));
        // Dynamic registration, because this server offers it.
        assert_eq!(bundle.client_id, "dcr-client-1");
        assert!(bundle.expires_at.is_some());
    }

    /// PKCE is only worth having if the server actually checks the verifier —
    /// and if a wrong one comes back as the provider's refusal, not a panic.
    #[test]
    fn a_bad_verifier_is_refused_by_the_server() {
        let auth = LocalAuthServer::start();
        auth.corrupt_next_verifier();
        let flow = ConnectFlow::start(request(&auth.mcp_url(), None)).expect("flow started");
        let authorize_url = await_authorize_url(&flow);
        auth.consent(&authorize_url);

        let err = await_outcome(&flow).unwrap_err();
        assert!(
            err.contains("invalid_grant") || err.contains("verifier"),
            "{err}"
        );
    }

    /// No dynamic registration and no user registration is the one case we
    /// cannot solve for the user — so it has to say exactly what they must do.
    #[test]
    fn without_registration_the_error_names_the_file_to_write() {
        let auth = LocalAuthServer::start();
        auth.disable_dynamic_registration();
        let dir = tempfile::tempdir().unwrap();
        let flow = ConnectFlow::start(request(&auth.mcp_url(), Some(dir.path().to_path_buf())))
            .expect("flow started");

        let err = await_outcome(&flow).unwrap_err();
        assert!(err.contains("oauth_clients.json"), "{err}");
        assert!(err.contains("Google"), "{err}");
    }

    /// A user's own registration wins over minting one.
    #[test]
    fn a_registered_client_is_used_instead_of_dynamic_registration() {
        let auth = LocalAuthServer::start();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(clients::CLIENTS_FILE),
            r#"{"google":{"clientId":"my-own-client"}}"#,
        )
        .unwrap();

        let flow = ConnectFlow::start(request(&auth.mcp_url(), Some(dir.path().to_path_buf())))
            .expect("flow started");
        let authorize_url = await_authorize_url(&flow);
        assert!(
            authorize_url.contains("client_id=my-own-client"),
            "{authorize_url}"
        );
        auth.consent(&authorize_url);
        assert_eq!(
            await_outcome(&flow).expect("a grant").client_id,
            "my-own-client"
        );
    }

    #[test]
    fn cancelling_ends_the_flow_without_a_grant() {
        let auth = LocalAuthServer::start();
        let flow = ConnectFlow::start(request(&auth.mcp_url(), None)).expect("flow started");
        await_authorize_url(&flow);
        flow.cancel();
        let err = await_outcome(&flow).unwrap_err();
        assert!(err.contains("cancelled"), "{err}");
    }
}
