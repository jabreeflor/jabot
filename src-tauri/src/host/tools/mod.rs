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
mod crypto;
mod flow;
mod http;
mod loopback;
mod oauth;
mod servers;
#[cfg(test)]
mod testing;

use std::collections::{BTreeSet, HashMap};

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

        let flow = ConnectFlow::start(ConnectRequest {
            tool_id: entry.id.to_string(),
            provider,
            server_url: (*url).to_string(),
            scopes: entry.scopes.iter().map(|s| (*s).to_string()).collect(),
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
        let mut credentials: HashMap<&'static str, Result<String, String>> = HashMap::new();
        for provider in providers {
            credentials.insert(provider, self.bearer_for(provider));
        }

        let profile_root = self.data_dir.as_ref().map(|dir| dir.join(PROFILE_DIR));
        let plan = servers::plan(
            entries.iter().copied(),
            |entry| match entry.provider {
                None => Credential::None,
                Some(provider) => match credentials.get(provider.id) {
                    Some(Ok(header)) => Credential::Bearer(header),
                    Some(Err(reason)) => Credential::Missing(reason.clone()),
                    None => Credential::Missing(format!("{} is not connected", entry.label)),
                },
            },
            profile_root.as_deref(),
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
        plan.as_params()
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

    /// A live `Authorization` header for a provider, refreshing if it is time.
    fn bearer_for(&mut self, provider: &str) -> Result<String, String> {
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
            return Ok(bundle.authorization_header());
        }
        match oauth::refresh(&http::CurlClient, &bundle) {
            Ok(fresh) => {
                let json = serde_json::to_string(&fresh).map_err(|e| e.to_string())?;
                store
                    .refresh_tool_grant(secrets, provider, fresh.expires_at.as_deref(), &json)
                    .map_err(|e| e.to_string())?;
                Ok(fresh.authorization_header())
            }
            Err(err) => {
                // The grant is gone, not merely stale: say so on the chip
                // rather than sending an expired bearer and blaming the tool.
                let reason = format!("{label} needs to be reconnected: {err}");
                let _ = store.expire_tool_grant(secrets, provider, &reason);
                Err(reason)
            }
        }
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
                    "connected" => {
                        status = ToolConnectionStatus::Connected;
                        detail = Some(match &row.account {
                            Some(account) => format!("Connected as {account}"),
                            None => format!("Connected to {}", provider.label),
                        });
                    }
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
            })
            .expect("thread");
    }

    fn connect_gmail(session: &mut HostSession, auth: &LocalAuthServer) {
        let flow = ConnectFlow::start(ConnectRequest {
            tool_id: "gmail".into(),
            provider: catalog::GOOGLE,
            server_url: auth.mcp_url(),
            scopes: vec!["gmail.compose".into()],
            config_dir: None,
        })
        .expect("flow");
        let deadline = Instant::now() + Duration::from_secs(10);
        let authorize_url = loop {
            if let Some(url) = flow.authorize_url() {
                break url;
            }
            assert!(Instant::now() < deadline, "no authorize URL");
            std::thread::sleep(Duration::from_millis(10));
        };
        auth.consent(&authorize_url);
        session.connect_flows.insert("google".into(), flow);
        let deadline = Instant::now() + Duration::from_secs(10);
        while session.connect_flows.contains_key("google") {
            assert!(Instant::now() < deadline, "flow never committed");
            session.drain_connect_flows();
            std::thread::sleep(Duration::from_millis(10));
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
        connect_gmail(&mut session, &auth);

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
        connect_gmail(&mut session, &auth);

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

    /// Disconnecting is per provider, and it takes the tokens with it.
    #[test]
    fn disconnecting_gmail_disconnects_the_whole_google_grant() {
        let (mut session, _dir) = host();
        let auth = LocalAuthServer::start();
        connect_gmail(&mut session, &auth);

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
