//! ACP client + adapter subprocess supervisor (#10).
//!
//! The UI never talks to ACP stdio. This module spawns one adapter process
//! per live JaBot thread, speaks ACP v1 JSON-RPC over newline-delimited
//! stdio, and forwards `session/update` / `session/request_permission` onto
//! the host notification bus.

mod connection;
mod runtime;
mod spawn;
mod wake;

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::permission::Withdrawal;
use super::protocol::error::RpcError;
use super::protocol::methods::{
    PromptParams, PromptResult, ResurfaceReason, SessionCancelParams, SessionCancelResult,
};
use super::HostSession;
use runtime::ProbeResult;

pub(crate) use connection::{AcpConnection, Inbound};
/// The Doctor's deep probe spawns an adapter the same way a session does (#13).
pub(crate) use runtime::HarnessRuntime;
pub use wake::AdapterWake;

impl HostSession {
    pub fn session_prompt(&mut self, params: PromptParams) -> Result<PromptResult, RpcError> {
        let thread_id = params.thread_id.clone();
        // Before anything is spawned: a turn already in flight owns this
        // session, and starting a second one loses whichever result comes back
        // second (#15). What happens instead is the client's call — refuse,
        // queue, or interrupt — and #14 owns that fork (`transcript::queue`).
        if let Some(queued) = self.intercept_in_flight(&params)? {
            return Ok(queued);
        }
        // Read before anything is spawned, because whether a session already
        // exists is what decides the next question. A freshly spawned
        // connection never has one, so asking first is the same answer.
        let existing = self
            .connections
            .get(&thread_id)
            .and_then(|conn| conn.session_id.clone());
        let cwd = self.resolve_cwd(&params)?;
        if existing.is_none() && !Path::new(&cwd).is_dir() {
            // `keep-alive.md`'s resume recipe, applied to the path a user
            // actually takes after a restart — reopen the thread and type.
            // `thread/resume` already refuses a vanished folder; without the
            // same refusal here the spawner would silently drop `current_dir`
            // and hand the agent JaBot's own directory, then `session/new`
            // would overwrite the receipt and put the real conversation out of
            // reach. Refusing before either happens is the whole point.
            let detail = format!("{cwd} is gone");
            self.try_resurface(&thread_id, ResurfaceReason::Failed, &detail, None);
            return Err(RpcError::CwdMissing { thread_id, cwd });
        }
        self.ensure_connection(&params)?;
        let session_id = match existing {
            Some(id) => id,
            None => {
                // A fresh process on a thread that already has a session is
                // the ordinary case after a quit, a crash, or an idle evict —
                // and `session/new` there would orphan the conversation
                // (`keep-alive.md`: "Do not session/new. That orphans the
                // conversation."). The supervisor resumes when it can and says
                // so when it cannot; only then is a new session minted (#21).
                match self.attach_session(&thread_id, &cwd) {
                    Ok(id) => id,
                    Err(err) => {
                        self.connections.remove(&thread_id);
                        return Err(err);
                    }
                }
            }
        };
        if let Some(store) = &self.store {
            let _ = store.set_thread_acp_session(&thread_id, &session_id);
        }
        if let Err(err) = self
            .connections
            .get_mut(&thread_id)
            .expect("spawned")
            .send_prompt(&session_id, &params.content)
        {
            self.connections.remove(&thread_id);
            return Err(err);
        }
        // The user's own words go into the transcript here, as the ACP
        // `user_message_chunk` an agent would have sent. Without it a reopened
        // thread replays the agent's half of a conversation and none of the
        // human's — the transcript overlay has to be the whole exchange (#14).
        self.record_user_prompt(&thread_id, &params.content);
        // The run ledger opens here, not when the first chunk arrives: the turn
        // exists from the moment the agent accepted the prompt, and a crash
        // before any output still has to show up as a run that failed (#15).
        self.lifecycle_run_started(&thread_id, &session_id);
        self.pump_acp();
        Ok(PromptResult {
            thread_id,
            acp_session_id: session_id,
            accepted: true,
            queued: false,
            queue_position: None,
        })
    }

    pub fn session_cancel(
        &mut self,
        params: SessionCancelParams,
    ) -> Result<SessionCancelResult, RpcError> {
        let thread_id = params.thread_id.clone();
        let Some(conn) = self.connections.get_mut(&thread_id) else {
            return Err(RpcError::Internal(format!(
                "no live adapter for thread {thread_id}"
            )));
        };
        let Some(session_id) = conn.session_id.clone() else {
            return Err(RpcError::Internal(format!(
                "adapter for {thread_id} has no ACP session"
            )));
        };
        // Order matters, and it is the reverse of the obvious one. #10:
        // "Cancellation resolves outstanding permission requests with
        // `cancelled` before `session/cancel`". An agent blocked on a
        // permission it never gets an answer to has no reason to act on the
        // cancel, so answering first is what actually unblocks the turn.
        // `withdraw_pending_permissions` borrows self, so the connection is
        // re-fetched after it rather than held across the call.
        self.withdraw_pending_permissions(&thread_id, "session/cancel", Withdrawal::Cancelled);
        let Some(conn) = self.connections.get_mut(&thread_id) else {
            return Err(RpcError::Internal(format!(
                "no live adapter for thread {thread_id}"
            )));
        };
        conn.cancel(&session_id)?;
        // Start the stopwatch. `session/cancel` is a notification, so ACP
        // gives it no reply and the only acknowledgement is the prompt
        // response that ends the turn — which a deaf adapter never sends.
        // `supervisor_tick` reads this to notice, and the `PromptResult` arm
        // clears it, so a well-behaved adapter never trips the grace.
        //
        // `insert` rather than `entry().or_insert`: a second cancel is the
        // user asking again, and the grace should be measured from the most
        // recent ask rather than silently already half spent.
        self.cancel_requested
            .insert(thread_id.clone(), std::time::Instant::now());
        self.pump_acp();
        Ok(SessionCancelResult {
            thread_id,
            cancelled: true,
        })
    }

    pub fn pump_acp(&mut self) {
        let thread_ids: Vec<String> = self.connections.keys().cloned().collect();
        for thread_id in thread_ids {
            let mut events = Vec::new();
            if let Some(conn) = self.connections.get_mut(&thread_id) {
                while let Ok(event) = conn.try_recv() {
                    events.push(event);
                }
            }
            for event in events {
                self.handle_inbound(&thread_id, event);
            }
        }
        // Chief's host tools arrive on their own loopback threads and are
        // answered here, on the one thread that owns the session (#24).
        self.pump_chief_tools();
        // The stuck backstop needs a clock, and this is the only thing the host
        // runs on a timer. It is a comparison per live thread.
        self.lifecycle_tick();
        // And the supervisor's keep-alive rides the same timer: reap adapters
        // whose process is gone, notice a machine that was asleep, and close
        // sessions nobody is using (#21). Rate-limited inside.
        self.supervisor_tick();
        // The in-process cron rides it too (#25). Decision #4 keeps the host in
        // the Tauri binary — no launchd, no daemon — so this pump *is* the only
        // clock a schedule has. Rate-limited inside, and deliberately after the
        // supervisor: a fire dispatched onto a thread whose adapter died this
        // tick should find the reap already done.
        self.schedule_tick();
    }

    /// Quit, in the sense decision #4 settled: kill the children, keep the
    /// question. An ask the user never answered stays `pending` on disk so the
    /// next launch can put it back in front of them (#20, #21).
    pub fn shutdown_adapters(&mut self) {
        let ids: Vec<String> = self.connections.keys().cloned().collect();
        for id in &ids {
            self.withdraw_pending_permissions(id, "host shutdown", Withdrawal::Abandoned);
        }
        for (_, mut conn) in self.connections.drain() {
            conn.kill();
        }
        // Quit takes the listeners with the children (decision #4).
        self.chief_bridges.clear();
    }

    pub fn adapter_wake(&self) -> std::sync::Arc<AdapterWake> {
        std::sync::Arc::clone(&self.wake)
    }

    pub fn live_adapter_count(&self) -> usize {
        self.connections.len()
    }

    /// An adapter is no longer there: EOF on its stdout, or the supervisor's
    /// keep-alive probe reaping a pid whose stdout nobody ever closed.
    ///
    /// One path for both, because the *consequences* are identical and the two
    /// discoveries are not: an adapter that forks something holding its stdout
    /// never produces the EOF, so a host that only listened for EOF would keep
    /// a dead session marked live for as long as the app ran (#21).
    pub(crate) fn on_adapter_gone(&mut self, thread_id: &str, error: Option<&str>) {
        if let Some(err) = error {
            eprintln!("adapter for {thread_id} closed: {err}");
        }
        self.withdraw_pending_permissions(thread_id, "adapter closed", Withdrawal::Cancelled);
        self.connections.remove(thread_id);
        // The session that was being served host tools is gone; the listener
        // serving them should not outlive it (#24).
        self.drop_chief_bridge(thread_id);
        self.drop_prompt_queue(thread_id, "the adapter stopped");
        // The tool-call ids this turn declared as `execute` describe a process
        // that no longer exists; keeping them would let the next adapter's
        // reuse of the same id be read as PR evidence (#28).
        self.pr_forget_thread(thread_id);
        self.lifecycle_on_adapter_closed(thread_id, error);
    }

    /// Drop a thread's adapter, `session/close` first where the agent said it
    /// speaks it.
    ///
    /// Close is what frees the agent's own resources; killing the group frees
    /// ours. Buzz only ever did the second and pinned a Claude process tree per
    /// session for the life of the app ([buzz#2961](https://github.com/block/buzz/issues/2961)),
    /// which is why the order here is close-then-kill and not kill-only. It
    /// stays best-effort: an adapter that will not answer must not be able to
    /// hold up the user's Archive.
    pub(crate) fn drop_adapter(&mut self, thread_id: &str) {
        if let Some(mut conn) = self.connections.remove(thread_id) {
            if let Some(session_id) = conn.session_id.clone() {
                if let Err(err) = conn.close_session(&session_id) {
                    eprintln!("session/close for {thread_id} failed: {err}");
                }
            }
            conn.kill();
        }
        self.drop_chief_bridge(thread_id);
        self.pr_forget_thread(thread_id);
    }

    /// The adapter's pid, for `thread/state` and `supervisor/status`.
    /// Diagnostic only: nothing durable is keyed on a pid (decision #4).
    pub(crate) fn adapter_pid(&self, thread_id: &str) -> Option<u32> {
        self.connections.get(thread_id).map(|conn| conn.pid())
    }

    pub(crate) fn ensure_connection(&mut self, params: &PromptParams) -> Result<(), RpcError> {
        if self.connections.contains_key(&params.thread_id) {
            return Ok(());
        }
        let runtime = self.resolve_runtime(params)?;
        match runtime.probe() {
            ProbeResult::Missing { command, hint } => {
                return Err(RpcError::HarnessUnavailable {
                    command,
                    install_hint: hint,
                });
            }
            ProbeResult::Installed(_) => {}
        }
        let log_path = self.adapter_log_path(&params.thread_id);
        let cwd = self.resolve_cwd(params)?;
        let cwd_path = PathBuf::from(&cwd);
        let conn = AcpConnection::spawn(
            &runtime,
            Some(cwd_path.as_path()),
            &log_path,
            std::sync::Arc::clone(&self.wake),
        )?;
        self.connections.insert(params.thread_id.clone(), conn);
        Ok(())
    }

    fn resolve_runtime(&self, params: &PromptParams) -> Result<HarnessRuntime, RpcError> {
        if let Some(store) = &self.store {
            if let Ok(Some(thread)) = store.get_thread(&params.thread_id) {
                return HarnessRuntime::from_runtime_json(&thread.harness_id, &thread.runtime_json)
                    .map_err(RpcError::InvalidParams);
            }
        }
        if let Some(spec) = &params.runtime {
            let id = params
                .harness_id
                .clone()
                .unwrap_or_else(|| "custom".to_string());
            return HarnessRuntime::from_spec(id, spec).map_err(RpcError::InvalidParams);
        }
        if let (Some(store), Some(harness_id)) = (&self.store, params.harness_id.as_deref()) {
            if let Ok(Some(row)) = store.get_harness(harness_id) {
                return HarnessRuntime::from_harness(&row).map_err(RpcError::InvalidParams);
            }
        }
        Err(RpcError::InvalidParams(format!(
            "no runtime for thread {}",
            params.thread_id
        )))
    }

    pub(crate) fn resolve_cwd(&self, params: &PromptParams) -> Result<String, RpcError> {
        if let Some(store) = &self.store {
            if let Ok(Some(thread)) = store.get_thread(&params.thread_id) {
                return absolute_cwd(&thread.cwd);
            }
        }
        if let Some(cwd) = params.cwd.as_deref() {
            return absolute_cwd(cwd);
        }
        let fallback = std::env::current_dir().map_err(|e| RpcError::Internal(e.to_string()))?;
        Ok(fallback.to_string_lossy().into_owned())
    }

    fn adapter_log_path(&self, thread_id: &str) -> PathBuf {
        self.log_dir.join(format!("{thread_id}.stderr.log"))
    }

    pub(crate) fn handle_inbound(&mut self, thread_id: &str, event: Inbound) {
        match event {
            Inbound::Update(acp) => {
                let seq = self.persist_transcript_event(thread_id, "session/update", &acp);
                self.notify_session_update_at(thread_id, acp.clone(), seq);
                // After the stream, so a client sees the chunk that ended the
                // turn before it sees the Inbox card the turn produced.
                self.lifecycle_on_update(thread_id, &acp);
                // And after the lifecycle, for the same reason: a `PR #23
                // opened` card is about work the turn did, so the turn's own
                // events go out first (#28).
                self.pr_observe_update(thread_id, &acp);
            }
            Inbound::Permission { acp_id, params } => {
                self.open_permission_request(thread_id, acp_id, &params)
            }
            Inbound::PromptResult(result) => {
                let stop_reason = result
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let acp = json!({
                    "sessionUpdate": "state_update",
                    "sessionState": "idle",
                    "stopReason": stop_reason,
                    "result": result,
                });
                // Labelled by the ACP message it came from — this payload was
                // synthesized from the *prompt response* — while the payload
                // itself stays `session/update`-shaped so one reducer replays
                // every row (#14).
                let seq = self.persist_transcript_event(thread_id, "session/prompt", &acp);
                self.notify_session_update_at(thread_id, acp, seq);
                // The completion signal, and the authoritative one: ACP puts
                // the stop reason on the prompt *response*. Ending the turn
                // straight from here rather than through a synthesized
                // `state_update` means a response that carries no stop reason
                // still ends the run, while a v2 adapter merely reporting that
                // it went idle no longer ends one it knows nothing about. A v2
                // adapter that does report a stop reason gets there first; the
                // ledger transition is idempotent and this is then a no-op.
                self.lifecycle_on_turn_end(thread_id, stop_reason.as_deref());
                // A turn that talked about opening a pull request but never
                // printed a URL gets asked about now, from the thread's own
                // worktree — the authoritative check in `pr-linkage.md` (#28).
                self.pr_on_turn_end(thread_id);
                // Whatever ended the turn — a stop reason, a cancel the
                // adapter honoured, or the adapter simply finishing — there is
                // no longer anything to be waiting on, so the stopwatch stops.
                self.cancel_requested.remove(thread_id);
                // The turn is over, so the session is free: anything the user
                // said while it was busy goes out now, in the order they said
                // it (#14 steer-vs-redispatch).
                self.drain_prompt_queue(thread_id);
            }
            Inbound::Closed { error } => self.on_adapter_gone(thread_id, error.as_deref()),
        }
    }
}

fn absolute_cwd(cwd: &str) -> Result<String, RpcError> {
    let path = PathBuf::from(cwd);
    if path.is_absolute() {
        return Ok(path.to_string_lossy().into_owned());
    }
    let joined = std::env::current_dir()
        .map_err(|e| RpcError::Internal(e.to_string()))?
        .join(path);
    Ok(joined.to_string_lossy().into_owned())
}
