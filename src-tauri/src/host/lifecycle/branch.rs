//! Conversation branching (#266).
//!
//! `thread/branch` opens a new Code thread whose transcript is the source's
//! log through a chosen message, then records the link so a second click on
//! the same message returns the same child. The original is not rewritten.
//! Project context follows New Chat: same folder, harness and runtime, and a
//! fresh worktree when the source had one — two threads never share a
//! checkout.

use serde_json::json;

use super::super::crew::standing::STANDING_PREFIX;
use super::super::protocol::error::RpcError;
use super::super::protocol::methods::{
    BranchedFromView, FoldPolicy, RuntimeSpec, ThreadBranchParams, ThreadOpenParams,
    ThreadRefParams, ThreadStateResult,
};
use super::super::HostSession;
use super::store_error;

/// Host-owned marker on the child's log: this conversation was forked.
pub const BRANCHED_FROM: &str = "branched_from";

impl HostSession {
    /// Fork a Code conversation at `through_seq`.
    ///
    /// Idempotent for a live child of `(thread, throughSeq)`. A mapping whose
    /// child was deleted is dropped so the next click can write a new one.
    pub fn thread_branch(
        &mut self,
        params: ThreadBranchParams,
    ) -> Result<ThreadStateResult, RpcError> {
        params.validate()?;
        let source = self
            .lifecycle_thread(&params.thread_id)?
            .ok_or_else(|| RpcError::ThreadNotFound(params.thread_id.clone()))?;
        if source.deleted_at.is_some() {
            return Err(RpcError::ThreadNotFound(params.thread_id.clone()));
        }
        if source.id.starts_with(STANDING_PREFIX) {
            return Err(RpcError::InvalidParams(
                "Branch in new chat is for Code conversations".into(),
            ));
        }
        let head = self
            .store_or_err()?
            .transcript_head(&source.id)
            .map_err(store_error)?;
        if params.through_seq > head {
            return Err(RpcError::InvalidParams(format!(
                "throughSeq {} is past the end of this conversation ({head})",
                params.through_seq
            )));
        }

        if let Some(existing) = self.live_branch_of(&source.id, params.through_seq)? {
            return self.thread_state(ThreadRefParams {
                thread_id: existing,
            });
        }

        let folder_cwd = source
            .folder_id
            .as_deref()
            .and_then(|id| {
                self.store
                    .as_ref()?
                    .get_folder(id)
                    .ok()
                    .flatten()
                    .map(|folder| folder.path)
            })
            .unwrap_or_else(|| source.cwd.clone());
        let runtime = serde_json::from_str::<RuntimeSpec>(&source.runtime_json).ok();
        let opened = self.thread_open(ThreadOpenParams {
            thread_id: None,
            title: branch_title(&source.title),
            cwd: folder_cwd,
            harness_id: source.harness_id.clone(),
            runtime,
            folder_id: source.folder_id.clone(),
            bot_id: None,
            fold_policy: Some(FoldPolicy::parse(&source.fold_policy)),
            use_checkout: None,
            base_ref: None,
            model: None,
        })?;
        let child_id = opened.thread_id.clone();

        if let Err(err) =
            self.store_or_err()?
                .copy_transcript_through(&source.id, &child_id, params.through_seq)
        {
            let _ = self.thread_delete(ThreadRefParams {
                thread_id: child_id,
            });
            return Err(store_error(err));
        }
        self.persist_transcript_event(
            &child_id,
            "session/update",
            &json!({
                "sessionUpdate": "state_update",
                "jabot": {
                    "event": BRANCHED_FROM,
                    "threadId": source.id,
                    "title": source.title,
                    "throughSeq": params.through_seq,
                }
            }),
        );

        let wrote = self
            .store_or_err()?
            .insert_branch(&source.id, params.through_seq, &child_id)
            .map_err(store_error)?;
        if !wrote {
            // A raced second write won. Drop this child so the unique pair
            // still names one conversation, then return the winner.
            if let Some(winner) = self.live_branch_of(&source.id, params.through_seq)? {
                if winner != child_id {
                    let _ = self.thread_delete(ThreadRefParams {
                        thread_id: child_id,
                    });
                    return self.thread_state(ThreadRefParams { thread_id: winner });
                }
            }
        }

        self.thread_state(ThreadRefParams {
            thread_id: opened.thread_id,
        })
    }

    /// First prompt on a branch that has never had an ACP session: the copied
    /// history is prepended so the agent can continue from the cut. Later
    /// prompts, and any thread that is not a branch, pass through unchanged.
    pub(crate) fn branch_first_prompt(
        &self,
        thread_id: &str,
        content: &serde_json::Value,
        new_session: bool,
    ) -> serde_json::Value {
        if !new_session {
            return content.clone();
        }
        let Some(store) = self.store.as_ref() else {
            return content.clone();
        };
        if store.branch_source_of(thread_id).ok().flatten().is_none() {
            return content.clone();
        }
        if store
            .get_thread(thread_id)
            .ok()
            .flatten()
            .and_then(|row| row.acp_session_id)
            .is_some()
        {
            return content.clone();
        }
        let Ok(events) = store.transcript_after(thread_id, 0) else {
            return content.clone();
        };
        let history = super::super::transcript::branch_history_text(&events);
        super::super::transcript::prepend_branch_history(content, &history)
    }

    /// The source this thread was forked from, when it is a branch.
    pub(crate) fn branched_from_view(&self, thread_id: &str) -> Option<BranchedFromView> {
        let store = self.store.as_ref()?;
        let row = store.branch_source_of(thread_id).ok()??;
        let source = store.get_thread(&row.source_thread_id).ok()??;
        Some(BranchedFromView {
            thread_id: source.id,
            title: source.title,
            through_seq: row.through_seq,
        })
    }

    fn live_branch_of(
        &self,
        source_thread_id: &str,
        through_seq: i64,
    ) -> Result<Option<String>, RpcError> {
        let store = self.store_or_err()?;
        let Some(row) = store
            .get_branch(source_thread_id, through_seq)
            .map_err(store_error)?
        else {
            return Ok(None);
        };
        match store
            .get_thread(&row.branch_thread_id)
            .map_err(store_error)?
        {
            Some(child) if child.deleted_at.is_none() => Ok(Some(child.id)),
            _ => {
                store
                    .delete_branch(source_thread_id, through_seq)
                    .map_err(store_error)?;
                Ok(None)
            }
        }
    }
}

fn branch_title(source: &str) -> String {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        "Branch".into()
    } else {
        format!("Branch of {trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::super::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use super::super::super::protocol::methods::{ThreadBranchParams, HOST_HELLO};
    use super::super::super::HostSession;
    use super::branch_title;

    fn persistent() -> (tempfile::TempDir, HostSession) {
        let dir = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(dir.path());
        session
            .handle_request(JsonRpcRequest::new(RequestId::Number(1), HOST_HELLO, None))
            .result
            .expect("hello");
        (dir, session)
    }

    fn open(session: &mut HostSession, thread_id: &str) {
        session
            .thread_open(super::super::super::protocol::methods::ThreadOpenParams {
                thread_id: Some(thread_id.into()),
                title: "Auth migration".into(),
                cwd: "/tmp".into(),
                harness_id: "claude".into(),
                runtime: None,
                folder_id: None,
                bot_id: None,
                fold_policy: None,
                use_checkout: None,
                base_ref: None,
                model: None,
            })
            .unwrap();
    }

    fn say(session: &mut HostSession, thread_id: &str, who: &str, text: &str) {
        session.persist_transcript_event(
            thread_id,
            "session/update",
            &json!({
                "sessionUpdate": format!("{who}_message_chunk"),
                "content": { "type": "text", "text": text },
            }),
        );
    }

    #[test]
    fn names_the_source() {
        assert_eq!(branch_title("Auth migration"), "Branch of Auth migration");
        assert_eq!(branch_title("  "), "Branch");
    }

    #[test]
    fn copies_history_through_the_cut_and_leaves_the_source() {
        let (_dir, mut session) = persistent();
        open(&mut session, "t-src");
        say(&mut session, "t-src", "user", "one");
        say(&mut session, "t-src", "agent", "two");
        say(&mut session, "t-src", "user", "three");

        let child = session
            .thread_branch(ThreadBranchParams {
                thread_id: "t-src".into(),
                through_seq: 2,
            })
            .unwrap();
        assert_ne!(child.thread_id, "t-src");
        assert_eq!(child.title, "Branch of Auth migration");
        let from = child.branched_from.expect("link back");
        assert_eq!(from.thread_id, "t-src");
        assert_eq!(from.through_seq, 2);

        let replay = session
            .thread_transcript(
                super::super::super::protocol::methods::ThreadTranscriptParams {
                    thread_id: child.thread_id.clone(),
                    after_seq: None,
                    limit: None,
                },
            )
            .unwrap();
        assert_eq!(replay.events.len(), 3);
        assert_eq!(replay.events[0].seq, 1);
        assert_eq!(replay.events[1].seq, 2);
        assert_eq!(replay.events[2].payload["jabot"]["event"], "branched_from");

        let source = session
            .thread_transcript(
                super::super::super::protocol::methods::ThreadTranscriptParams {
                    thread_id: "t-src".into(),
                    after_seq: None,
                    limit: None,
                },
            )
            .unwrap();
        assert_eq!(source.events.len(), 3);

        say(&mut session, "t-src", "agent", "later on the source");
        say(
            &mut session,
            &child.thread_id,
            "user",
            "later on the branch",
        );
        assert_eq!(
            session
                .thread_transcript(
                    super::super::super::protocol::methods::ThreadTranscriptParams {
                        thread_id: "t-src".into(),
                        after_seq: None,
                        limit: None,
                    },
                )
                .unwrap()
                .events
                .len(),
            4
        );
        assert_eq!(
            session
                .thread_transcript(
                    super::super::super::protocol::methods::ThreadTranscriptParams {
                        thread_id: child.thread_id,
                        after_seq: None,
                        limit: None,
                    },
                )
                .unwrap()
                .events
                .len(),
            4
        );
    }

    #[test]
    fn a_second_click_returns_the_same_child() {
        let (_dir, mut session) = persistent();
        open(&mut session, "t-src");
        say(&mut session, "t-src", "user", "one");
        let first = session
            .thread_branch(ThreadBranchParams {
                thread_id: "t-src".into(),
                through_seq: 1,
            })
            .unwrap();
        let second = session
            .thread_branch(ThreadBranchParams {
                thread_id: "t-src".into(),
                through_seq: 1,
            })
            .unwrap();
        assert_eq!(first.thread_id, second.thread_id);
    }

    #[test]
    fn standing_threads_cannot_branch() {
        let (_dir, mut session) = persistent();
        open(&mut session, "bot-chief");
        say(&mut session, "bot-chief", "user", "hi");
        let err = session
            .thread_branch(ThreadBranchParams {
                thread_id: "bot-chief".into(),
                through_seq: 1,
            })
            .unwrap_err();
        assert!(err.to_string().contains("Code conversations"), "{err}");
    }

    #[test]
    fn first_prompt_carries_copied_history() {
        let (_dir, mut session) = persistent();
        open(&mut session, "t-src");
        say(&mut session, "t-src", "user", "start the migration");
        say(&mut session, "t-src", "agent", "reading the store");
        let child = session
            .thread_branch(ThreadBranchParams {
                thread_id: "t-src".into(),
                through_seq: 2,
            })
            .unwrap();
        let content = session.branch_first_prompt(&child.thread_id, &json!("and now this"), true);
        let text = content.as_str().unwrap();
        assert!(text.contains("start the migration"), "{text}");
        assert!(text.contains("reading the store"), "{text}");
        assert!(text.ends_with("and now this"), "{text}");
    }
}
