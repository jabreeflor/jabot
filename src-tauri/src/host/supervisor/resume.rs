//! The resume recipe (`session-lifecycle/keep-alive.md`).
//!
//! > Do not `session/new`. That orphans the conversation.
//!
//! Every path that finds a thread with a stored ACP session and no live
//! adapter comes through here: an explicit `thread/resume`, and the first
//! prompt after a quit, a crash, or an idle evict. The order is the research's
//! own — spawn the snapshotted runtime, `initialize`, then `session/resume` if
//! the agent advertises it, `session/load` if it only has that, and a new
//! session only once both have been ruled out and the user has been told.
//!
//! Two refusals matter more than the happy path.
//!
//! **Drift.** #15 persists a fingerprint of the five inputs a session was
//! created with. If any of them moved, the conversation on disk is not the job
//! that would be spawned now, and resuming it would continue someone else's
//! work under this thread's title. So drift is reported, not resumed.
//!
//! **A missing `cwd`.** `keep-alive.md` is explicit: refuse and resurface
//! `failed` ("folder missing"). Silently resuming in a different directory —
//! or worse, in the host's own — is how an agent gets told to edit files that
//! are not there.

use std::path::Path;

use super::super::acp::Inbound;
use super::super::lifecycle::receipt::drift;
use super::super::protocol::error::RpcError;
use super::super::protocol::methods::{
    PromptParams, ResumeOutcome, ResurfaceReason, ThreadRefParams, ThreadResumeResult,
};
use super::super::store::ThreadRow;
use super::super::HostSession;

/// Whether a stored session could be handed back, and what stops it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResumeReadiness {
    pub resumable: bool,
    pub drift: Vec<String>,
}

/// How a restore attempt ended, before it is dressed up as a wire result.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Restored {
    Resumed,
    Loaded,
    /// The adapter speaks neither verb, or the one it speaks refused.
    Unsupported(Option<String>),
}

impl HostSession {
    /// Could `thread/resume` put this thread's conversation back right now?
    ///
    /// Read-only and cheap — two store reads and a `stat` — because
    /// `thread/state` answers it on every poll.
    pub(crate) fn resume_readiness(&self, thread: &ThreadRow) -> ResumeReadiness {
        let Some(store) = self.store.as_ref() else {
            return ResumeReadiness::default();
        };
        let Ok(Some(receipt)) = store.get_session_receipt(&thread.id) else {
            // No receipt means nothing was ever spawned for this thread, or it
            // was spawned by a build that predates #15. Either way there is
            // nothing to check a resume against.
            return ResumeReadiness::default();
        };
        let drifted: Vec<String> = drift(&receipt, &self.fingerprint_for(thread))
            .into_iter()
            .map(|field| field.as_str().to_string())
            .collect();
        ResumeReadiness {
            resumable: drifted.is_empty()
                && thread.acp_session_id.is_some()
                && thread.deleted_at.is_none()
                && Path::new(&receipt.cwd).is_dir(),
            drift: drifted,
        }
    }

    pub(crate) fn resume_thread(
        &mut self,
        params: ThreadRefParams,
    ) -> Result<ThreadResumeResult, RpcError> {
        let thread_id = params.thread_id.clone();
        let thread = self
            .lifecycle_thread(&thread_id)?
            .ok_or_else(|| RpcError::ThreadNotFound(thread_id.clone()))?;
        if thread.deleted_at.is_some() {
            return Err(RpcError::IllegalTransition {
                thread_id,
                from: "deleted".into(),
                action: "resume".into(),
            });
        }

        // Already attached. Reporting `live` rather than tearing the process
        // down and building it again is the difference between reopening a
        // folded thread and interrupting it.
        if let Some(session_id) = self
            .conn(&thread_id)
            .and_then(|conn| conn.session_for(&thread_id))
        {
            return self.resume_result(
                &thread_id,
                ResumeOutcome::Live,
                Some(session_id),
                Vec::new(),
                None,
            );
        }

        let Some(session_id) = thread.acp_session_id.clone() else {
            return self.resume_result(
                &thread_id,
                ResumeOutcome::NoSession,
                None,
                Vec::new(),
                Some("this thread has never had an ACP session".into()),
            );
        };

        let readiness = self.resume_readiness(&thread);
        if !readiness.drift.is_empty() {
            let detail = format!(
                "{} changed since this session was created",
                readiness.drift.join(", ")
            );
            return self.resume_result(
                &thread_id,
                ResumeOutcome::Drifted,
                Some(session_id),
                readiness.drift,
                Some(detail),
            );
        }
        if !Path::new(&thread.cwd).is_dir() {
            // `keep-alive.md`: refuse, and say the folder is gone. The thread
            // is not broken — the checkout moved — and a `failed` card with
            // that sentence is the only thing that leads anywhere.
            let detail = format!("{} is gone", thread.cwd);
            self.try_resurface(&thread_id, ResurfaceReason::Failed, &detail, None);
            return self.resume_result(
                &thread_id,
                ResumeOutcome::CwdMissing,
                Some(session_id),
                Vec::new(),
                Some(detail),
            );
        }

        match self.restore_session(&thread_id, &thread.cwd, &session_id)? {
            Restored::Resumed => self.resume_result(
                &thread_id,
                ResumeOutcome::Resumed,
                Some(session_id),
                Vec::new(),
                None,
            ),
            Restored::Loaded => self.resume_result(
                &thread_id,
                ResumeOutcome::Loaded,
                Some(session_id),
                Vec::new(),
                None,
            ),
            Restored::Unsupported(detail) => {
                // A process holding no session is worse than no process: it
                // looks connected and can answer nothing. Drop it and let the
                // next prompt start a session honestly.
                self.drop_adapter(&thread_id);
                self.lifecycle_on_detached(&thread_id);
                self.resume_result(
                    &thread_id,
                    ResumeOutcome::Unsupported,
                    Some(session_id),
                    Vec::new(),
                    Some(detail.unwrap_or_else(|| {
                        "this adapter speaks neither session/resume nor session/load".into()
                    })),
                )
            }
        }
    }

    /// The session a prompt should be sent on when the connection is new.
    ///
    /// Resume first, then load, then — only then — `session/new`. The fallback
    /// is not a failure the user has to be walked through: it is what happens
    /// when the job itself changed, or when the adapter cannot restore. What
    /// makes it honest is that the receipt is rewritten with the new session,
    /// so `thread/state` stops claiming the old conversation is resumable.
    pub(crate) fn attach_session(
        &mut self,
        thread_id: &str,
        cwd: &str,
    ) -> Result<String, RpcError> {
        let stored = self
            .lifecycle_thread(thread_id)
            .ok()
            .flatten()
            .and_then(|thread| {
                let readiness = self.resume_readiness(&thread);
                readiness
                    .resumable
                    .then(|| thread.acp_session_id.clone())
                    .flatten()
            });
        if let Some(session_id) = stored {
            match self.restore_session(thread_id, cwd, &session_id) {
                Ok(Restored::Resumed | Restored::Loaded) => return Ok(session_id),
                Ok(Restored::Unsupported(detail)) => {
                    if let Some(detail) = detail {
                        eprintln!("resume of {thread_id} fell back to a new session: {detail}");
                    }
                }
                Err(err) => {
                    // The connection is still up — only the restore failed —
                    // so a new session is still reachable and is better than
                    // refusing the user's prompt outright.
                    eprintln!("resume of {thread_id} failed: {err}");
                }
            }
        }
        // Host-selected MCP, from the bot's allowlist (#18). Resolved here
        // because `session/new` fixes the tool surface for the life of the
        // session: a tool that is not in this array is one the model never
        // sees a schema for.
        let mcp_servers = self.mcp_servers_for_thread(thread_id);
        let conn = self
            .conn_mut(thread_id)
            .ok_or_else(|| RpcError::Internal(format!("no adapter for thread {thread_id}")))?;
        conn.new_session(thread_id, cwd, mcp_servers)
    }

    /// Spawn (if needed), `initialize`, and hand the session back.
    fn restore_session(
        &mut self,
        thread_id: &str,
        cwd: &str,
        session_id: &str,
    ) -> Result<Restored, RpcError> {
        self.ensure_connection(&PromptParams {
            thread_id: thread_id.to_string(),
            content: serde_json::Value::Null,
            mode: None,
            cwd: Some(cwd.to_string()),
            harness_id: None,
            runtime: None,
        })?;
        let mcp_servers = self.mcp_servers_for_thread(thread_id);
        let capabilities = {
            let conn = self
                .conn_mut(thread_id)
                .ok_or_else(|| RpcError::Internal(format!("no adapter for thread {thread_id}")))?;
            // Capabilities are only knowable after the handshake, and the
            // handshake failing is the "install hint" case, not a resume case.
            conn.initialize()?;
            conn.capabilities()
        };

        if capabilities.resume {
            let conn = self
                .conn_mut(thread_id)
                .ok_or_else(|| RpcError::Internal(format!("no adapter for thread {thread_id}")))?;
            match conn.resume_session(thread_id, session_id, cwd, mcp_servers.clone()) {
                Ok(()) => {
                    self.lifecycle_on_attached(thread_id);
                    return Ok(Restored::Resumed);
                }
                // Advertised and then refused. Fall through to load rather
                // than give up: the adapter still has the conversation.
                Err(err) => eprintln!("session/resume for {thread_id} failed: {err}"),
            }
        }

        if capabilities.load_session {
            // A load replays the whole conversation as `session/update`
            // notifications. We keep our own transcript (#14), so replaying
            // into a thread that already has one would draw every message
            // twice — `keep-alive.md` step 4: "replay into renderer **only**
            // if our overlay transcript is empty".
            let replay_wanted = self
                .store
                .as_ref()
                .and_then(|store| store.transcript_head(thread_id).ok())
                .unwrap_or(0)
                == 0;
            let conn = self
                .conn_mut(thread_id)
                .ok_or_else(|| RpcError::Internal(format!("no adapter for thread {thread_id}")))?;
            match conn.load_session(thread_id, session_id, cwd, mcp_servers) {
                Ok(()) => {
                    self.settle_replay(thread_id, replay_wanted);
                    self.lifecycle_on_attached(thread_id);
                    return Ok(Restored::Loaded);
                }
                Err(err) => return Ok(Restored::Unsupported(Some(err.to_string()))),
            }
        }

        Ok(Restored::Unsupported(None))
    }

    /// Deal with what `session/load` pushed at us while it was running.
    ///
    /// The replay is already sitting in the connection's queue by the time the
    /// response lands, so this is the one moment it can be dropped as a unit.
    /// Everything that is *not* a transcript update is handled normally: an
    /// adapter that died mid-load has to reach the same code path as any other
    /// adapter that died.
    fn settle_replay(&mut self, thread_id: &str, replay_wanted: bool) {
        let mut carried = Vec::new();
        // Only this thread's replay is dropped. On a shared process a
        // neighbour's chunks can be in the same queue, and they are not part of
        // anybody's load — so they are routed and carried like any other event.
        if let Some(conn) = self.conn_mut(thread_id) {
            let mine = conn.session_for(thread_id);
            while let Ok(event) = conn.try_recv() {
                let owners = conn.route(&event);
                let is_my_replay = mine.is_some()
                    && matches!(event, Inbound::Update(_))
                    && owners.iter().any(|owner| owner == thread_id);
                if is_my_replay && !replay_wanted {
                    continue;
                }
                match owners.len() {
                    1 => carried.push((owners.into_iter().next().expect("one"), event)),
                    _ => carried.push((thread_id.to_string(), event)),
                }
            }
        }
        for (owner, event) in carried {
            self.handle_inbound(&owner, event);
        }
    }

    fn resume_result(
        &mut self,
        thread_id: &str,
        outcome: ResumeOutcome,
        acp_session_id: Option<String>,
        drift: Vec<String>,
        detail: Option<String>,
    ) -> Result<ThreadResumeResult, RpcError> {
        let state = self.thread_state(ThreadRefParams {
            thread_id: thread_id.to_string(),
        })?;
        Ok(ThreadResumeResult {
            thread_id: thread_id.to_string(),
            resumed: outcome.is_attached(),
            outcome,
            acp_session_id,
            drift,
            detail,
            state,
        })
    }
}
