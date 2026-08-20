//! The persisted transcript overlay (#14).
//!
//! Three jobs, and the reason they are one module is that they are one
//! invariant: **the durable log and the live stream must be the same events.**
//!
//! 1. *Write.* Every ACP notification the host consumes is appended to
//!    `transcript_events` — the ACP payload verbatim, never a JaBot bubble
//!    schema ([`store.md` transcript ownership](../../../../docs/research/data-and-persistence/store.md)).
//!    The user's own prompt is appended too, as the ACP `user_message_chunk`
//!    an agent would have sent: without it a reopened thread replays the
//!    agent's half of a conversation and none of the human's.
//! 2. *Read.* [`HostSession::thread_transcript`] serves that log back, so
//!    reopening a thread hydrates from our SQLite and never from a harness's
//!    JSONL — which is undocumented, drifts, and gets deleted after 30 days.
//! 3. *Reconcile.* A client that is streaming *and* hydrating has to know
//!    which live events it has already replayed. Every `session/update`
//!    notification carries the `transcriptSeq` its row got, and
//!    `thread/transcript` reports `headSeq`; above the head is new, at or
//!    below it is already in hand. Without that pairing the two counters —
//!    notification `seq` and transcript `seq` — cannot be compared, because
//!    they count different sets of events.
//!
//! Steer-vs-redispatch lives in [`queue`].

pub(crate) mod queue;

use serde_json::{json, Value};

use super::protocol::error::RpcError;
use super::protocol::methods::{
    ThreadTranscriptParams, ThreadTranscriptResult, TranscriptEventView,
};
use super::HostSession;

/// How many rows a `thread/transcript` reply carries when the caller does not
/// say. A long code session is thousands of chunks; the chat only ever shows
/// the tail, and asking for the rest is one more call with `afterSeq`.
const DEFAULT_LIMIT: usize = 1_000;

/// What the host writes a dispatched prompt into the transcript as.
///
/// ACP's own name for "the user said this", so a replay of our table and a
/// replay from `session/load` reduce through the same mapper.
pub(crate) const USER_MESSAGE_CHUNK: &str = "user_message_chunk";

/// The `jabot.event` marker on a prompt that came off the queue.
///
/// A client draws a "waiting" strip from [`ThreadTranscriptResult::queued`]
/// and keeps its own copy of it, because the queue is RAM and no notification
/// reports it. When the host drains one, the only thing the client sees is an
/// ordinary `user_message_chunk` — indistinguishable from the echo of a prompt
/// it sent itself — so its strip would stay up over a message that has in fact
/// been delivered, and the "Send now" button on that strip would go on
/// cancelling turns the stale entry has nothing to do with. This says which
/// bubbles are a queue leaving, so the mirror can shrink with it.
pub(crate) const PROMPT_DISPATCHED: &str = "prompt_dispatched";

impl HostSession {
    /// Replay a thread's transcript from the store.
    ///
    /// Ordered oldest → newest even when `limit` took a window off the end,
    /// because the renderer reduces a stream and a stream only runs one way.
    pub fn thread_transcript(
        &self,
        params: ThreadTranscriptParams,
    ) -> Result<ThreadTranscriptResult, RpcError> {
        let store = self.store.as_ref().ok_or(RpcError::StoreUnavailable)?;
        let after = params.after_seq.unwrap_or(0);
        let rows = store
            .transcript_after(&params.thread_id, after)
            .map_err(|err| RpcError::Internal(err.to_string()))?;
        // Asked of the log rather than taken from the last row: a caller that
        // is already up to date gets back no rows, and a head derived from
        // them would walk backwards on every poll.
        let head_seq = store
            .transcript_head(&params.thread_id)
            .map_err(|err| RpcError::Internal(err.to_string()))?;
        let limit = params.limit.unwrap_or(DEFAULT_LIMIT);
        let truncated = rows.len() > limit;
        let window = if truncated { rows.len() - limit } else { 0 };

        let events = rows
            .into_iter()
            .skip(window)
            .map(|row| TranscriptEventView {
                seq: row.seq,
                method: row.acp_method,
                // A row that will not parse is a row we wrote badly. It is
                // still an event that happened, so it comes back as a string
                // rather than taking the whole replay down with it.
                payload: serde_json::from_str(&row.payload_json)
                    .unwrap_or(Value::String(row.payload_json)),
                created_at: row.created_at,
            })
            .collect();

        Ok(ThreadTranscriptResult {
            thread_id: params.thread_id.clone(),
            head_seq,
            events,
            truncated,
            queued: self.queued_prompts(&params.thread_id),
            run_state: self
                .open_run(&params.thread_id)
                .map(|(_, state)| state.as_str().to_string()),
        })
    }

    /// Persist a prompt as `user_message_chunk` and stream it to every client.
    ///
    /// Called at *dispatch*, not at accept: a prompt still sitting in the
    /// queue has not been said to the agent yet, and a transcript that claims
    /// otherwise is one the user cannot trust about what the agent was told.
    pub(crate) fn record_user_prompt(&mut self, thread_id: &str, content: &Value) {
        self.record_prompt_event(thread_id, content, false);
    }

    /// The same, for a prompt the queue just handed to the agent.
    ///
    /// Carries [`PROMPT_DISPATCHED`] so a client mirroring the queue knows
    /// this bubble *is* the head of it, rather than a message on top of it.
    pub(crate) fn record_queued_prompt_dispatched(&mut self, thread_id: &str, content: &Value) {
        self.record_prompt_event(thread_id, content, true);
    }

    fn record_prompt_event(&mut self, thread_id: &str, content: &Value, from_queue: bool) {
        let mut acp = json!({
            "sessionUpdate": USER_MESSAGE_CHUNK,
            "content": { "type": "text", "text": prompt_text(content) },
        });
        // Beside the ACP shape, never inside it: the payload has to stay
        // something the same reducer maps whether it came from us or from an
        // adapter, and a client that has never heard of `jabot` still draws
        // the bubble.
        if from_queue {
            acp["jabot"] = json!({ "event": PROMPT_DISPATCHED });
        }
        let seq = self.persist_transcript_event(thread_id, "session/update", &acp);
        self.notify_session_update_at(thread_id, acp, seq);
    }

    /// Append one consumed ACP payload to `transcript_events`.
    ///
    /// Insert-only, and never an UPDATE of an earlier row: the table is a log
    /// and the renderer reduces it (store.md). A replacement `tool_call_update`
    /// is a new row carrying the same `toolCallId`, which is exactly what the
    /// adapter sent us.
    ///
    /// Returns the row's `seq`, or `None` when there is no store — an
    /// ephemeral host streams without persisting, and the notification says so
    /// by carrying no `transcriptSeq` rather than by carrying a made-up one.
    pub(crate) fn persist_transcript_event(
        &self,
        thread_id: &str,
        method: &str,
        payload: &Value,
    ) -> Option<i64> {
        let store = self.store.as_ref()?;
        let json = serde_json::to_string(payload).ok()?;
        match store.append_transcript(thread_id, method, &json) {
            Ok(row) => Some(row.seq),
            Err(err) => {
                // A transcript we could not write is not a turn we should
                // abandon: the agent is still working and the client is still
                // streaming. Losing the replay is the smaller failure.
                eprintln!("failed to persist transcript for {thread_id}: {err}");
                None
            }
        }
    }
}

/// The prompt as one line of text.
///
/// `session/prompt` takes anything ACP takes — a bare string, one content
/// block, or an array of them. The transcript echo is a chat bubble, so the
/// text blocks are joined and everything else is named rather than dropped:
/// "the user attached an image" is information, and an empty bubble is not.
pub(crate) fn prompt_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .map(block_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        other => block_text(other),
    }
}

fn block_text(block: &Value) -> String {
    if let Some(text) = block.as_str() {
        return text.to_string();
    }
    if let Some(text) = block.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    match block.get("type").and_then(Value::as_str) {
        Some(kind) => format!("[{kind}]"),
        None => block.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use crate::host::protocol::{HOST_HELLO, THREAD_TRANSCRIPT};

    fn persistent_host() -> (tempfile::TempDir, HostSession) {
        let dir = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(dir.path());
        session
            .handle_request(JsonRpcRequest::new(RequestId::Number(1), HOST_HELLO, None))
            .result
            .expect("hello");
        (dir, session)
    }

    fn open_thread(session: &mut HostSession, thread_id: &str) {
        session
            .thread_open(crate::host::protocol::methods::ThreadOpenParams {
                thread_id: Some(thread_id.into()),
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
    }

    #[test]
    fn replays_in_order_and_reports_the_head() {
        let (_dir, mut session) = persistent_host();
        open_thread(&mut session, "t-replay");
        let store = session.store().expect("store");
        for text in ["one", "two", "three"] {
            store
                .append_transcript(
                    "t-replay",
                    "session/update",
                    &json!({ "sessionUpdate": "agent_message_chunk", "text": text }).to_string(),
                )
                .unwrap();
        }

        let result = session
            .thread_transcript(ThreadTranscriptParams {
                thread_id: "t-replay".into(),
                after_seq: None,
                limit: None,
            })
            .unwrap();

        assert_eq!(result.head_seq, 3);
        assert!(!result.truncated);
        let seqs: Vec<i64> = result.events.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![1, 2, 3]);
        assert_eq!(result.events[0].payload["text"], "one");
    }

    /// `limit` takes the *newest* rows and still hands them back oldest-first:
    /// a reducer replays a stream forwards, and a window that arrived
    /// backwards would rebuild the conversation inside out.
    #[test]
    fn limit_windows_the_tail_and_says_it_truncated() {
        let (_dir, mut session) = persistent_host();
        open_thread(&mut session, "t-window");
        let store = session.store().expect("store");
        for n in 0..5 {
            store
                .append_transcript("t-window", "session/update", &json!({ "n": n }).to_string())
                .unwrap();
        }

        let result = session
            .thread_transcript(ThreadTranscriptParams {
                thread_id: "t-window".into(),
                after_seq: None,
                limit: Some(2),
            })
            .unwrap();

        assert!(result.truncated);
        assert_eq!(
            result.head_seq, 5,
            "head is the log's head, not the window's"
        );
        let seqs: Vec<i64> = result.events.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![4, 5]);
    }

    /// The contract that lets a client stream and hydrate at the same time: an
    /// event's notification carries the same `seq` `thread/transcript` will
    /// report for it, so "have I already got this one" has an answer.
    #[test]
    fn a_streamed_event_carries_the_seq_its_row_got() {
        let (_dir, mut session) = persistent_host();
        open_thread(&mut session, "t-pair");
        session.record_user_prompt("t-pair", &json!("ship it"));

        let outbound = session.take_outbound();
        let params = outbound
            .iter()
            .find(|n| n.method == crate::host::protocol::SESSION_UPDATE)
            .and_then(|n| n.params.clone())
            .expect("session/update");
        assert_eq!(params["acp"]["sessionUpdate"], USER_MESSAGE_CHUNK);
        assert_eq!(params["acp"]["content"]["text"], "ship it");
        let streamed = params["transcriptSeq"].as_i64().expect("transcriptSeq");

        let replay = session
            .thread_transcript(ThreadTranscriptParams {
                thread_id: "t-pair".into(),
                after_seq: None,
                limit: None,
            })
            .unwrap();
        assert_eq!(replay.head_seq, streamed);
        assert_eq!(replay.events.len(), 1);
        assert_eq!(replay.events[0].seq, streamed);
        assert_eq!(replay.events[0].payload["content"]["text"], "ship it");
    }

    /// A view that mounts mid-turn cannot find the turn in the replay: the
    /// last row of a live turn and the last row of one whose host died under
    /// it are the same row. So the ledger's answer rides along with the rows
    /// it has to agree with, and it stops the moment the run does.
    #[test]
    fn the_replay_reports_a_run_that_is_still_open() {
        let (_dir, mut session) = persistent_host();
        open_thread(&mut session, "t-open");
        let params = || ThreadTranscriptParams {
            thread_id: "t-open".into(),
            after_seq: None,
            limit: None,
        };
        assert_eq!(
            session.thread_transcript(params()).unwrap().run_state,
            None,
            "a thread that has never run is not busy"
        );

        session.record_user_prompt("t-open", &json!("go"));
        session.lifecycle_run_started("t-open", "sess-1");
        assert_eq!(
            session
                .thread_transcript(params())
                .unwrap()
                .run_state
                .as_deref(),
            Some("running"),
        );

        session.lifecycle_on_turn_end("t-open", Some("end_turn"));
        assert_eq!(
            session.thread_transcript(params()).unwrap().run_state,
            None,
            "a finished run is not an open one, whatever it finished as"
        );
    }

    /// The marker a client's queue mirror shrinks on. A dispatched prompt and
    /// a fresh one are the same bubble otherwise, which leaves the "N messages
    /// waiting" strip up over a message the agent has already been given.
    #[test]
    fn only_a_prompt_off_the_queue_says_it_was_dispatched() {
        let (_dir, mut session) = persistent_host();
        open_thread(&mut session, "t-mark");
        session.record_user_prompt("t-mark", &json!("first"));
        session.record_queued_prompt_dispatched("t-mark", &json!("second"));

        let replay = session
            .thread_transcript(ThreadTranscriptParams {
                thread_id: "t-mark".into(),
                after_seq: None,
                limit: None,
            })
            .unwrap();
        let events = &replay.events;
        assert_eq!(events[0].payload["content"]["text"], "first");
        assert!(events[0].payload.get("jabot").is_none());
        assert_eq!(events[1].payload["content"]["text"], "second");
        assert_eq!(events[1].payload["jabot"]["event"], PROMPT_DISPATCHED);
        // Still an ACP `user_message_chunk`: a client that has never heard of
        // the marker draws exactly the bubble it drew before.
        assert_eq!(events[1].payload["sessionUpdate"], USER_MESSAGE_CHUNK);
    }

    #[test]
    fn after_seq_asks_only_for_the_rest() {
        let (_dir, mut session) = persistent_host();
        open_thread(&mut session, "t-after");
        session.record_user_prompt("t-after", &json!("first"));
        session.record_user_prompt("t-after", &json!("second"));

        let result = session
            .thread_transcript(ThreadTranscriptParams {
                thread_id: "t-after".into(),
                after_seq: Some(1),
                limit: None,
            })
            .unwrap();
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].payload["content"]["text"], "second");

        // Nothing new: the head must still be the log's head, or a caller that
        // polls would walk backwards every time it is up to date.
        let caught_up = session
            .thread_transcript(ThreadTranscriptParams {
                thread_id: "t-after".into(),
                after_seq: Some(2),
                limit: None,
            })
            .unwrap();
        assert!(caught_up.events.is_empty());
        assert_eq!(caught_up.head_seq, 2);
    }

    #[test]
    fn prompt_text_reads_every_shape_acp_accepts() {
        assert_eq!(prompt_text(&json!("plain")), "plain");
        assert_eq!(
            prompt_text(&json!({ "type": "text", "text": "block" })),
            "block"
        );
        assert_eq!(
            prompt_text(&json!([
                { "type": "text", "text": "look at" },
                { "type": "image", "data": "…" },
                { "type": "text", "text": "this" },
            ])),
            "look at\n[image]\nthis"
        );
    }

    #[test]
    fn transcript_reaches_the_router() {
        let (_dir, mut session) = persistent_host();
        open_thread(&mut session, "t-router");
        session.record_user_prompt("t-router", &json!("hi"));

        let response = session.handle_request(JsonRpcRequest::new(
            RequestId::Number(2),
            THREAD_TRANSCRIPT,
            Some(json!({ "threadId": "t-router" })),
        ));
        let value = response.result.expect("transcript");
        assert_eq!(value["headSeq"], 1);
        assert_eq!(value["events"][0]["method"], "session/update");
    }
}
