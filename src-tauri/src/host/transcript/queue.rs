//! Steer vs redispatch: what the user gets when they talk to a busy thread.
//!
//! The fork Buzz names is `queue | steer | interrupt | owner-interrupt`, with
//! "if the adapter lacks steer, cancel and redispatch merged context"
//! ([setup-porting/buzz.md §8](../../../../docs/research/setup-porting/buzz.md)).
//!
//! **Every ACP adapter lacks steer.** `session/prompt` is one turn per session
//! and its stop reason comes back on the *response*, matched to the thread
//! rather than to the prompt it answers — which is precisely why #15 refuses a
//! second concurrent run: the second turn would collect the first turn's
//! outcome and the first run would be retired holding nothing. Injecting a
//! mid-turn message is not something the protocol can express.
//!
//! So JaBot ships the two answers that *are* expressible, and the client says
//! which one it means:
//!
//! | `mode` | Behaviour |
//! |---|---|
//! | `reject` (default) | #15's `RUN_IN_FLIGHT` error, unchanged |
//! | `queue` | Held here; sent the moment the turn in flight ends |
//! | `interrupt` | `session/cancel`, then sent when the cancelled turn reports |
//!
//! `reject` stays the default deliberately. A client that has never heard of
//! this module cannot grow a queue by accident, and the error remains the
//! backstop for a UI whose idea of "busy" is stale.
//!
//! The queue is RAM, like the rest of the supervisor's picture of what is
//! running (#5: "still working is supervisor RAM, reconciled on boot"). A
//! queued prompt has not been said to the agent, so it is not in the
//! transcript either — it is an unsent draft the host is holding, and a host
//! that dies holding one has lost a draft, not a turn.

use std::collections::VecDeque;

use serde_json::Value;

use crate::host::protocol::error::RpcError;
use crate::host::protocol::methods::{
    PromptMode, PromptParams, PromptResult, QueuedPromptView, SessionCancelParams,
};
use crate::host::store::now_utc;
use crate::host::HostSession;

/// A prompt the user has sent that the agent has not been given yet.
#[derive(Debug, Clone)]
pub(crate) struct QueuedPrompt {
    pub content: Value,
    pub queued_at: String,
}

impl HostSession {
    /// The whole of the in-flight decision, taken before anything is spawned.
    ///
    /// `Ok(None)` means the thread is free and the caller should send now.
    /// `Ok(Some(result))` means this prompt is queued and the caller is done.
    pub(crate) fn intercept_in_flight(
        &mut self,
        params: &PromptParams,
    ) -> Result<Option<PromptResult>, RpcError> {
        let thread_id = params.thread_id.clone();
        // A queue that is already non-empty means the turn in flight has not
        // ended yet, even in the instant between the prompt response and the
        // drain. Order has to hold: the second follow-up cannot overtake the
        // first just because it arrived while the ledger was mid-transition.
        let already_queued = self.queue_depth(&thread_id) > 0;
        let in_flight = self.refuse_overlapping_run(&thread_id).err();
        if in_flight.is_none() && !already_queued {
            return Ok(None);
        }

        match params.mode.unwrap_or_default() {
            // The same error either way, and `runState` says which: a run the
            // agent is still working on, or a prompt already waiting for it.
            PromptMode::Reject => Err(in_flight.unwrap_or(RpcError::RunInFlight {
                thread_id: thread_id.clone(),
                run_id: String::new(),
                state: "queued".into(),
            })),
            PromptMode::Queue => Ok(Some(self.enqueue_prompt(&thread_id, &params.content))),
            PromptMode::Interrupt => {
                // Cancel first, queue second, and only if the cancel was
                // actually delivered. A prompt queued behind a turn that was
                // never told to stop waits for a stop reason that is not
                // coming; refusing hands the text back to the client, which is
                // still holding it.
                self.session_cancel(SessionCancelParams {
                    thread_id: thread_id.clone(),
                })?;
                Ok(Some(self.enqueue_prompt(&thread_id, &params.content)))
            }
        }
    }

    /// The turn ended — send the next held prompt, if there is one.
    ///
    /// Deliberately *not* a call back into `session_prompt`: this runs inside
    /// the adapter pump, and re-entering the pump from an event handler would
    /// nest one drain inside another. Everything the response produces is
    /// picked up on the pump's next pass, which is at most one tick away.
    pub(crate) fn drain_prompt_queue(&mut self, thread_id: &str) {
        let Some(next) = self.pop_queued(thread_id) else {
            return;
        };
        // The adapter has to still be there. If it is not, the prompt stays
        // undelivered and the client is told by the drop path, not by silence.
        let Some(session_id) = self
            .conn(thread_id)
            .and_then(|conn| conn.session_for(thread_id))
        else {
            self.requeue_front(thread_id, next);
            self.drop_prompt_queue(thread_id, "the adapter is no longer running");
            return;
        };
        let Some(conn) = self.conn_mut(thread_id) else {
            return;
        };
        if let Err(err) = conn.send_prompt(thread_id, &session_id, &next.content) {
            eprintln!("failed to send a queued prompt for {thread_id}: {err}");
            self.drop_adapter(thread_id);
            self.requeue_front(thread_id, next);
            self.drop_prompt_queue(thread_id, "the adapter stopped accepting prompts");
            return;
        }
        // Marked as a dispatch off the queue: this is the only signal a client
        // gets that its "waiting" strip just got one shorter.
        self.record_queued_prompt_dispatched(thread_id, &next.content);
        self.lifecycle_run_started(thread_id, &session_id);
    }

    /// Give up on everything still held for this thread, and say so.
    ///
    /// A queued prompt is the user's words. When the adapter goes, they are
    /// never going to be delivered, and leaving them in a queue nothing will
    /// drain would show a chat waiting forever on a message no agent will ever
    /// read. The `sys` line is the only honest end for them.
    pub(crate) fn drop_prompt_queue(&mut self, thread_id: &str, reason: &str) {
        let dropped: Vec<QueuedPrompt> = self
            .prompt_queue
            .remove(thread_id)
            .map(|queue| queue.into_iter().collect())
            .unwrap_or_default();
        for prompt in dropped {
            let acp = serde_json::json!({
                "sessionUpdate": "state_update",
                "jabot": {
                    "event": "prompt_dropped",
                    "reason": reason,
                    "content": prompt.content,
                },
            });
            let seq = self.persist_transcript_event(thread_id, "session/update", &acp);
            self.notify_session_update_at(thread_id, acp, seq);
        }
    }

    /// What `thread/transcript` reports as still held.
    pub(crate) fn queued_prompts(&self, thread_id: &str) -> Vec<QueuedPromptView> {
        self.prompt_queue
            .get(thread_id)
            .into_iter()
            .flat_map(|queue| queue.iter())
            .enumerate()
            .map(|(index, prompt)| QueuedPromptView {
                position: index + 1,
                content: prompt.content.clone(),
                queued_at: prompt.queued_at.clone(),
            })
            .collect()
    }

    /// Every thread holding at least one undelivered prompt.
    ///
    /// The supervisor needs this to find queues on threads whose adapter is
    /// gone; iterating live connections cannot see them, because the whole
    /// problem is that there is no connection left.
    pub(crate) fn queued_thread_ids(&self) -> Vec<String> {
        self.prompt_queue
            .iter()
            .filter(|(_, queue)| !queue.is_empty())
            .map(|(thread_id, _)| thread_id.clone())
            .collect()
    }

    pub(crate) fn queue_depth(&self, thread_id: &str) -> usize {
        self.prompt_queue
            .get(thread_id)
            .map(VecDeque::len)
            .unwrap_or(0)
    }

    fn enqueue_prompt(&mut self, thread_id: &str, content: &Value) -> PromptResult {
        let queue = self.prompt_queue.entry(thread_id.to_string()).or_default();
        queue.push_back(QueuedPrompt {
            content: content.clone(),
            queued_at: now_utc(),
        });
        let position = queue.len();
        PromptResult {
            thread_id: thread_id.to_string(),
            acp_session_id: self.acp_session_id_for(thread_id),
            // Not a lie of omission: the agent has not accepted anything.
            accepted: false,
            queued: true,
            queue_position: Some(position),
        }
    }

    fn pop_queued(&mut self, thread_id: &str) -> Option<QueuedPrompt> {
        self.prompt_queue.get_mut(thread_id)?.pop_front()
    }

    fn requeue_front(&mut self, thread_id: &str, prompt: QueuedPrompt) {
        self.prompt_queue
            .entry(thread_id.to_string())
            .or_default()
            .push_front(prompt);
    }

    /// The session a queued prompt will be sent on, as far as anyone can know
    /// before it is sent. Empty when the thread has never had one.
    fn acp_session_id_for(&self, thread_id: &str) -> String {
        if let Some(id) = self
            .conn(thread_id)
            .and_then(|conn| conn.session_for(thread_id))
        {
            return id;
        }
        self.store
            .as_ref()
            .and_then(|store| store.get_thread(thread_id).ok().flatten())
            .and_then(|thread| thread.acp_session_id)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::error::RUN_IN_FLIGHT;
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use crate::host::protocol::methods::ThreadOpenParams;
    use crate::host::protocol::HOST_HELLO;
    use serde_json::json;

    fn host() -> (tempfile::TempDir, HostSession) {
        let dir = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(dir.path());
        session
            .handle_request(JsonRpcRequest::new(RequestId::Number(1), HOST_HELLO, None))
            .result
            .expect("hello");
        session
            .thread_open(ThreadOpenParams {
                thread_id: Some("t-queue".into()),
                title: "Auth migration".into(),
                cwd: std::env::temp_dir().to_string_lossy().into_owned(),
                harness_id: "claude".into(),
                runtime: None,
                folder_id: None,
                bot_id: None,
                fold_policy: None,
                use_checkout: None,
                base_ref: None,
            })
            .expect("thread/open");
        (dir, session)
    }

    fn prompt(mode: Option<PromptMode>) -> PromptParams {
        PromptParams {
            thread_id: "t-queue".into(),
            content: json!("and also fix the tests"),
            mode,
            cwd: None,
            harness_id: None,
            runtime: None,
        }
    }

    /// With nothing running there is nothing to decide: every mode sends now.
    #[test]
    fn an_idle_thread_is_never_intercepted() {
        let (_dir, mut session) = host();
        for mode in [
            None,
            Some(PromptMode::Queue),
            Some(PromptMode::Interrupt),
            Some(PromptMode::Reject),
        ] {
            assert!(session
                .intercept_in_flight(&prompt(mode))
                .unwrap()
                .is_none());
        }
    }

    /// The view a client draws its "waiting to send" chips from, in order.
    #[test]
    fn queued_prompts_keep_their_order_and_are_reported() {
        let (_dir, mut session) = host();
        session.enqueue_prompt("t-queue", &json!("first"));
        session.enqueue_prompt("t-queue", &json!("second"));

        let held = session.queued_prompts("t-queue");
        assert_eq!(held.len(), 2);
        assert_eq!(held[0].position, 1);
        assert_eq!(held[0].content, json!("first"));
        assert_eq!(held[1].position, 2);
        assert_eq!(held[1].content, json!("second"));
    }

    /// The regression this guards: a follow-up sent in the gap between the
    /// prompt response and the drain must not overtake one already waiting.
    /// The run ledger is momentarily clear there, so "is a run open?" is not
    /// on its own enough to decide whether this thread is free.
    #[test]
    fn a_prompt_never_overtakes_the_queue_it_arrives_behind() {
        let (_dir, mut session) = host();
        session.enqueue_prompt("t-queue", &json!("first"));

        let second = session
            .intercept_in_flight(&PromptParams {
                content: json!("second"),
                ..prompt(Some(PromptMode::Queue))
            })
            .unwrap()
            .expect("queued behind the first");
        assert_eq!(second.queue_position, Some(2));
        assert!(second.queued);
        assert!(!second.accepted);
        assert_eq!(session.queue_depth("t-queue"), 2);
    }

    /// `reject` is the default, and it is still #15's error — not a queue that
    /// grew quietly under a client that never asked for one.
    #[test]
    fn the_default_mode_still_refuses() {
        let (_dir, mut session) = host();
        session.enqueue_prompt("t-queue", &json!("first"));

        let err = session
            .intercept_in_flight(&prompt(None))
            .expect_err("refused");
        assert_eq!(err.code(), RUN_IN_FLIGHT);
        assert_eq!(err.data().unwrap()["runState"], "queued");
        assert_eq!(session.queue_depth("t-queue"), 1, "and nothing was queued");
    }

    /// Losing the adapter must empty the queue rather than leave the user
    /// watching a message that will never be delivered.
    #[test]
    fn dropping_the_queue_says_so_in_the_transcript() {
        let (_dir, mut session) = host();
        session.enqueue_prompt("t-queue", &json!("and also fix the tests"));
        session.drop_prompt_queue("t-queue", "the adapter is no longer running");

        assert_eq!(session.queue_depth("t-queue"), 0);
        let replay = session
            .thread_transcript(crate::host::protocol::methods::ThreadTranscriptParams {
                thread_id: "t-queue".into(),
                after_seq: None,
                limit: None,
            })
            .unwrap();
        assert!(replay.queued.is_empty());
        let payload = &replay.events.last().expect("a dropped event").payload;
        assert_eq!(payload["jabot"]["event"], "prompt_dropped");
        assert_eq!(payload["jabot"]["content"], json!("and also fix the tests"));
    }
}
