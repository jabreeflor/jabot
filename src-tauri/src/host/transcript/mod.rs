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
        })
    }

    /// Persist a prompt as `user_message_chunk` and stream it to every client.
    ///
    /// Called at *dispatch*, not at accept: a prompt still sitting in the
    /// queue has not been said to the agent yet, and a transcript that claims
    /// otherwise is one the user cannot trust about what the agent was told.
    pub(crate) fn record_user_prompt(&mut self, thread_id: &str, content: &Value) {
        let acp = json!({
            "sessionUpdate": USER_MESSAGE_CHUNK,
            "content": { "type": "text", "text": prompt_text(content) },
        });
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
