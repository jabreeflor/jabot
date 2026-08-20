//! OAuth 2.1 authorization code + PKCE for remote MCP servers.
//!
//! JaBot is a **public** desktop client: no client secret, PKCE mandatory,
//! loopback redirect (RFC 8252). Every endpoint is *discovered* from the MCP
//! server rather than compiled in — protected-resource metadata (RFC 9728)
//! names the authorization server, authorization-server metadata (RFC 8414)
//! names its endpoints, and dynamic client registration (RFC 7591) mints the
//! client id where the provider offers it. That is what the MCP authorization
//! spec asks of a client, and it is also why this file contains no client ids:
//! there are none to hardcode, and a fabricated one is a login that fails at
//! the provider with a message the user cannot act on.
//!
//! Where a provider does not offer dynamic registration, the client id comes
//! from the user's own registration in `oauth_clients.json` — see
//! [`super::clients`]. Never from us.
//!
//! Tokens are returned to the caller and go straight into the vault. Nothing
//! here writes a token to disk, to a log, or into a `Debug` line.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::crypto::{base64url, random_token, sha256};
use super::http::{percent_encode, HttpClient, HttpError};

/// Refresh this long before the access token actually expires, so a prompt
/// does not race the clock between minting a header and the server reading it.
pub const EXPIRY_SKEW_SECS: i64 = 60;

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error("{0}")]
    Discovery(String),
    /// The provider answered, and said no. Its own words, because ours would
    /// be a guess: `invalid_grant` and `access_denied` need different fixes.
    /// The status rides along because it is the only part of the answer that
    /// says whether the grant is dead or the provider is merely having a bad
    /// minute — see [`OAuthError::is_grant_refusal`].
    #[error("{provider} refused the request: {detail}")]
    Provider {
        provider: String,
        status: u16,
        detail: String,
    },
    #[error("{0}")]
    Protocol(String),
}

impl OAuthError {
    /// True only when the provider itself rejected the grant.
    ///
    /// This is the one question that may cost a user their refresh token, so
    /// it answers narrowly. RFC 6749 §5.2 gives 400 for `invalid_grant` and
    /// 400/401 for `invalid_client` — a grant that will never work again.
    /// Everything else is a failure to get an answer, not an answer: a 429, a
    /// 503, a token endpoint behind a captive portal, curl not on PATH. Those
    /// grants are still good and must survive to the next attempt.
    pub fn is_grant_refusal(&self) -> bool {
        matches!(self, Self::Provider { status, .. } if matches!(status, 400 | 401))
    }
}

/// What RFC 8414 metadata tells us about an authorization server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthServer {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    /// Present when the provider supports dynamic client registration.
    pub registration_endpoint: Option<String>,
    pub code_challenge_methods: Vec<String>,
}

impl AuthServer {
    /// S256 is required by OAuth 2.1 and by the MCP spec. A server that lists
    /// only `plain` is refused rather than downgraded to it.
    pub fn supports_s256(&self) -> bool {
        self.code_challenge_methods.is_empty()
            || self.code_challenge_methods.iter().any(|m| m == "S256")
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(raw).ok()?;
        Some(Self {
            issuer: string(&value, "issuer").unwrap_or_default(),
            authorization_endpoint: string(&value, "authorization_endpoint")?,
            token_endpoint: string(&value, "token_endpoint")?,
            registration_endpoint: string(&value, "registration_endpoint"),
            code_challenge_methods: string_array(&value, "code_challenge_methods_supported"),
        })
    }
}

/// A registered (or dynamically registered) OAuth client.
///
/// `secret` is `None` for a public client, which is what JaBot registers as.
/// If a provider hands one out anyway it is a credential, and it goes to the
/// vault with the tokens rather than into `tool_connections`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthClient {
    pub client_id: String,
    pub secret: Option<String>,
}

/// A PKCE verifier and its S256 challenge.
#[derive(Clone, PartialEq, Eq)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Self {
        let verifier = random_token();
        let challenge = base64url(&sha256(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

/// The verifier is a one-time secret: printing it beside the challenge would
/// undo the point of PKCE in any log that captures both.
impl std::fmt::Debug for Pkce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pkce")
            .field("verifier", &"<redacted>")
            .field("challenge", &self.challenge)
            .finish()
    }
}

/// Everything needed to mint a bearer for one provider, later.
///
/// Stored as a single vault item per provider. `token_endpoint` and
/// `client_id` ride along so a refresh needs no rediscovery — and so a refresh
/// can never be pointed at an endpoint the grant was not issued by.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenBundle {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    pub token_type: String,
    /// RFC 3339. `None` when the provider issues tokens that do not expire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub token_endpoint: String,
    /// Every MCP server this grant is audience-bound to (RFC 8707). A list
    /// rather than one URL because one provider grant serves every chip that
    /// shares it, and a token bound to Gmail's URL alone is one the Calendar
    /// server is entitled to refuse.
    pub resources: Vec<String>,
    /// Which account the human authorised, for the chip to show. Display only
    /// — nothing is authorised on the strength of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

impl std::fmt::Debug for TokenBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenBundle")
            .field("accessToken", &"<redacted>")
            .field(
                "refreshToken",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expiresAt", &self.expires_at)
            .field("scopes", &self.scopes)
            .field("clientId", &self.client_id)
            .finish_non_exhaustive()
    }
}

impl TokenBundle {
    pub fn authorization_header(&self) -> String {
        let scheme = if self.token_type.is_empty() {
            "Bearer"
        } else {
            &self.token_type
        };
        format!("{scheme} {}", self.access_token)
    }

    /// True when the access token is gone or about to be.
    pub fn needs_refresh(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        let Some(expires_at) = &self.expires_at else {
            return false;
        };
        match chrono::DateTime::parse_from_rfc3339(expires_at) {
            Ok(at) => at.timestamp() - now.timestamp() <= EXPIRY_SKEW_SECS,
            // An expiry we cannot read is treated as expired: refreshing a
            // live token costs a round trip, using a dead one fails the turn.
            Err(_) => true,
        }
    }
}

/// Find the authorization server behind an MCP endpoint.
///
/// The unauthenticated probe is the documented path — a remote MCP server
/// answers 401 with `WWW-Authenticate: Bearer resource_metadata="…"` — but the
/// well-known location is derived as a fallback, because a server that answers
/// 404 to an unauthenticated GET is still discoverable.
pub fn discover(http: &dyn HttpClient, server_url: &str) -> Result<AuthServer, OAuthError> {
    let prm_url = http
        .get(server_url, &[])
        .ok()
        .and_then(|response| resource_metadata_url(response.header("WWW-Authenticate")?))
        .unwrap_or_else(|| well_known(server_url, "oauth-protected-resource"));

    let issuer = match http.get(&prm_url, &[]) {
        Ok(response) if response.is_success() => serde_json::from_str::<Value>(&response.body)
            .ok()
            .and_then(|value| {
                string_array(&value, "authorization_servers")
                    .into_iter()
                    .next()
            }),
        _ => None,
    }
    // A server that publishes no resource metadata may still be its own
    // authorization server; that is the shape most single-tenant MCP servers
    // ship with, and it costs one more GET to find out.
    .unwrap_or_else(|| origin_of(server_url));

    for candidate in [
        well_known(&issuer, "oauth-authorization-server"),
        format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        ),
    ] {
        if let Ok(response) = http.get(&candidate, &[]) {
            if response.is_success() {
                if let Some(server) = AuthServer::parse(&response.body) {
                    if !server.supports_s256() {
                        return Err(OAuthError::Discovery(format!(
                            "{issuer} does not support PKCE S256, which JaBot requires"
                        )));
                    }
                    return Ok(server);
                }
            }
        }
    }
    Err(OAuthError::Discovery(format!(
        "could not read OAuth metadata for {issuer}"
    )))
}

/// RFC 7591 dynamic client registration, as a public native client.
pub fn register_client(
    http: &dyn HttpClient,
    server: &AuthServer,
    redirect_uri: &str,
    scopes: &[String],
) -> Result<OAuthClient, OAuthError> {
    let endpoint = server
        .registration_endpoint
        .as_deref()
        .ok_or_else(|| OAuthError::Discovery("no registration endpoint".into()))?;
    let body = serde_json::json!({
        "client_name": "JaBot",
        "redirect_uris": [redirect_uri],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "application_type": "native",
        // Public client: there is no secret to keep on a user's Mac.
        "token_endpoint_auth_method": "none",
        "scope": scopes.join(" "),
    });
    let response = http.post_json(endpoint, &body.to_string())?;
    if !response.is_success() {
        return Err(OAuthError::Provider {
            provider: server.issuer.clone(),
            status: response.status,
            detail: error_detail(&response.body, response.status),
        });
    }
    let value: Value = serde_json::from_str(&response.body)
        .map_err(|e| OAuthError::Protocol(format!("registration response: {e}")))?;
    Ok(OAuthClient {
        client_id: string(&value, "client_id")
            .ok_or_else(|| OAuthError::Protocol("registration returned no client_id".into()))?,
        secret: string(&value, "client_secret"),
    })
}

/// The four things both halves of the grant need to agree on. Passed as one
/// value so an authorize request and its token exchange cannot disagree about
/// the redirect URI or the resource — a mismatch either provider would reject
/// with a message about neither.
#[derive(Debug, Clone)]
pub struct GrantContext<'a> {
    pub client: &'a OAuthClient,
    pub redirect_uri: &'a str,
    pub scopes: &'a [String],
    /// Every MCP server this grant is for (RFC 8707 `resource`, which §2
    /// allows to repeat). All of them, not just the one whose chip was
    /// clicked: the grant is per provider, so the audience must be too.
    pub resources: &'a [String],
}

/// The URL to open in the user's browser.
pub fn authorize_url(
    server: &AuthServer,
    ctx: &GrantContext<'_>,
    state: &str,
    pkce: &Pkce,
    extra: &[(&str, &str)],
) -> String {
    let mut params: Vec<(&str, String)> = vec![
        ("response_type", "code".into()),
        ("client_id", ctx.client.client_id.clone()),
        ("redirect_uri", ctx.redirect_uri.to_string()),
        ("state", state.to_string()),
        ("code_challenge", pkce.challenge.clone()),
        ("code_challenge_method", "S256".into()),
    ];
    // RFC 8707: name the MCP servers this grant is for, so a token minted for
    // one provider's servers cannot be replayed against another's.
    for resource in ctx.resources {
        params.push(("resource", resource.clone()));
    }
    if !ctx.scopes.is_empty() {
        params.push(("scope", ctx.scopes.join(" ")));
    }
    for (key, value) in extra {
        params.push((key, (*value).to_string()));
    }
    let query = params
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    let separator = if server.authorization_endpoint.contains('?') {
        '&'
    } else {
        '?'
    };
    format!("{}{separator}{query}", server.authorization_endpoint)
}

/// Trade the authorization code for tokens.
pub fn exchange_code(
    http: &dyn HttpClient,
    server: &AuthServer,
    ctx: &GrantContext<'_>,
    code: &str,
    pkce: &Pkce,
) -> Result<TokenBundle, OAuthError> {
    let mut form: Vec<(&str, String)> = vec![
        ("grant_type", "authorization_code".into()),
        ("code", code.to_string()),
        ("redirect_uri", ctx.redirect_uri.to_string()),
        ("client_id", ctx.client.client_id.clone()),
        ("code_verifier", pkce.verifier.clone()),
    ];
    for resource in ctx.resources {
        form.push(("resource", resource.clone()));
    }
    if let Some(secret) = &ctx.client.secret {
        form.push(("client_secret", secret.clone()));
    }
    let response = http.post_form(&server.token_endpoint, &form)?;
    if !response.is_success() {
        return Err(OAuthError::Provider {
            provider: server.issuer.clone(),
            status: response.status,
            detail: error_detail(&response.body, response.status),
        });
    }
    parse_token_response(
        &response.body,
        ctx.client,
        &server.token_endpoint,
        ctx.resources,
        ctx.scopes,
        None,
    )
}

/// Exchange a refresh token for a fresh access token.
pub fn refresh(http: &dyn HttpClient, bundle: &TokenBundle) -> Result<TokenBundle, OAuthError> {
    let refresh_token = bundle
        .refresh_token
        .as_deref()
        .ok_or_else(|| OAuthError::Protocol("grant has no refresh token".into()))?;
    let client = OAuthClient {
        client_id: bundle.client_id.clone(),
        secret: bundle.client_secret.clone(),
    };
    let mut form: Vec<(&str, String)> = vec![
        ("grant_type", "refresh_token".into()),
        ("refresh_token", refresh_token.to_string()),
        ("client_id", client.client_id.clone()),
    ];
    for resource in &bundle.resources {
        form.push(("resource", resource.clone()));
    }
    if let Some(secret) = &client.secret {
        form.push(("client_secret", secret.clone()));
    }
    let response = http.post_form(&bundle.token_endpoint, &form)?;
    if !response.is_success() {
        return Err(OAuthError::Provider {
            provider: bundle.client_id.clone(),
            status: response.status,
            detail: error_detail(&response.body, response.status),
        });
    }
    parse_token_response(
        &response.body,
        &client,
        &bundle.token_endpoint,
        &bundle.resources,
        &bundle.scopes,
        bundle.refresh_token.as_deref(),
    )
}

/// RFC 6749 §5.1, plus the one rule that bites everyone: a refresh response
/// that omits `refresh_token` means *keep the one you have*, not "the grant no
/// longer has one".
fn parse_token_response(
    raw: &str,
    client: &OAuthClient,
    token_endpoint: &str,
    resources: &[String],
    requested_scopes: &[String],
    previous_refresh: Option<&str>,
) -> Result<TokenBundle, OAuthError> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|e| OAuthError::Protocol(format!("token response is not JSON: {e}")))?;
    let access_token = string(&value, "access_token")
        .ok_or_else(|| OAuthError::Protocol("token response had no access_token".into()))?;
    let expires_at = value
        .get("expires_in")
        .and_then(Value::as_i64)
        .map(|seconds| {
            (chrono::Utc::now() + chrono::Duration::seconds(seconds))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        });
    let scopes = match string(&value, "scope") {
        Some(scope) if !scope.trim().is_empty() => {
            scope.split_whitespace().map(str::to_string).collect()
        }
        _ => requested_scopes.to_vec(),
    };
    Ok(TokenBundle {
        access_token,
        refresh_token: string(&value, "refresh_token")
            .or_else(|| previous_refresh.map(str::to_string)),
        token_type: string(&value, "token_type").unwrap_or_else(|| "Bearer".into()),
        expires_at,
        scopes,
        client_id: client.client_id.clone(),
        client_secret: client.secret.clone(),
        token_endpoint: token_endpoint.to_string(),
        resources: resources.to_vec(),
        account: string(&value, "id_token")
            .as_deref()
            .and_then(id_token_email),
    })
}

/// The `email` claim of an OIDC id token, for display next to the chip.
///
/// The signature is not verified, and that is deliberate rather than lazy:
/// this token came back on the TLS connection to the token endpoint we just
/// discovered, which is the one case OIDC allows a client to skip validation.
/// It is also used for nothing but a label — no access decision reads it.
fn id_token_email(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let claims: Value = serde_json::from_slice(&base64url_decode(payload)?).ok()?;
    string(&claims, "email").or_else(|| string(&claims, "preferred_username"))
}

fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            b'=' => break,
            _ => return None,
        } as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// `Bearer resource_metadata="https://…"` → the URL.
fn resource_metadata_url(challenge: &str) -> Option<String> {
    let start = challenge.find("resource_metadata=")? + "resource_metadata=".len();
    let rest = &challenge[start..];
    let url = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next()?,
        None => rest.split(',').next()?.trim(),
    };
    (!url.is_empty()).then(|| url.to_string())
}

/// RFC 9728 / RFC 8414 well-known location: the suffix goes *before* the
/// resource path, not after it.
fn well_known(url: &str, suffix: &str) -> String {
    let (origin, path) = split_origin(url);
    let path = path.trim_end_matches('/');
    format!("{origin}/.well-known/{suffix}{path}")
}

fn origin_of(url: &str) -> String {
    split_origin(url).0
}

fn split_origin(url: &str) -> (String, String) {
    let Some((scheme, rest)) = url.split_once("://") else {
        return (url.to_string(), String::new());
    };
    match rest.find('/') {
        Some(index) => (
            format!("{scheme}://{}", &rest[..index]),
            rest[index..].to_string(),
        ),
        None => (format!("{scheme}://{rest}"), String::new()),
    }
}

/// The provider's own error, if it sent one — `error_description` first, then
/// `error`, then the status code.
fn error_detail(body: &str, status: u16) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    if let Some(value) = parsed {
        if let Some(description) = string(&value, "error_description") {
            return description;
        }
        if let Some(error) = string(&value, "error") {
            return error;
        }
        if let Some(message) = string(&value, "message") {
            return message;
        }
    }
    format!("HTTP {status}")
}

fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

fn string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> AuthServer {
        AuthServer {
            issuer: "https://auth.example.com".into(),
            authorization_endpoint: "https://auth.example.com/authorize".into(),
            token_endpoint: "https://auth.example.com/token".into(),
            registration_endpoint: Some("https://auth.example.com/register".into()),
            code_challenge_methods: vec!["S256".into()],
        }
    }

    fn client() -> OAuthClient {
        OAuthClient {
            client_id: "client-from-registration".into(),
            secret: None,
        }
    }

    #[test]
    fn authorize_url_carries_pkce_state_and_every_resource() {
        let pkce = Pkce::generate();
        let scopes = vec!["https://www.googleapis.com/auth/gmail.compose".to_string()];
        let resources = vec![
            "https://gmailmcp.googleapis.com/mcp/v1".to_string(),
            "https://calendarmcp.googleapis.com/mcp/v1".to_string(),
        ];
        let client = client();
        let url = authorize_url(
            &server(),
            &GrantContext {
                client: &client,
                redirect_uri: "http://127.0.0.1:49152/callback",
                scopes: &scopes,
                resources: &resources,
            },
            "state-123",
            &pkce,
            &[("access_type", "offline")],
        );

        assert!(url.starts_with("https://auth.example.com/authorize?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("code_challenge={}", pkce.challenge)));
        assert!(url.contains("state=state-123"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A49152%2Fcallback"));
        // Both, not just the first: the grant has to work against every MCP
        // server the provider serves, and RFC 8707 §2 lets `resource` repeat.
        assert!(url.contains("resource=https%3A%2F%2Fgmailmcp.googleapis.com%2Fmcp%2Fv1"));
        assert!(url.contains("resource=https%3A%2F%2Fcalendarmcp.googleapis.com%2Fmcp%2Fv1"));
        assert!(url.contains("access_type=offline"));
        // The verifier is the half that must never leave the host.
        assert!(!url.contains(&pkce.verifier));
    }

    #[test]
    fn well_known_locations_put_the_suffix_before_the_path() {
        assert_eq!(
            well_known(
                "https://gmailmcp.googleapis.com/mcp/v1",
                "oauth-protected-resource"
            ),
            "https://gmailmcp.googleapis.com/.well-known/oauth-protected-resource/mcp/v1"
        );
        assert_eq!(
            well_known("https://mcp.notion.com/mcp", "oauth-authorization-server"),
            "https://mcp.notion.com/.well-known/oauth-authorization-server/mcp"
        );
        assert_eq!(
            well_known("https://auth.example.com", "oauth-authorization-server"),
            "https://auth.example.com/.well-known/oauth-authorization-server"
        );
    }

    #[test]
    fn resource_metadata_is_read_from_the_challenge() {
        assert_eq!(
            resource_metadata_url(
                r#"Bearer realm="mcp", resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource""#
            )
            .as_deref(),
            Some("https://mcp.example.com/.well-known/oauth-protected-resource")
        );
        assert_eq!(resource_metadata_url("Bearer realm=\"mcp\""), None);
    }

    /// RFC 6749 §5.1: a refresh response may omit the refresh token, and the
    /// old one stays valid. Dropping it here would log the user out on the
    /// second refresh of every Google grant.
    #[test]
    fn refresh_keeps_the_previous_refresh_token() {
        let bundle = parse_token_response(
            r#"{"access_token":"new-access","token_type":"Bearer","expires_in":3599}"#,
            &client(),
            "https://auth.example.com/token",
            &["https://mcp.example.com/mcp".to_string()],
            &["scope-a".into()],
            Some("keep-me"),
        )
        .expect("parsed");

        assert_eq!(bundle.access_token, "new-access");
        assert_eq!(bundle.refresh_token.as_deref(), Some("keep-me"));
        assert_eq!(bundle.scopes, vec!["scope-a".to_string()]);
        assert!(bundle.expires_at.is_some());
        assert!(!bundle.needs_refresh(chrono::Utc::now()));
    }

    #[test]
    fn granted_scopes_win_over_requested_ones() {
        let bundle = parse_token_response(
            r#"{"access_token":"a","token_type":"Bearer","scope":"read write"}"#,
            &client(),
            "https://auth.example.com/token",
            &["https://mcp.example.com/mcp".to_string()],
            &["read".into(), "write".into(), "admin".into()],
            None,
        )
        .expect("parsed");
        assert_eq!(bundle.scopes, vec!["read".to_string(), "write".to_string()]);
        // No `expires_in` means no expiry to chase.
        assert!(bundle.expires_at.is_none());
        assert!(!bundle.needs_refresh(chrono::Utc::now()));
    }

    #[test]
    fn an_expired_or_unreadable_expiry_asks_for_a_refresh() {
        let mut bundle = parse_token_response(
            r#"{"access_token":"a","token_type":"Bearer","expires_in":10}"#,
            &client(),
            "https://auth.example.com/token",
            &["https://mcp.example.com/mcp".to_string()],
            &[],
            None,
        )
        .expect("parsed");
        // Ten seconds out is inside the skew window.
        assert!(bundle.needs_refresh(chrono::Utc::now()));

        bundle.expires_at = Some("not a timestamp".into());
        assert!(bundle.needs_refresh(chrono::Utc::now()));
    }

    #[test]
    fn a_token_response_without_a_token_is_an_error() {
        let err = parse_token_response(
            r#"{"error":"invalid_grant"}"#,
            &client(),
            "https://auth.example.com/token",
            &["https://mcp.example.com/mcp".to_string()],
            &[],
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("no access_token"), "{err}");
    }

    #[test]
    fn provider_errors_keep_the_provider_wording() {
        assert_eq!(
            error_detail(
                r#"{"error":"invalid_grant","error_description":"Code was already redeemed"}"#,
                400
            ),
            "Code was already redeemed"
        );
        assert_eq!(
            error_detail(r#"{"error":"access_denied"}"#, 400),
            "access_denied"
        );
        assert_eq!(error_detail("<html>nope</html>", 503), "HTTP 503");
    }

    /// The one classification that can cost a user their refresh token. A
    /// provider that never answered has not refused anything, and treating
    /// its silence as a refusal turns a Wi-Fi blip into a re-consent.
    #[test]
    fn only_a_provider_refusal_counts_as_a_dead_grant() {
        let refusal = |status| OAuthError::Provider {
            provider: "https://auth.example.com".into(),
            status,
            detail: "invalid_grant".into(),
        };
        assert!(refusal(400).is_grant_refusal());
        assert!(refusal(401).is_grant_refusal());

        // Rate limited, down, or misrouted: the grant is untouched by any of
        // these, and it has to still be there when the provider comes back.
        for status in [429, 500, 502, 503, 504, 404] {
            assert!(!refusal(status).is_grant_refusal(), "HTTP {status}");
        }
        assert!(!OAuthError::Http(HttpError::Transport {
            method: "POST",
            url: "https://auth.example.com/token".into(),
            detail: "could not resolve host".into(),
        })
        .is_grant_refusal());
        assert!(!OAuthError::Discovery("no metadata".into()).is_grant_refusal());
        assert!(!OAuthError::Protocol("token response is not JSON".into()).is_grant_refusal());
    }

    /// A grant is the one thing in this host that must not turn up in a log
    /// line, and `{:?}` is how it would.
    #[test]
    fn debug_output_redacts_the_tokens() {
        let bundle = TokenBundle {
            access_token: "ya29.super-secret-access".into(),
            refresh_token: Some("1//super-secret-refresh".into()),
            token_type: "Bearer".into(),
            expires_at: None,
            scopes: vec![],
            client_id: "client-1".into(),
            client_secret: None,
            token_endpoint: "https://auth.example.com/token".into(),
            resources: vec!["https://mcp.example.com/mcp".into()],
            account: Some("you@example.com".into()),
        };
        let rendered = format!("{bundle:?}");
        assert!(!rendered.contains("ya29.super-secret-access"), "{rendered}");
        assert!(!rendered.contains("1//super-secret-refresh"), "{rendered}");

        let pkce = Pkce::generate();
        assert!(!format!("{pkce:?}").contains(&pkce.verifier));
    }

    /// The chip wants to say *which* account is connected. The id token is the
    /// only place the token response offers one.
    #[test]
    fn the_account_label_comes_from_the_id_token() {
        // header.payload.signature, payload = {"email":"you@example.com"}
        let payload = super::base64url(br#"{"email":"you@example.com"}"#);
        let raw = format!(
            r#"{{"access_token":"a","token_type":"Bearer","id_token":"header.{payload}.sig"}}"#
        );
        let bundle = parse_token_response(
            &raw,
            &client(),
            "https://auth.example.com/token",
            &["https://mcp.example.com/mcp".to_string()],
            &[],
            None,
        )
        .expect("parsed");
        assert_eq!(bundle.account.as_deref(), Some("you@example.com"));

        // No id token is not an error; the chip just says "connected".
        let plain = parse_token_response(
            r#"{"access_token":"a","token_type":"Bearer"}"#,
            &client(),
            "https://auth.example.com/token",
            &["https://mcp.example.com/mcp".to_string()],
            &[],
            None,
        )
        .expect("parsed");
        assert!(plain.account.is_none());
    }

    #[test]
    fn s256_is_required() {
        let mut plain_only = server();
        plain_only.code_challenge_methods = vec!["plain".into()];
        assert!(!plain_only.supports_s256());
        // A server that lists nothing is not claiming to be plain-only; S256
        // is the OAuth 2.1 default and the request will say so.
        let mut silent = server();
        silent.code_challenge_methods = vec![];
        assert!(silent.supports_s256());
    }
}
