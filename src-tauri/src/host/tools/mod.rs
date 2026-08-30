//! Tool / MCP connection framework (#18).
//!
//! One catalog, owned by the host, that crew bots and code threads both draw
//! from. The harness never decides which servers a session sees — the host
//! builds the `mcpServers` array on ACP `session/new` from the bot's allowlist
//! and passes it in, and ambient harness MCP is suppressed so host selection
//! is the only selection (decision #6).
//!
//! Three separate things, kept separate on purpose:
//!
//! | | What it is | Where it lives |
//! |---|---|---|
//! | Capability | A catalog entry a bot may call | [`catalog`] + `bots.tools_json` |
//! | Credential | An OAuth grant that authorises it | OS keychain + `tool_connections` |
//! | Connection | Whether the credential is usable today | [`HostSession::tools_list`] |
//!
//! Enforcement is by omission. A bot that does not allowlist Gmail does not
//! get a Gmail server on `session/new`, so the model never sees a schema it
//! is not allowed to call — see [`servers`]. Filtering a tool call after the
//! model has already made it is not enforcement; by then the request has
//! either been sent or the turn has been wasted arguing about it.

pub mod catalog;
mod clients;
pub(crate) mod crypto;
mod flow;
mod http;
mod loopback;
mod oauth;
mod servers;
#[cfg(test)]
mod testing;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use serde_json::Value;

use super::protocol::error::RpcError;
use super::protocol::methods::{
    ToolCardView, ToolConnectResult, ToolConnectionStatus, ToolDisconnectResult, ToolListResult,
    ToolRefParams, ToolTransport,
};
use super::store::{StoreError, ToolConnectionRow};
use super::HostSession;
use catalog::{Provider, ToolEntry, Transport};
pub(crate) use flow::ConnectFlow;
use flow::ConnectRequest;
use oauth::TokenBundle;
use servers::Credential;

/// Browser profiles and any other JaBot-owned MCP state, under the data dir.
const PROFILE_DIR: &str = "mcp-profiles";

impl HostSession {
    /// The catalog with today's connection status on each entry.
    pub fn tools_list(&mut self) -> Result<ToolListResult, RpcError> {
        self.drain_connect_flows();
        let connections = self.tool_connections();
        let tools = catalog::CATALOG
            .iter()
            .map(|entry| self.card(entry, &connections))
            .collect();
        Ok(ToolListResult { tools })
    }

    /// Start an OAuth flow for the grant this tool needs.
    ///
    /// Returns as soon as the flow is running. Consent happens in the user's
    /// browser and the host answers JSON-RPC on one thread, so blocking here
    /// would freeze every other thread in the app until someone found their
    /// password. The UI polls `tools/list` for `authorizeUrl` and the outcome.
    pub fn tools_connect(&mut self, params: ToolRefParams) -> Result<ToolConnectResult, RpcError> {
        self.drain_connect_flows();
        let entry = entry_or_invalid(&params.tool_id)?;
        let provider = connectable_provider(entry)?;
        let Transport::Http { url } = &entry.transport else {
            return Err(RpcError::InvalidParams(format!(
                "{} needs no connection",
                entry.label
            )));
        };
        // A grant is stored the moment consent lands, so a host with no store
        // would send the user through a browser flow and drop the result.
        if self.store.is_none() {
            return Err(RpcError::StoreUnavailable);
        }

        if let Some(existing) = self.connect_flows.get(provider.id) {
            return Ok(ToolConnectResult {
                tool_id: existing.tool_id.clone(),
                provider: provider.id.to_string(),
                status: ToolConnectionStatus::Connecting,
                authorize_url: existing.authorize_url(),
                redirect_uri: existing.redirect_uri.clone(),
                affects: affected_tools(provider.id),
            });
        }

        // The grant is per provider, so it is requested per provider: this one
        // consent is what Calendar and Drive will be handed too, and a token
        // minted with only Gmail's scopes and only Gmail's audience is a chip
        // that says connected over a server that refuses every call.
        let (scopes, resources) = provider_grant_shape(provider.id);
        let flow = ConnectFlow::start(ConnectRequest {
            tool_id: entry.id.to_string(),
            provider,
            server_url: (*url).to_string(),
            resources,
            scopes,
            config_dir: self.data_dir.clone(),
        })
        .map_err(RpcError::Internal)?;

        let result = ToolConnectResult {
            tool_id: entry.id.to_string(),
            provider: provider.id.to_string(),
            status: ToolConnectionStatus::Connecting,
            authorize_url: flow.authorize_url(),
            redirect_uri: flow.redirect_uri.clone(),
            affects: affected_tools(provider.id),
        };
        self.connect_flows.insert(provider.id.to_string(), flow);
        Ok(result)
    }

    /// Forget the grant behind this tool: vault bytes, pointer, and row.
    ///
    /// It disconnects the *provider*, which is more than the one chip the user
    /// clicked — there was only ever one Google login — so the result names
    /// everything that just lost access.
    pub fn tools_disconnect(
        &mut self,
        params: ToolRefParams,
    ) -> Result<ToolDisconnectResult, RpcError> {
        let entry = entry_or_invalid(&params.tool_id)?;
        let provider = connectable_provider(entry)?;
        if let Some(flow) = self.connect_flows.remove(provider.id) {
            flow.cancel();
        }
        let disconnected = match (&self.store, &mut self.secrets) {
            (Some(store), secrets) => store
                .delete_tool_grant(secrets, provider.id)
                .map_err(store_error)?,
            (None, _) => false,
        };
        Ok(ToolDisconnectResult {
            tool_id: entry.id.to_string(),
            provider: provider.id.to_string(),
            disconnected,
            affects: affected_tools(provider.id),
        })
    }

    /// The `mcpServers` array for a thread's ACP `session/new`.
    ///
    /// Deny by default: a thread with no bot, or a bot with an empty
    /// allowlist, gets an empty array. That is the correct answer, not a
    /// missing one — a session with no tools can still write code and talk.
    pub(crate) fn mcp_servers_for_thread(&mut self, thread_id: &str) -> Value {
        self.drain_connect_flows();
        let allowlist = self.tool_allowlist(thread_id);
        let entries: Vec<&'static ToolEntry> = allowlist
            .iter()
            .filter_map(|id| catalog::find(id))
            .collect();

        let providers: BTreeSet<&'static str> = entries
            .iter()
            .filter_map(|entry| entry.provider.map(|p| p.id))
            .collect();
        let mut grants: HashMap<&'static str, Result<Grant, String>> = HashMap::new();
        for provider in providers {
            grants.insert(provider, self.bearer_for(provider));
        }

        // Claimed before the plan is built, because a profile directory is a
        // lock and the answer depends on which other threads are live.
        let mut profiles: HashMap<&'static str, Result<PathBuf, String>> = HashMap::new();
        for entry in &entries {
            if entry.profile_flag().is_some() {
                profiles.insert(entry.id, self.claim_profile(thread_id, entry));
            }
        }

        let plan = servers::plan(
            entries.iter().copied(),
            |entry| match entry.provider {
                None => Credential::None,
                Some(provider) => match grants.get(provider.id) {
                    // A provider grant covers the provider, so it can still be
                    // short of what one chip needs — an older consent, or a
                    // user who unticked a scope on the provider's own screen.
                    // Passing the bearer anyway would be a server whose every
                    // call fails on insufficient scope.
                    Some(Ok(grant)) => match missing_scope(entry, &grant.scopes) {
                        None => Credential::Bearer(&grant.header),
                        Some(scope) => Credential::Missing(format!(
                            "the {} grant does not cover {scope}, which {} needs",
                            provider.label, entry.label
                        )),
                    },
                    Some(Err(reason)) => Credential::Missing(reason.clone()),
                    None => Credential::Missing(format!("{} is not connected", entry.label)),
                },
            },
            |entry| match profiles.get(entry.id) {
                Some(claim) => claim.clone(),
                None => Err(format!("{} was not offered a profile", entry.label)),
            },
            super::harness::resolve_command,
        );

        // A tool the bot was allowed to use and did not get is worth a line in
        // the host log: from inside the session it looks like the model simply
        // never tried.
        for skipped in &plan.skipped {
            eprintln!(
                "thread {thread_id}: {} was allowlisted but not passed — {}",
                skipped.tool_id, skipped.reason
            );
        }
        let mut servers = plan.as_params();
        // Chief's host tools ride the same array (#24). They are not catalog
        // entries and never were — `crew::HOST_TOOLS` is a separate list for
        // exactly this reason — but from the adapter's side one `mcpServers`
        // list is the only seam there is, so the host puts its own server on
        // it. Last, so a provider chip the user pressed is never displaced by
        // one they did not.
        if let (Some(array), Some(host_tools)) =
            (servers.as_array_mut(), self.chief_mcp_server(thread_id))
        {
            array.push(host_tools);
        }
        servers
    }

    /// A bot's `tools[]`, resolved through the thread.
    pub(crate) fn tool_allowlist(&self, thread_id: &str) -> Vec<String> {
        let Some(store) = &self.store else {
            return Vec::new();
        };
        let bot_id = store
            .get_thread(thread_id)
            .ok()
            .flatten()
            .and_then(|thread| thread.bot_id);
        let Some(bot_id) = bot_id else {
            return Vec::new();
        };
        store
            .get_bot(&bot_id)
            .ok()
            .flatten()
            .and_then(|bot| serde_json::from_str::<Vec<String>>(&bot.tools_json).ok())
            .unwrap_or_default()
    }

    /// A live grant for a provider, refreshing if it is time.
    fn bearer_for(&mut self, provider: &str) -> Result<Grant, String> {
        let label = provider_label(provider);
        let Some(store) = self.store.as_ref() else {
            return Err(format!("{label} is not connected"));
        };
        let secrets = &mut self.secrets;
        let raw = store
            .get_tool_grant(secrets, provider)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("{label} is not connected"))?;
        let bundle: TokenBundle = serde_json::from_str(&raw)
            .map_err(|_| format!("{label}'s saved grant is unreadable"))?;

        if !bundle.needs_refresh(chrono::Utc::now()) {
            return Ok(Grant::from(&bundle));
        }
        match oauth::refresh(&http::CurlClient, &bundle) {
            Ok(fresh) => {
                let json = serde_json::to_string(&fresh).map_err(|e| e.to_string())?;
                store
                    .refresh_tool_grant(secrets, provider, fresh.expires_at.as_deref(), &json)
                    .map_err(|e| e.to_string())?;
                Ok(Grant::from(&fresh))
            }
            // The provider said no: the grant is gone, not merely stale, and
            // the tokens go with it so the chip offers Connect rather than
            // sending a bearer that will be refused for the rest of the day.
            Err(err) if err.is_grant_refusal() => {
                let reason = format!("{label} needs to be reconnected: {err}");
                let _ = store.expire_tool_grant(secrets, provider, &reason);
                Err(reason)
            }
            // Nobody said no — the request never got an answer. Expiring here
            // would delete the refresh token, so a Wi-Fi blip or an hour of
            // provider downtime would cost the user a full re-consent (and,
            // for Google and Slack, their own `oauth_clients.json` again).
            // The row carries the reason and the vault keeps the grant, so the
            // next prompt simply tries the refresh again.
            Err(err) => {
                let reason = format!("{label} could not be refreshed: {err}");
                let _ = store.fail_tool_connection(provider, &reason);
                Err(reason)
            }
        }
    }

    /// Claim the JaBot-owned profile directory a local MCP server locks.
    ///
    /// `--user-data-dir` is a Chromium profile lock, not a preference: a
    /// second Playwright MCP pointed at the same directory dies inside the
    /// adapter, and JaBot runs one adapter per live thread with Browser chipped
    /// on three seeded bots, so overlapping runs are the normal case
    /// (`mcp-and-tools.md`: "One profile lock at a time"). The lease belongs to
    /// whichever thread still has an adapter — read off `connections` rather
    /// than tracked separately, because a lease that outlived the process it
    /// described would lock the tool out for the rest of the session.
    fn claim_profile(&mut self, thread_id: &str, entry: &ToolEntry) -> Result<PathBuf, String> {
        let root = self
            .data_dir
            .as_ref()
            .ok_or_else(|| {
                format!(
                    "{} needs a profile directory this host does not have",
                    entry.label
                )
            })?
            .join(PROFILE_DIR);
        if let Some(holder) = self.mcp_profiles.get(entry.id) {
            if holder != thread_id && self.has_adapter(holder) {
                return Err(format!(
                    "{} is in use by another thread: its profile takes one process at a time",
                    entry.label
                ));
            }
        }
        self.mcp_profiles
            .insert(entry.id.to_string(), thread_id.to_string());
        Ok(root.join(entry.id))
    }

    /// Commit finished OAuth flows. Called from every tools method and from
    /// session spawn, so a grant is live the moment the browser comes back.
    fn drain_connect_flows(&mut self) {
        let providers: Vec<String> = self.connect_flows.keys().cloned().collect();
        for provider in providers {
            let outcome = {
                let flow = &self.connect_flows[&provider];
                flow.take_outcome().or_else(|| {
                    flow.is_abandoned()
                        .then(|| Err("the sign-in window was not completed".to_string()))
                })
            };
            let Some(outcome) = outcome else { continue };
            self.connect_flows.remove(&provider);
            if let Err(err) = self.commit_grant(&provider, outcome) {
                eprintln!("failed to record the {provider} connection: {err}");
            }
        }
    }

    fn commit_grant(
        &mut self,
        provider: &str,
        outcome: Result<TokenBundle, String>,
    ) -> Result<(), StoreError> {
        let Some(store) = self.store.as_ref() else {
            return Ok(());
        };
        match outcome {
            Ok(bundle) => {
                let json = serde_json::to_string(&bundle)?;
                store.put_tool_grant(
                    &mut self.secrets,
                    provider,
                    bundle.account.as_deref(),
                    &bundle.scopes,
                    Some(&bundle.client_id),
                    bundle.expires_at.as_deref(),
                    &json,
                )?;
            }
            Err(message) => {
                store.fail_tool_connection(provider, &message)?;
            }
        }
        Ok(())
    }

    fn tool_connections(&self) -> HashMap<String, ToolConnectionRow> {
        self.store
            .as_ref()
            .and_then(|store| store.list_tool_connections().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|row| (row.provider.clone(), row))
            .collect()
    }

    fn card(
        &self,
        entry: &ToolEntry,
        connections: &HashMap<String, ToolConnectionRow>,
    ) -> ToolCardView {
        let (transport, mut status, mut detail) = match &entry.transport {
            Transport::HarnessExecute => (
                ToolTransport::HarnessExecute,
                ToolConnectionStatus::Connected,
                Some("Runs through the harness. Every command asks first.".to_string()),
            ),
            Transport::Stdio { command, .. } => match super::harness::resolve_command(command) {
                Some(path) => (
                    ToolTransport::Stdio,
                    ToolConnectionStatus::Connected,
                    Some(format!("Local server: {}", path.display())),
                ),
                None => (
                    ToolTransport::Stdio,
                    ToolConnectionStatus::Missing,
                    Some(format!("{command} is not installed")),
                ),
            },
            Transport::Http { .. } => (
                ToolTransport::Http,
                ToolConnectionStatus::NeedsAuth,
                Some("Not connected".to_string()),
            ),
        };

        let mut account = None;
        let mut expires_at = None;
        let mut authorize_url = None;
        let mut redirect_uri = None;

        if let Some(provider) = entry.provider {
            if let Some(flow) = self.connect_flows.get(provider.id) {
                status = ToolConnectionStatus::Connecting;
                detail = Some(format!("Waiting for {} sign-in", provider.label));
                authorize_url = flow.authorize_url();
                redirect_uri = Some(flow.redirect_uri.clone());
            } else if let Some(row) = connections.get(provider.id) {
                account = row.account.clone();
                expires_at = row.expires_at.clone();
                match row.status.as_str() {
                    // Connected to the *provider* is not the same as connected
                    // for this chip. Saying "Connected as you@example.com" on
                    // Calendar when the grant only carries Gmail's scopes is a
                    // trust claim the row cannot support, and the bot editor is
                    // exactly where a user decides what a bot can reach.
                    "connected" => match missing_scope(entry, &granted_scopes(row)) {
                        None => {
                            status = ToolConnectionStatus::Connected;
                            detail = Some(match &row.account {
                                Some(account) => format!("Connected as {account}"),
                                None => format!("Connected to {}", provider.label),
                            });
                        }
                        Some(scope) => {
                            status = ToolConnectionStatus::NeedsAuth;
                            detail = Some(format!(
                                "{} is connected, but not for {scope} — reconnect to add it",
                                provider.label
                            ));
                        }
                    },
                    "error" => {
                        status = ToolConnectionStatus::Error;
                        detail = row.last_error.clone();
                    }
                    _ => {
                        status = ToolConnectionStatus::NeedsAuth;
                        detail = row
                            .last_error
                            .clone()
                            .or(Some(format!("{} needs to be reconnected", provider.label)));
                    }
                }
            }
        }

        ToolCardView {
            id: entry.id.to_string(),
            label: entry.label.to_string(),
            blurb: entry.blurb.to_string(),
            transport,
            mcp: entry.is_mcp(),
            provider: entry.provider.map(|p| p.id.to_string()),
            provider_label: entry.provider.map(|p| p.label.to_string()),
            scopes: entry.scopes.iter().map(|s| (*s).to_string()).collect(),
            status,
            detail,
            account,
            expires_at,
            authorize_url,
            redirect_uri,
            docs_url: entry.docs_url.to_string(),
        }
    }
}

/// A usable provider grant: the header to send, and what it is good for.
///
/// The scopes travel with the header because the two are only meaningful
/// together — a bearer alone cannot say which of the provider's chips it can
/// actually serve.
struct Grant {
    header: String,
    scopes: Vec<String>,
}

/// Derived `Debug` would print the bearer, and this value exists to be put in
/// a `Result` that a failed `expect` renders. Same rule as [`TokenBundle`].
impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grant")
            .field("header", &"<redacted>")
            .field("scopes", &self.scopes)
            .finish()
    }
}

impl From<&TokenBundle> for Grant {
    fn from(bundle: &TokenBundle) -> Self {
        Self {
            header: bundle.authorization_header(),
            scopes: bundle.scopes.clone(),
        }
    }
}

/// What one consent has to ask for to cover a provider: the union of every
/// sibling chip's scopes, and every sibling MCP URL as an RFC 8707 `resource`.
///
/// Catalog order, deduplicated — a provider that repeats a scope across chips
/// (Google's `drive.file` in two products, say) would otherwise send it twice.
fn provider_grant_shape(provider_id: &str) -> (Vec<String>, Vec<String>) {
    let mut scopes: Vec<String> = Vec::new();
    let mut resources: Vec<String> = Vec::new();
    for entry in catalog::entries_for_provider(provider_id) {
        for scope in entry.scopes {
            let scope = (*scope).to_string();
            if !scopes.contains(&scope) {
                scopes.push(scope);
            }
        }
        if let Transport::Http { url } = &entry.transport {
            let url = (*url).to_string();
            if !resources.contains(&url) {
                resources.push(url);
            }
        }
    }
    (scopes, resources)
}

/// The first scope this entry needs that the grant does not carry.
fn missing_scope(entry: &ToolEntry, granted: &[String]) -> Option<&'static str> {
    entry
        .scopes
        .iter()
        .copied()
        .find(|scope| !granted.iter().any(|granted| granted == scope))
}

/// A row's `scopes_json`. Unreadable JSON reads as an empty grant rather than
/// an error: the honest answer to "what does this cover" is then "nothing".
fn granted_scopes(row: &ToolConnectionRow) -> Vec<String> {
    serde_json::from_str(&row.scopes_json).unwrap_or_default()
}

fn entry_or_invalid(tool_id: &str) -> Result<&'static ToolEntry, RpcError> {
    catalog::find(tool_id).ok_or_else(|| RpcError::InvalidParams(format!("unknown tool {tool_id}")))
}

/// Terminal has no provider and a local server has no grant; asking to connect
/// either is a UI bug worth naming rather than a silent no-op.
fn connectable_provider(entry: &ToolEntry) -> Result<Provider, RpcError> {
    entry.provider.ok_or_else(|| {
        RpcError::InvalidParams(format!(
            "{} has no connection to make: it is not a remote MCP server",
            entry.label
        ))
    })
}

fn affected_tools(provider_id: &str) -> Vec<String> {
    catalog::entries_for_provider(provider_id)
        .map(|entry| entry.id.to_string())
        .collect()
}

fn provider_label(provider_id: &str) -> &'static str {
    catalog::entries_for_provider(provider_id)
        .next()
        .and_then(|entry| entry.provider.map(|p| p.label))
        .unwrap_or("This provider")
}

fn store_error(err: StoreError) -> RpcError {
    RpcError::Internal(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::testing::LocalAuthServer;
    use super::*;
    use crate::host::store::NewThread;
    use crate::host::HostSession;
    use std::time::{Duration, Instant};

    fn host() -> (HostSession, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(dir.path());
        // The e2e and CI hosts run on Linux, where there is no keychain; a
        // process-local vault is what makes the flow exercisable there.
        session.secrets = crate::host::store::Secrets::memory();
        (session, dir)
    }

    fn open_thread(session: &HostSession, thread_id: &str, bot_id: &str) {
        session
            .store()
            .expect("store")
            .insert_thread(&NewThread {
                id: thread_id.into(),
                folder_id: None,
                bot_id: Some(bot_id.into()),
                harness_id: "claude".into(),
                cwd: "/tmp".into(),
                runtime_json: r#"{"command":"claude-agent-acp"}"#.into(),
                title: "t".into(),
                fold_policy: "default".into(),
                worktree_path: None,
                repo: Default::default(),
            })
            .expect("thread");
    }

    /// Run a whole Google grant through the host: real flow, real consent on a
    /// local authorization server, real commit into the vault and the store.
    ///
    /// `scopes` is what the consent is asked for, which is the whole question
    /// behind who ends up connected — the local server does not narrow it, so
    /// the grant carries exactly this.
    fn connect_google_for(
        session: &mut HostSession,
        auth: &LocalAuthServer,
        scopes: Vec<String>,
        consent: bool,
    ) {
        let flow = ConnectFlow::start(ConnectRequest {
            tool_id: "gmail".into(),
            provider: catalog::GOOGLE,
            server_url: auth.mcp_url(),
            resources: vec![auth.mcp_url()],
            scopes,
            config_dir: session.data_dir.clone(),
        })
        .expect("flow");
        if consent {
            let deadline = Instant::now() + Duration::from_secs(10);
            let authorize_url = loop {
                if let Some(url) = flow.authorize_url() {
                    break url;
                }
                assert!(Instant::now() < deadline, "no authorize URL");
                std::thread::sleep(Duration::from_millis(10));
            };
            auth.consent(&authorize_url);
        }
        session.connect_flows.insert("google".into(), flow);
        let deadline = Instant::now() + Duration::from_secs(10);
        while session.connect_flows.contains_key("google") {
            assert!(Instant::now() < deadline, "flow never finished");
            session.drain_connect_flows();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The stored Google grant, read the way the host reads it. Field access
    /// rather than `store()` so the vault can be borrowed mutably alongside.
    fn grant_json(session: &HostSession) -> Option<String> {
        session
            .store
            .as_ref()
            .expect("store")
            .get_tool_grant(&session.secrets, "google")
            .expect("read the grant")
    }

    fn stored_bundle(session: &HostSession) -> TokenBundle {
        serde_json::from_str(&grant_json(session).expect("a grant")).expect("a bundle")
    }

    fn store_bundle(session: &mut HostSession, bundle: &TokenBundle) {
        let json = serde_json::to_string(bundle).expect("serialise");
        session
            .store
            .as_ref()
            .expect("store")
            .refresh_tool_grant(&mut session.secrets, "google", None, &json)
            .expect("stamp the grant");
    }

    /// Consent to what `tools/connect` actually asks for: the union across
    /// Gmail, Calendar and Drive, because there is one Google login.
    fn connect_google(session: &mut HostSession, auth: &LocalAuthServer) {
        let (scopes, _) = provider_grant_shape("google");
        connect_google_for(session, auth, scopes, true);
    }

    /// Host-selected MCP has to be the *only* MCP a session sees (#10,
    /// decision #6). A harness that also reads servers from its own config
    /// would hand the model schemas no bot allowlisted, so it must be launched
    /// with the vendor's documented switch that turns that off. The catalog
    /// (#13) carries those switches as an env floor; this is the invariant
    /// that keeps them there, asserted from the side that depends on it.
    ///
    /// Hermes is the only harness in the catalog that documents such a switch.
    /// For the rest the host still passes its own array and never merges
    /// anything into it — but it cannot suppress what a vendor gives no flag
    /// for, and inventing an env var name would be a guess that does nothing.
    #[test]
    fn harnesses_with_ambient_mcp_are_launched_with_it_switched_off() {
        const SWITCHES: &[(&str, &str, &str)] =
            &[("hermes", "HERMES_ACP_SKIP_CONFIGURED_MCP", "1")];

        let catalog = crate::host::harness::catalog::compiled_in();
        for (harness_id, key, value) in SWITCHES {
            let descriptor = catalog
                .iter()
                .find(|card| card.id == *harness_id)
                .unwrap_or_else(|| panic!("{harness_id} is missing from the harness catalog"));
            assert_eq!(
                descriptor.env.get(*key).map(String::as_str),
                Some(*value),
                "{harness_id} would merge its own MCP servers into a JaBot session"
            );
        }
    }

    #[test]
    fn the_catalog_lists_every_prototype_chip_with_a_status() {
        let (mut session, _dir) = host();
        let tools = session.tools_list().unwrap().tools;
        let ids: Vec<&str> = tools.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["gmail", "calendar", "drive", "github", "browser", "notion", "slack", "terminal"]
        );

        let gmail = tools.iter().find(|t| t.id == "gmail").unwrap();
        assert_eq!(gmail.status, ToolConnectionStatus::NeedsAuth);
        assert_eq!(gmail.provider.as_deref(), Some("google"));
        assert!(gmail.mcp);

        let terminal = tools.iter().find(|t| t.id == "terminal").unwrap();
        assert_eq!(terminal.transport, ToolTransport::HarnessExecute);
        assert!(!terminal.mcp, "Terminal is not an MCP server");
        assert!(terminal.provider.is_none());
    }

    /// The enforcement claim at the host level: the array a session is spawned
    /// with contains what the bot allowlisted and nothing else.
    #[test]
    fn a_session_only_gets_the_tools_its_bot_allowlists() {
        let (mut session, _dir) = host();
        let auth = LocalAuthServer::start();
        connect_google(&mut session, &auth);

        // Inbox Mgr allowlists gmail; Scheduler allowlists calendar. Both draw
        // on the one Google grant, so this is enforcement and not just auth.
        open_thread(&session, "t-inbox", "inboxm");
        open_thread(&session, "t-sched", "sched");

        let inbox = session.mcp_servers_for_thread("t-inbox");
        let names: Vec<&str> = inbox
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["gmail"]);
        assert!(!inbox.to_string().contains("calendar"));

        let sched = session.mcp_servers_for_thread("t-sched");
        let names: Vec<&str> = sched
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["calendar"]);
    }

    #[test]
    fn a_thread_with_no_bot_gets_no_tools() {
        let (mut session, _dir) = host();
        session
            .store()
            .unwrap()
            .insert_thread(&NewThread {
                id: "t-loose".into(),
                folder_id: None,
                bot_id: None,
                harness_id: "claude".into(),
                cwd: "/tmp".into(),
                runtime_json: r#"{"command":"claude-agent-acp"}"#.into(),
                title: "t".into(),
                fold_policy: "default".into(),
                worktree_path: None,
                repo: Default::default(),
            })
            .unwrap();
        assert_eq!(
            session.mcp_servers_for_thread("t-loose"),
            serde_json::json!([])
        );
        assert_eq!(
            session.mcp_servers_for_thread("no-such-thread"),
            serde_json::json!([])
        );
    }

    /// Terminal is the Code bot's second chip. It must never turn into a
    /// server, and GitHub without a grant must not be passed unauthenticated.
    #[test]
    fn terminal_is_never_passed_as_a_server() {
        let (mut session, _dir) = host();
        open_thread(&session, "t-code", "code");
        assert_eq!(
            session.mcp_servers_for_thread("t-code"),
            serde_json::json!([])
        );
    }

    /// A completed consent has to land in the vault and in the row, and the
    /// chip has to change on the strength of it.
    #[test]
    fn a_completed_grant_connects_every_tool_that_shares_it() {
        let (mut session, _dir) = host();
        let auth = LocalAuthServer::start();
        connect_google(&mut session, &auth);

        let tools = session.tools_list().unwrap().tools;
        for id in ["gmail", "calendar", "drive"] {
            let tool = tools.iter().find(|t| t.id == id).unwrap();
            assert_eq!(tool.status, ToolConnectionStatus::Connected, "{id}");
            assert_eq!(tool.account.as_deref(), Some("you@example.com"), "{id}");
        }
        // A different provider is untouched by a Google grant.
        let notion = tools.iter().find(|t| t.id == "notion").unwrap();
        assert_eq!(notion.status, ToolConnectionStatus::NeedsAuth);

        let row = session
            .store()
            .unwrap()
            .get_tool_connection("google")
            .unwrap()
            .expect("a row");
        assert_eq!(row.status, "connected");
        assert!(row.secret_ref_id.is_some(), "the row points at the vault");
    }

    /// The consent has to be asked for on behalf of every chip the grant will
    /// serve, or the other two are connected in name only.
    #[test]
    fn one_consent_asks_for_every_scope_and_resource_the_provider_serves() {
        let (scopes, resources) = provider_grant_shape("google");
        for entry in catalog::entries_for_provider("google") {
            for scope in entry.scopes {
                assert!(
                    scopes.contains(&(*scope).to_string()),
                    "{} needs {scope}",
                    entry.id
                );
            }
            let Transport::Http { url } = &entry.transport else {
                unreachable!("every Google entry is a remote server")
            };
            assert!(resources.contains(&(*url).to_string()), "{}", entry.id);
        }
        assert_eq!(resources.len(), 3, "one resource per Google MCP server");

        // A provider with one chip asks for one resource and nothing extra.
        let (notion_scopes, notion_resources) = provider_grant_shape("notion");
        assert!(notion_scopes.is_empty());
        assert_eq!(notion_resources, vec!["https://mcp.notion.com/mcp"]);
    }

    /// The dishonest state this guards against: a grant that only covers Gmail
    /// lighting up Calendar and Drive, and then handing them a bearer their
    /// server will refuse on every call.
    #[test]
    fn a_grant_that_misses_a_chips_scopes_does_not_connect_that_chip() {
        let (mut session, _dir) = host();
        let auth = LocalAuthServer::start();
        let gmail_only: Vec<String> = catalog::find("gmail")
            .unwrap()
            .scopes
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        connect_google_for(&mut session, &auth, gmail_only, true);

        let tools = session.tools_list().unwrap().tools;
        let gmail = tools.iter().find(|t| t.id == "gmail").unwrap();
        assert_eq!(gmail.status, ToolConnectionStatus::Connected);

        for id in ["calendar", "drive"] {
            let tool = tools.iter().find(|t| t.id == id).unwrap();
            assert_eq!(tool.status, ToolConnectionStatus::NeedsAuth, "{id}");
            let detail = tool.detail.as_deref().expect("a reason");
            assert!(detail.contains("reconnect"), "{id}: {detail}");
        }

        // And the session is denied the server, not given a dead one.
        open_thread(&session, "t-sched", "sched");
        assert_eq!(
            session.mcp_servers_for_thread("t-sched"),
            serde_json::json!([])
        );
        open_thread(&session, "t-inbox", "inboxm");
        let inbox = session.mcp_servers_for_thread("t-inbox");
        assert_eq!(inbox.as_array().unwrap().len(), 1);
    }

    /// A refresh that never reached the provider must not cost the user their
    /// grant: the refresh token stays in the vault so the next prompt can try
    /// again, and only the chip carries the bad news.
    #[test]
    fn a_refresh_that_could_not_be_delivered_keeps_the_grant() {
        let (mut session, _dir) = host();
        let auth = LocalAuthServer::start();
        connect_google(&mut session, &auth);

        // Age the access token out and point the refresh at a port nobody is
        // listening on — the offline case, without unplugging the test host.
        let mut bundle = stored_bundle(&session);
        bundle.expires_at = Some("2020-01-01T00:00:00Z".into());
        bundle.token_endpoint = "http://127.0.0.1:1/token".into();
        store_bundle(&mut session, &bundle);

        let reason = session.bearer_for("google").unwrap_err();
        assert!(reason.contains("could not be refreshed"), "{reason}");

        // The refresh token is still there, which is the whole point.
        let kept = stored_bundle(&session);
        assert_eq!(kept.refresh_token, bundle.refresh_token);
        assert!(kept.refresh_token.is_some());

        let tools = session.tools_list().unwrap().tools;
        let gmail = tools.iter().find(|t| t.id == "gmail").unwrap();
        assert_eq!(gmail.status, ToolConnectionStatus::Error);
        assert_eq!(gmail.detail.as_deref(), Some(reason.as_str()));
    }

    /// A provider that actually refuses the refresh is the other half: that
    /// grant is dead, and keeping the tokens would mean sending a bearer the
    /// provider has already said no to.
    #[test]
    fn a_refused_refresh_expires_the_grant() {
        let (mut session, _dir) = host();
        let auth = LocalAuthServer::start();
        connect_google(&mut session, &auth);

        // A token endpoint that answers 400 `invalid_grant`, which is what a
        // revoked grant gets back and the only answer that may destroy one.
        let mut bundle = stored_bundle(&session);
        bundle.expires_at = Some("2020-01-01T00:00:00Z".into());
        bundle.token_endpoint = auth.refusing_token_endpoint();
        store_bundle(&mut session, &bundle);

        let reason = session.bearer_for("google").unwrap_err();
        assert!(reason.contains("needs to be reconnected"), "{reason}");
        assert!(
            grant_json(&session).is_none(),
            "a refused grant keeps no tokens"
        );
        let tools = session.tools_list().unwrap().tools;
        let gmail = tools.iter().find(|t| t.id == "gmail").unwrap();
        assert_eq!(gmail.status, ToolConnectionStatus::NeedsAuth);
    }

    /// Disconnecting is per provider, and it takes the tokens with it.
    #[test]
    fn disconnecting_gmail_disconnects_the_whole_google_grant() {
        let (mut session, _dir) = host();
        let auth = LocalAuthServer::start();
        connect_google(&mut session, &auth);

        let result = session
            .tools_disconnect(ToolRefParams {
                tool_id: "gmail".into(),
            })
            .unwrap();
        assert!(result.disconnected);
        assert_eq!(result.affects, vec!["gmail", "calendar", "drive"]);

        assert!(session
            .store()
            .unwrap()
            .get_tool_connection("google")
            .unwrap()
            .is_none());
        assert!(session
            .store()
            .unwrap()
            .list_secret_refs()
            .unwrap()
            .is_empty());

        open_thread(&session, "t-inbox", "inboxm");
        assert_eq!(
            session.mcp_servers_for_thread("t-inbox"),
            serde_json::json!([])
        );
    }

    /// A flow that fails has to leave the reason where the user will see it —
    /// on the chip — instead of a chip that quietly says "not connected" while
    /// the actual problem is a file only they can write.
    #[test]
    fn a_failed_connect_leaves_the_providers_reason_on_the_chip() {
        let (mut session, _dir) = host();
        let auth = LocalAuthServer::start();
        auth.disable_dynamic_registration();

        let (scopes, _) = provider_grant_shape("google");
        connect_google_for(&mut session, &auth, scopes, false);

        let tools = session.tools_list().unwrap().tools;
        let gmail = tools.iter().find(|t| t.id == "gmail").unwrap();
        assert_eq!(gmail.status, ToolConnectionStatus::Error);
        let detail = gmail.detail.as_deref().expect("a reason");
        assert!(detail.contains("oauth_clients.json"), "{detail}");

        // And a failed connect grants nothing.
        open_thread(&session, "t-inbox", "inboxm");
        assert_eq!(
            session.mcp_servers_for_thread("t-inbox"),
            serde_json::json!([])
        );
    }

    /// A Chromium `--user-data-dir` takes one process at a time, and three
    /// seeded bots chip Browser, so two live threads asking for it at once is
    /// the ordinary case rather than an edge one. The second must be told no
    /// here, where it becomes a skip with a reason, instead of inside the
    /// adapter as a profile-lock crash.
    #[test]
    fn one_browser_profile_is_held_by_one_live_thread_at_a_time() {
        let (mut session, _dir) = host();
        let browser = catalog::find("browser").expect("browser");

        let first = session
            .claim_profile("t-research", browser)
            .expect("the first claim");
        // The same thread re-claiming — a respawned adapter — keeps its own.
        assert_eq!(
            session.claim_profile("t-research", browser).unwrap(),
            first,
            "a thread does not lock itself out"
        );

        // With a live adapter behind the lease, nobody else may have it.
        session.attach_adapter_for_test("t-research", idle_adapter(&session), "sess-idle");
        let refused = session
            .claim_profile("t-talent", browser)
            .expect_err("the profile is taken");
        assert!(refused.contains("in use by another thread"), "{refused}");

        // When that thread's adapter is gone, so is its claim on the profile.
        session.drop_adapter("t-research");
        assert_eq!(session.claim_profile("t-talent", browser).unwrap(), first);
    }

    /// A process that holds stdin open and answers nothing: enough to stand
    /// for a live adapter without pretending to speak ACP.
    fn idle_adapter(session: &HostSession) -> crate::host::acp::AcpConnection {
        let runtime =
            crate::host::acp::HarnessRuntime::from_runtime_json("idle", r#"{"command":"cat"}"#)
                .expect("runtime");
        crate::host::acp::AcpConnection::spawn(
            &runtime,
            None,
            &session.log_dir.join("idle.log"),
            std::sync::Arc::clone(&session.wake),
        )
        .expect("spawn")
    }

    #[test]
    fn connecting_something_that_is_not_a_remote_server_is_refused() {
        let (mut session, _dir) = host();
        for tool_id in ["terminal", "browser"] {
            let err = session
                .tools_connect(ToolRefParams {
                    tool_id: tool_id.into(),
                })
                .unwrap_err();
            assert!(
                matches!(err, RpcError::InvalidParams(_)),
                "{tool_id}: {err:?}"
            );
        }
        let err = session
            .tools_connect(ToolRefParams {
                tool_id: "nope".into(),
            })
            .unwrap_err();
        assert!(err.to_string().contains("unknown tool"), "{err}");
    }
}
