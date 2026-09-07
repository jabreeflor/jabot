//! The one line a chat row shows under a bot's name.
//!
//! A sidebar that lists conversations has to say what each one is *about*, and
//! the only honest answer is the last thing said in it. `threads.preview` has
//! been a column since `0001_init.sql` and `FolderThreadView.preview` has been
//! on the wire since #16 — nothing ever wrote it, so every reader got `NULL`.
//! This is the writer.
//!
//! **Maintained on the way past, not queried after the fact.** The transcript
//! is a log of ACP chunks: one message arrives as dozens of rows, and the last
//! row of a message is usually a fragment (" now.", or a bare full stop).
//! Reconstructing a message would mean reading a window of the log back and
//! reducing it on every `crew/list`. Instead the tail of the message being
//! written is kept in RAM, exactly as the renderer's reducer keeps an open
//! bubble, and the column is rewritten as it grows.
//!
//! **RAM, and that is the right lifetime.** The tail describes a message
//! currently streaming. A host that restarts has no turn in flight to be
//! extending, so the next chunk starts a fresh preview — and the *column*
//! still holds whatever was last said, which is what a restarted sidebar
//! needs.

use std::collections::HashMap;

use serde_json::Value;

use super::block_text;
use crate::host::HostSession;

/// How much of a message the column keeps.
///
/// Two lines at the sidebar's width, and a bound on what `crew/list` carries:
/// a preview is a hint, and an agent that writes an essay must not put the
/// essay in every crew listing.
const PREVIEW_MAX: usize = 160;

/// Who said the message a preview is quoting. Not on the wire — it is what
/// decides whether the next chunk *extends* this preview or replaces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Speaker {
    User,
    Agent,
}

/// The message being written right now, per thread.
#[derive(Debug, Clone)]
pub(crate) struct PreviewTail {
    speaker: Speaker,
    /// The visible line, already collapsed and already capped.
    line: String,
}

pub(crate) type PreviewTails = HashMap<String, PreviewTail>;

impl HostSession {
    /// Fold one consumed `session/update` into the thread's preview.
    ///
    /// Called from the two places a message reaches the log — the adapter's
    /// chunks and the host's own echo of a dispatched prompt — so the column
    /// says the same thing the transcript does.
    pub(crate) fn observe_preview(&mut self, thread_id: &str, acp: &Value) {
        match classify(acp) {
            Chunk::Message(speaker, text) => {
                let line = self.extend_tail(thread_id, speaker, &text);
                // Only when the visible line actually moved. A capped preview
                // still receives chunks for the rest of a long message, and
                // an UPDATE per chunk would write the same string hundreds of
                // times to say nothing new.
                if let Some(line) = line {
                    if let Some(store) = self.store.as_ref() {
                        if let Err(err) = store.set_thread_preview(thread_id, &line) {
                            // The same trade `persist_transcript_event` makes:
                            // a preview is not worth a turn.
                            eprintln!("failed to write preview for {thread_id}: {err}");
                        }
                    }
                }
            }
            // The message is over: a tool line, a plan, the end of the turn.
            // The column keeps saying what was last said; the *next* chunk
            // starts a new preview rather than continuing one the user has
            // already watched finish.
            Chunk::Break => {
                self.preview_tails.remove(thread_id);
            }
            // A thought stream, an `available_commands_update`, anything ACP
            // adds next. Nothing to show and nothing to close — a message
            // interrupted by one of these is still the same message.
            Chunk::Other => {}
        }
    }

    /// Append to the open message, or start a new one. Returns the line to
    /// store, or `None` when nothing visible changed.
    fn extend_tail(&mut self, thread_id: &str, speaker: Speaker, text: &str) -> Option<String> {
        let tail = self
            .preview_tails
            .entry(thread_id.to_string())
            .and_modify(|tail| {
                if tail.speaker != speaker {
                    tail.speaker = speaker;
                    tail.line.clear();
                }
            })
            .or_insert_with(|| PreviewTail {
                speaker,
                line: String::new(),
            });

        let grown = append(&tail.line, text);
        if grown == tail.line {
            return None;
        }
        tail.line = grown.clone();
        Some(grown)
    }
}

/// What one `session/update` means to a preview.
enum Chunk {
    /// Something was said, and this is the fragment of it.
    Message(Speaker, String),
    /// The message, if any, has ended.
    Break,
    /// Neither — leave the preview exactly as it is.
    Other,
}

fn classify(acp: &Value) -> Chunk {
    let text = || block_text(acp.get("content").unwrap_or(&Value::Null));
    match acp.get("sessionUpdate").and_then(Value::as_str) {
        Some("user_message_chunk") => Chunk::Message(Speaker::User, text()),
        Some("agent_message_chunk") => Chunk::Message(Speaker::Agent, text()),
        // The three the renderer draws as something other than a bubble, so
        // they are the three that close one.
        Some("tool_call") | Some("tool_call_update") | Some("plan") | Some("state_update") => {
            Chunk::Break
        }
        _ => Chunk::Other,
    }
}

/// One line of chat, collapsed and capped.
///
/// Newlines and runs of spaces become single spaces: the row is one line
/// however the agent laid its markdown out, and a preview holding a fenced
/// code block would be a preview of nothing. Capped on a character boundary,
/// because `String` is UTF-8 and an agent writes in every language.
fn append(line: &str, text: &str) -> String {
    let mut out = String::with_capacity(line.len() + text.len());
    out.push_str(line);
    let mut chars = out.chars().count();
    let mut space = out.is_empty();
    for ch in text.chars() {
        if chars >= PREVIEW_MAX {
            break;
        }
        if ch.is_whitespace() {
            if !space {
                out.push(' ');
                chars += 1;
                space = true;
            }
            continue;
        }
        out.push(ch);
        chars += 1;
        space = false;
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chunk(kind: &str, text: &str) -> Value {
        json!({ "sessionUpdate": kind, "content": { "type": "text", "text": text } })
    }

    fn line(session: &HostSession, thread_id: &str) -> Option<String> {
        session
            .store()
            .expect("store")
            .get_thread(thread_id)
            .expect("thread")
            .expect("row")
            .preview
    }

    fn host_with_thread(thread_id: &str) -> (tempfile::TempDir, HostSession) {
        let dir = tempfile::tempdir().unwrap();
        let session = HostSession::load(dir.path());
        session
            .store()
            .expect("store")
            .insert_thread(&crate::host::store::NewThread {
                id: thread_id.into(),
                folder_id: None,
                bot_id: Some("writer".into()),
                harness_id: "claude".into(),
                cwd: "/tmp".into(),
                runtime_json: r#"{"command":"claude-agent-acp"}"#.into(),
                title: "Writer".into(),
                fold_policy: "default".into(),
                worktree_path: None,
                repo: Default::default(),
            })
            .expect("thread");
        (dir, session)
    }

    /// The property the whole module exists for: a message arrives in pieces
    /// and the preview is the message, not its last fragment.
    #[test]
    fn consecutive_chunks_grow_one_line() {
        let (_dir, mut session) = host_with_thread("bot-writer");
        session.observe_preview("bot-writer", &chunk("agent_message_chunk", "Digest is"));
        session.observe_preview("bot-writer", &chunk("agent_message_chunk", " parked"));
        session.observe_preview("bot-writer", &chunk("agent_message_chunk", " for you."));
        assert_eq!(
            line(&session, "bot-writer").as_deref(),
            Some("Digest is parked for you.")
        );
    }

    #[test]
    fn the_other_speaker_starts_a_new_line() {
        let (_dir, mut session) = host_with_thread("bot-writer");
        session.observe_preview(
            "bot-writer",
            &chunk("agent_message_chunk", "Ready when you are."),
        );
        session.observe_preview("bot-writer", &chunk("user_message_chunk", "Send it."));
        assert_eq!(line(&session, "bot-writer").as_deref(), Some("Send it."));
    }

    /// A tool call ends the message. The column keeps the last thing said —
    /// there is nothing better to show while a tool runs — and the agent's
    /// next sentence replaces it rather than being glued to the end of it.
    #[test]
    fn a_tool_call_closes_the_message() {
        let (_dir, mut session) = host_with_thread("bot-writer");
        session.observe_preview(
            "bot-writer",
            &chunk("agent_message_chunk", "Reading the repo."),
        );
        session.observe_preview(
            "bot-writer",
            &json!({ "sessionUpdate": "tool_call", "toolCallId": "t1", "kind": "read" }),
        );
        assert_eq!(
            line(&session, "bot-writer").as_deref(),
            Some("Reading the repo.")
        );
        session.observe_preview("bot-writer", &chunk("agent_message_chunk", "Six files."));
        assert_eq!(line(&session, "bot-writer").as_deref(), Some("Six files."));
    }

    /// Reasoning is not the transcript (`views/transcript.ts` says the same),
    /// so it neither shows nor breaks the sentence it lands inside.
    #[test]
    fn a_thought_is_neither_shown_nor_a_break() {
        let (_dir, mut session) = host_with_thread("bot-writer");
        session.observe_preview("bot-writer", &chunk("agent_message_chunk", "Half a"));
        session.observe_preview("bot-writer", &chunk("agent_thought_chunk", "hmm"));
        session.observe_preview("bot-writer", &chunk("agent_message_chunk", " sentence."));
        assert_eq!(
            line(&session, "bot-writer").as_deref(),
            Some("Half a sentence.")
        );
    }

    #[test]
    fn markdown_collapses_to_one_line() {
        let (_dir, mut session) = host_with_thread("bot-writer");
        session.observe_preview(
            "bot-writer",
            &chunk("agent_message_chunk", "Done.\n\n- one\n- two"),
        );
        assert_eq!(
            line(&session, "bot-writer").as_deref(),
            Some("Done. - one - two")
        );
    }

    /// Capped, on a character boundary, and stops writing once it is full.
    #[test]
    fn a_long_message_is_capped() {
        let (_dir, mut session) = host_with_thread("bot-writer");
        let long = "é".repeat(400);
        session.observe_preview("bot-writer", &chunk("agent_message_chunk", &long));
        let stored = line(&session, "bot-writer").expect("preview");
        assert_eq!(stored.chars().count(), PREVIEW_MAX);
    }
}
