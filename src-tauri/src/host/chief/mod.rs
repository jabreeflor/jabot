//! Chief's host tools: routing the crew is something JaBot does (#24).
//!
//! Decision #6 is emphatic about what Chief is **not**. It is not a third
//! runtime and not a thin host LLM loop with a tool-use `while` in it. It is
//! an ACP harness session exactly like every other bot, given four extra tools
//! that the *host* implements:
//!
//! | tool | what the host actually does |
//! |---|---|
//! | `handoff_to_bot` | opens the receiving bot's standing thread and prompts it |
//! | `spawn_code_session` | opens a thread in a registered folder — with a worktree (#23) |
//! | `fold_thread` | #15's fold, so a long job goes to the Inbox instead of the sidebar |
//! | `list_crew_status` | reads `bots` × `threads` × `runs` |
//!
//! Every one of them is an ordinary host action reached through the ordinary
//! host code paths. `handoff_to_bot` calls the same `thread/open` and
//! `session/prompt` a human clicking New Chat would; `spawn_code_session` gets
//! its worktree from #23 because it went through `thread_open`, not because
//! anything here knows about git. That is the point of routing being a host
//! action rather than a nested subagent: there is exactly one way a thread
//! gets opened, and Chief uses it.
//!
//! Three rules run through the file.
//!
//! **A handoff is recorded before it is delivered.** The `handoffs` row is
//! written first and `dispatched` is set afterwards, for the reason #5 gives
//! about the Inbox: work that was handed over and never heard is still work
//! that was handed over, and the human has to be able to find out. A bot whose
//! harness is not installed produces a real handoff with `dispatched: false`
//! and the reason in `detail` — never a silent nothing.
//!
//! **The allowlist is the boundary, checked twice.** `tools/list` offers only
//! the host tools this bot's `tools[]` actually carries, and `tools/call`
//! re-checks: a session that was live while the user unchipped a tool must not
//! keep using it. This is #18's enforcement rule applied to the host's own
//! tools — a tool the bot may not use is not in the list, so the model never
//! learns it exists.
//!
//! **Nothing here is Chief-only by identity.** The tools are gated on the
//! allowlist, not on `is_chief`. Chief is the bot that ships with the chips;
//! if the user gives Writer `fold_thread`, Writer can fold. The seat is what
//! is un-removable (#17), not the capability.

mod bridge;
mod tools;

use serde_json::{json, Value};
use uuid::Uuid;

use super::protocol::methods::{
    FoldPolicy, HandoffView, PromptMode, PromptParams, ThreadFoldParams, ThreadOpenParams,
};
use super::store::{BotRow, FolderRow, HandoffRow, NewHandoff, HANDOFF_CODE_SESSION, KIND_HANDOFF};
use super::HostSession;

pub use bridge::{Bridge, MCP_PROTOCOL_VERSION as MCP_VERSION, SERVER_NAME as MCP_SERVER_NAME};

/// The seeded Code bot's id. Folder threads belong to whichever crew member
/// owns coding work, and on a shipped install that is this row.
const CODE_BOT_ID: &str = "code";
/// …and its name, for an install where the user rebuilt it from scratch.
const CODE_BOT_NAME: &str = "code";

impl HostSession {
    // ---- the seam into the ACP session ---------------------------------

    /// The `mcpServers` element that carries Chief's host tools, or `None` for
    /// a thread whose bot has none of them.
    ///
    /// Called from `tools::mcp_servers_for_thread`, so a host tool travels the
    /// same road a provider server does and `session/new` has one list. What
    /// is different is who is on the other end: this one is answered by the
    /// host itself over loopback ([`bridge`]), which is what decision #6 means
    /// by "extra host tools" rather than four more catalog entries.
    pub(crate) fn chief_mcp_server(&mut self, thread_id: &str) -> Option<Value> {
        if self.chief_host_tools(thread_id).is_empty() {
            return None;
        }
        if !self.chief_bridges.contains_key(thread_id) {
            match Bridge::start(thread_id, self.adapter_wake()) {
                Ok(bridge) => {
                    self.chief_bridges.insert(thread_id.to_string(), bridge);
                }
                Err(err) => {
                    // A session with no host tools is a Chief that can still
                    // talk; a session that will not start is not. Log and go on.
                    eprintln!("thread {thread_id}: could not open the host tool bridge: {err}");
                    return None;
                }
            }
        }
        self.chief_bridges
            .get(thread_id)
            .map(|bridge| bridge.server_json())
    }

    /// Stop serving a thread's host tools. Called when its adapter goes.
    pub(crate) fn drop_chief_bridge(&mut self, thread_id: &str) {
        // `Bridge::drop` stops the listener and frees the port.
        self.chief_bridges.remove(thread_id);
    }

    /// Answer every host tool call waiting on the pump.
    ///
    /// Reentrancy is real here and not theoretical: a handoff prompts another
    /// thread, `session_prompt` ends by pumping, and the pump comes back here.
    /// The guard makes the inner pass a no-op rather than letting one tool call
    /// answer another mid-flight.
    pub(crate) fn pump_chief_tools(&mut self) {
        if self.chief_dispatching {
            return;
        }
        let mut waiting = Vec::new();
        for bridge in self.chief_bridges.values() {
            while let Ok(pending) = bridge.try_recv() {
                waiting.push(pending);
            }
        }
        if waiting.is_empty() {
            return;
        }
        self.chief_dispatching = true;
        for pending in waiting {
            let thread_id = pending.thread_id.clone();
            let answer = match &pending.ask {
                bridge::Ask::ListTools => Ok(json!(self
                    .chief_host_tools(&thread_id)
                    .iter()
                    .map(|spec| tools::describe(spec))
                    .collect::<Vec<_>>())),
                bridge::Ask::Call { tool, arguments } => {
                    self.chief_tool_call(&thread_id, tool, arguments)
                }
            };
            pending.answer(answer);
        }
        self.chief_dispatching = false;
    }

    /// The host tools this thread's bot is allowed to use, in catalog order.
    fn chief_host_tools(&self, thread_id: &str) -> Vec<&'static tools::HostToolSpec> {
        let allowlist = self.tool_allowlist(thread_id);
        tools::SPECS
            .iter()
            .filter(|spec| allowlist.iter().any(|id| id == spec.id))
            .collect()
    }

    // ---- the four tools --------------------------------------------------

    fn chief_tool_call(
        &mut self,
        thread_id: &str,
        tool: &str,
        args: &Value,
    ) -> Result<Value, String> {
        // Re-checked rather than trusted from the list: the user can unchip a
        // tool while the session is live, and the answer to "may I" has to be
        // the row as it stands now.
        if !self
            .chief_host_tools(thread_id)
            .iter()
            .any(|s| s.id == tool)
        {
            return Err(match tools::find(tool) {
                Some(_) => format!("{tool} is not one of this bot's tools"),
                None => format!("no such tool: {tool}"),
            });
        }
        match tool {
            "handoff_to_bot" => self.tool_handoff_to_bot(thread_id, args),
            "spawn_code_session" => self.tool_spawn_code_session(thread_id, args),
            "fold_thread" => self.tool_fold_thread(thread_id, args),
            "list_crew_status" => self.tool_list_crew_status(),
            other => Err(format!("no such tool: {other}")),
        }
    }

    /// Put a job on another crew member's standing thread.
    fn tool_handoff_to_bot(&mut self, from_thread: &str, args: &Value) -> Result<Value, String> {
        let reference = tools::required(args, "bot")?;
        let task = tools::required(args, "task")?;
        let context = tools::optional(args, "context");

        let target = self.find_bot(&reference)?;
        let from_bot = self.thread_bot(from_thread);
        if from_bot.as_deref() == Some(target.id.as_str()) {
            // Handing a job to yourself is a loop with extra steps, and the
            // model is better told than left to discover it.
            return Err(format!(
                "{} is the bot you are; do the work or hand it to someone else",
                target.name
            ));
        }

        let thread = self
            .open_standing_thread(&target)
            .map_err(|err| format!("could not open {}'s thread: {err}", target.name))?;

        let handoff = self.record_handoff(NewHandoff {
            kind: KIND_HANDOFF.to_string(),
            to_thread_id: thread.thread_id.clone(),
            to_bot_id: Some(target.id.clone()),
            from_thread_id: Some(from_thread.to_string()),
            from_bot_id: from_bot,
            task: task.clone(),
            context: context.clone(),
            dispatched: false,
            detail: None,
        })?;

        let (dispatched, detail) = self.deliver(
            &thread.thread_id,
            &handoff_prompt(&task, context.as_deref()),
        );
        self.settle_handoff(&handoff.id, dispatched, detail.as_deref());

        Ok(json!({
            "handoffId": handoff.id,
            "botId": target.id,
            "bot": target.name,
            "threadId": thread.thread_id,
            "cwd": thread.cwd,
            "dispatched": dispatched,
            "detail": detail,
        }))
    }

    /// Start a coding thread in a registered folder — the only way a bot gets
    /// a repository (decision #6). The worktree comes from #23 for free,
    /// because this opens the thread the same way New Chat does.
    fn tool_spawn_code_session(
        &mut self,
        from_thread: &str,
        args: &Value,
    ) -> Result<Value, String> {
        let reference = tools::required(args, "folder")?;
        let task = tools::required(args, "task")?;
        let title = tools::optional(args, "title").unwrap_or_else(|| summarize(&task));
        let base_ref = tools::optional(args, "baseRef");

        let folder = self.find_folder(&reference)?;
        let code_bot = self.code_bot();
        let harness_id = match &code_bot {
            Some(bot) => bot.harness_id.clone(),
            // No Code bot on this install: the thread still needs an engine,
            // and the caller's own is the only one known to be configured.
            None => self
                .thread_harness(from_thread)
                .ok_or_else(|| "there is no Code bot and no harness to fall back on".to_string())?,
        };
        let thread_id = format!("code-{}", Uuid::new_v4());
        let thread = self
            .thread_open(ThreadOpenParams {
                thread_id: Some(thread_id.clone()),
                title,
                cwd: folder
                    .repo_root
                    .clone()
                    .unwrap_or_else(|| folder.path.clone()),
                harness_id,
                runtime: None,
                folder_id: Some(folder.id.clone()),
                bot_id: code_bot.as_ref().map(|bot| bot.id.clone()),
                fold_policy: None,
                // Never the user's own checkout: a session Chief started while
                // the user is elsewhere is exactly the one that must not be
                // editing the tree they are looking at (#23).
                use_checkout: Some(false),
                base_ref,
            })
            .map_err(|err| format!("could not start a session in {}: {err}", folder.name))?;

        let handoff = self.record_handoff(NewHandoff {
            kind: HANDOFF_CODE_SESSION.to_string(),
            to_thread_id: thread.thread_id.clone(),
            to_bot_id: thread.bot_id.clone(),
            from_thread_id: Some(from_thread.to_string()),
            from_bot_id: self.thread_bot(from_thread),
            task: task.clone(),
            context: None,
            dispatched: false,
            detail: None,
        })?;

        let (dispatched, detail) = self.deliver(&thread.thread_id, &task);
        self.settle_handoff(&handoff.id, dispatched, detail.as_deref());

        Ok(json!({
            "handoffId": handoff.id,
            "threadId": thread.thread_id,
            "folderId": folder.id,
            "folder": folder.name,
            "cwd": thread.cwd,
            "worktreePath": thread.worktree_path,
            "branch": thread.branch,
            "repo": thread.repo,
            "dispatched": dispatched,
            "detail": detail,
        }))
    }

    /// Fold: hide the thread, keep the process (#15). The one gesture the
    /// product is built around, and Chief gets it as a tool so a long job can
    /// be put to sleep by the bot that started it.
    fn tool_fold_thread(&mut self, from_thread: &str, args: &Value) -> Result<Value, String> {
        let thread_id =
            tools::optional(args, "threadId").unwrap_or_else(|| from_thread.to_string());
        let policy = match tools::optional(args, "policy").as_deref() {
            None => None,
            Some("default") => Some(FoldPolicy::Default),
            Some("wait_for_inbox") => Some(FoldPolicy::WaitForInbox),
            Some(other) => {
                return Err(format!(
                    "unknown policy {other}; expected default or wait_for_inbox"
                ))
            }
        };
        let state = self
            .thread_fold(ThreadFoldParams {
                thread_id: thread_id.clone(),
                policy,
            })
            .map_err(|err| err.to_string())?;
        Ok(json!({
            "threadId": state.thread_id,
            "title": state.title,
            "state": state.state,
            "foldPolicy": state.fold_policy.as_str(),
            "acpState": state.process.acp_state,
        }))
    }

    /// What every bot is working on, folded jobs included.
    fn tool_list_crew_status(&mut self) -> Result<Value, String> {
        let store = self.store_or_err().map_err(|err| err.to_string())?;
        let bots = store.list_bots().map_err(|err| err.to_string())?;
        let mut crew = Vec::with_capacity(bots.len());
        for bot in bots {
            let threads = self
                .store_or_err()
                .map_err(|err| err.to_string())?
                .list_bot_threads(&bot.id)
                .map_err(|err| err.to_string())?;
            let mut working = Vec::with_capacity(threads.len());
            for thread in threads {
                let latest = self
                    .store_or_err()
                    .map_err(|err| err.to_string())?
                    .latest_run(&thread.id)
                    .map_err(|err| err.to_string())?;
                working.push(json!({
                    "threadId": thread.id,
                    "title": thread.title,
                    "state": thread.state,
                    "acpState": self.acp_state(&thread.id).as_str(),
                    "repo": thread.repo,
                    "branch": thread.branch,
                    "run": latest.map(|run| json!({ "state": run.state, "startedAt": run.started_at })),
                    "updatedAt": thread.updated_at,
                }));
            }
            crew.push(json!({
                "botId": bot.id,
                "name": bot.name,
                "isChief": bot.is_chief,
                "harnessId": bot.harness_id,
                "idle": working.is_empty(),
                "threads": working,
            }));
        }
        Ok(json!({ "crew": crew }))
    }

    // ---- shared machinery ------------------------------------------------

    /// Send the task to the receiving thread.
    ///
    /// `queue` rather than `reject`: a bot that is mid-turn when a second job
    /// arrives should end up with both, in order — which is exactly what #14's
    /// prompt queue is for. A failure here is reported, never thrown away: the
    /// handoff row already exists and this is what it records.
    fn deliver(&mut self, thread_id: &str, prompt: &str) -> (bool, Option<String>) {
        match self.session_prompt(PromptParams {
            thread_id: thread_id.to_string(),
            content: Value::String(prompt.to_string()),
            mode: Some(PromptMode::Queue),
            cwd: None,
            harness_id: None,
            runtime: None,
        }) {
            Ok(result) if result.queued => (
                true,
                Some("the bot is mid-task; this is queued behind the turn in flight".into()),
            ),
            Ok(_) => (true, None),
            Err(err) => (false, Some(err.to_string())),
        }
    }

    fn record_handoff(&mut self, new: NewHandoff) -> Result<HandoffRow, String> {
        self.store_or_err()
            .map_err(|err| err.to_string())?
            .insert_handoff(&new)
            .map_err(|err| err.to_string())
    }

    /// Best-effort: the handoff happened whether or not this write lands, and
    /// losing the delivery note must not turn a completed action into an error
    /// the model retries.
    fn settle_handoff(&mut self, handoff_id: &str, dispatched: bool, detail: Option<&str>) {
        let Ok(store) = self.store_or_err() else {
            return;
        };
        if let Err(err) = store.set_handoff_dispatched(handoff_id, dispatched, detail) {
            eprintln!("handoff {handoff_id}: could not record delivery: {err}");
        }
    }

    /// The handoff `thread/state` reports — resolved to a name, because
    /// `from_bot_id` is not something a human reading a thread can use.
    pub(crate) fn handoff_view(&self, thread_id: &str) -> Option<HandoffView> {
        let store = self.store.as_ref()?;
        let row = store.latest_handoff_to(thread_id).ok().flatten()?;
        let from_bot_name = row
            .from_bot_id
            .as_deref()
            .and_then(|id| store.get_bot(id).ok().flatten())
            .map(|bot| bot.name);
        Some(HandoffView {
            handoff_id: row.id,
            kind: row.kind,
            task: row.task,
            context: row.context,
            from_thread_id: row.from_thread_id,
            from_bot_id: row.from_bot_id,
            from_bot_name,
            dispatched: row.dispatched,
            detail: row.detail,
            created_at: row.created_at,
        })
    }

    /// Resolve a crew member the way a model would name one: by id, or by the
    /// name it just read out of `list_crew_status`.
    fn find_bot(&self, reference: &str) -> Result<BotRow, String> {
        let reference = reference.trim();
        let store = self.store_or_err().map_err(|err| err.to_string())?;
        let bots = store.list_bots().map_err(|err| err.to_string())?;
        bots.iter()
            .find(|bot| bot.id == reference)
            .or_else(|| {
                bots.iter()
                    .find(|bot| bot.name.eq_ignore_ascii_case(reference))
            })
            .cloned()
            .ok_or_else(|| {
                format!(
                    "no crew member called {reference}; the crew is {}",
                    bots.iter()
                        .map(|bot| bot.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }

    /// Same, for a folder: id, display name, or the path the user registered.
    fn find_folder(&self, reference: &str) -> Result<FolderRow, String> {
        let reference = reference.trim();
        let store = self.store_or_err().map_err(|err| err.to_string())?;
        let folders = store.list_folders().map_err(|err| err.to_string())?;
        folders
            .iter()
            .find(|folder| folder.id == reference)
            .or_else(|| {
                folders
                    .iter()
                    .find(|folder| folder.name.eq_ignore_ascii_case(reference))
            })
            .or_else(|| {
                folders.iter().find(|folder| {
                    folder.path == reference || folder.repo_root.as_deref() == Some(reference)
                })
            })
            .cloned()
            .ok_or_else(|| {
                if folders.is_empty() {
                    "no folders are registered yet; the user has to add one first".to_string()
                } else {
                    format!(
                        "no folder called {reference}; registered folders are {}",
                        folders
                            .iter()
                            .map(|folder| folder.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            })
    }

    /// Who owns folder threads (decision #6: "Code remains one crew member
    /// that owns many folder threads").
    ///
    /// Resolved by id first so a renamed Code bot still owns its work, and by
    /// name second so an install where the user rebuilt it from a blank bot
    /// still has an owner. `None` — the user deleted it and made no
    /// replacement — is legal: the thread is opened without a bot, exactly as a
    /// New Chat in a folder with no bot selected would be.
    fn code_bot(&self) -> Option<BotRow> {
        let store = self.store.as_ref()?;
        let bots = store.list_bots().ok()?;
        bots.iter()
            .find(|bot| bot.id == CODE_BOT_ID)
            .or_else(|| {
                bots.iter()
                    .find(|bot| bot.name.eq_ignore_ascii_case(CODE_BOT_NAME) && !bot.is_chief)
            })
            .cloned()
    }

    fn thread_bot(&self, thread_id: &str) -> Option<String> {
        self.store
            .as_ref()?
            .get_thread(thread_id)
            .ok()
            .flatten()?
            .bot_id
    }

    fn thread_harness(&self, thread_id: &str) -> Option<String> {
        Some(
            self.store
                .as_ref()?
                .get_thread(thread_id)
                .ok()
                .flatten()?
                .harness_id,
        )
    }
}

/// What the receiving agent actually reads.
///
/// Named as a handoff on purpose: the bot on the other end is a fresh session
/// with no idea a conversation happened elsewhere, and "someone asked me to do
/// this" is the difference between a coherent reply and a confused one.
fn handoff_prompt(task: &str, context: Option<&str>) -> String {
    let mut prompt = String::from("Handoff from Chief.\n\nTask:\n");
    prompt.push_str(task.trim());
    if let Some(context) = context {
        prompt.push_str("\n\nContext:\n");
        prompt.push_str(context.trim());
    }
    prompt.push('\n');
    prompt
}

/// A thread title from a task, when the caller did not give one. First line,
/// clipped on a word boundary — a sidebar row is not a paragraph.
fn summarize(task: &str) -> String {
    let line = task.lines().next().unwrap_or(task).trim();
    if line.chars().count() <= 60 {
        return line.to_string();
    }
    let mut clipped: String = line.chars().take(60).collect();
    if let Some(space) = clipped.rfind(' ') {
        if space > 20 {
            clipped.truncate(space);
        }
    }
    format!("{}…", clipped.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::crew::standing;
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use crate::host::protocol::{CREW_LIST, CREW_THREAD, CREW_UPDATE, HOST_HELLO, THREAD_STATE};
    use crate::host::repo::git::testing;

    /// A host with a real data directory: bots need memory directories, and a
    /// standing thread's `cwd` is one of them.
    fn host() -> (HostSession, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(&dir.path().join("data"));
        session
            .handle_request(JsonRpcRequest::new(RequestId::Number(1), HOST_HELLO, None))
            .result
            .expect("hello");
        (session, dir)
    }

    fn ok(session: &mut HostSession, method: &str, params: Value) -> Value {
        let response = session.handle_request(JsonRpcRequest::new(
            RequestId::Number(7),
            method,
            Some(params),
        ));
        assert!(response.error.is_none(), "{method}: {:?}", response.error);
        response.result.expect("result")
    }

    /// Chief calling one of its own tools, the way the bridge would.
    fn call(session: &mut HostSession, tool: &str, args: Value) -> Result<Value, String> {
        session.chief_tool_call(&standing::thread_id_for("chief"), tool, &args)
    }

    /// Chief has to have a thread of its own before it can call anything from
    /// one — the same standing thread the bridge would be serving.
    fn chief_at_work(session: &mut HostSession) {
        ok(session, CREW_THREAD, json!({ "botId": "chief" }));
    }

    fn bot_named<'a>(crew: &'a [Value], name: &str) -> &'a Value {
        crew.iter()
            .find(|bot| bot["name"] == name)
            .unwrap_or_else(|| panic!("no bot named {name}"))
    }

    // ---- the standing thread (D-009 left this to #24) --------------------

    #[test]
    fn a_bot_gets_one_standing_thread_in_its_memory_directory_and_no_worktree() {
        let (mut session, _dir) = host();
        let crew = ok(&mut session, CREW_LIST, json!({}));
        let writer = bot_named(crew["bots"].as_array().unwrap(), "Writer").clone();

        let thread = ok(
            &mut session,
            CREW_THREAD,
            json!({ "botId": writer["botId"] }),
        );
        assert_eq!(thread["cwd"], writer["memoryDir"]);
        assert_eq!(thread["botId"], writer["botId"]);
        assert_eq!(thread["title"], "Writer");
        assert_eq!(thread["state"], "active");
        // Decision #6: a worker has no repo unless it asks for one.
        assert!(thread["worktreePath"].is_null(), "{thread}");
        assert!(thread["repo"].is_null());

        // Asking twice is asking once: the id is derived from the bot, so two
        // handoffs arriving together cannot make two standing threads.
        let again = ok(
            &mut session,
            CREW_THREAD,
            json!({ "botId": writer["botId"] }),
        );
        assert_eq!(again["threadId"], thread["threadId"]);
    }

    #[test]
    fn a_standing_thread_for_a_bot_that_is_not_there_says_so() {
        let (mut session, _dir) = host();
        let response = session.handle_request(JsonRpcRequest::new(
            RequestId::Number(3),
            CREW_THREAD,
            Some(json!({ "botId": "nobody" })),
        ));
        let error = response.error.expect("no such bot");
        assert!(error.message.contains("nobody"), "{}", error.message);
    }

    // ---- handoff_to_bot --------------------------------------------------

    /// The claim the issue makes: a handoff is traceable. The receiving bot's
    /// thread has to be able to say where the work came from.
    #[test]
    fn a_handoff_lands_on_the_receiving_bot_and_the_thread_records_who_sent_it() {
        let (mut session, _dir) = host();
        chief_at_work(&mut session);

        let result = call(
            &mut session,
            "handoff_to_bot",
            json!({
                "bot": "Writer",
                "task": "Draft the launch note",
                "context": "Jabree hates exclamation marks",
            }),
        )
        .expect("handoff");

        assert_eq!(result["bot"], "Writer");
        assert_eq!(result["threadId"], standing::thread_id_for("writer"));

        let thread = ok(
            &mut session,
            THREAD_STATE,
            json!({ "threadId": result["threadId"] }),
        );
        let handoff = &thread["handoff"];
        assert_eq!(handoff["handoffId"], result["handoffId"]);
        assert_eq!(handoff["kind"], "handoff");
        assert_eq!(handoff["task"], "Draft the launch note");
        assert_eq!(handoff["context"], "Jabree hates exclamation marks");
        assert_eq!(handoff["fromBotId"], "chief");
        assert_eq!(handoff["fromBotName"], "Chief");
        assert_eq!(handoff["fromThreadId"], standing::thread_id_for("chief"));

        // No `claude` on a test machine, so nothing could be dispatched — and
        // that is exactly the case the row exists for. The handoff happened;
        // `dispatched` says nobody heard it, and `detail` says why.
        assert_eq!(handoff["dispatched"], false);
        assert!(
            handoff["detail"].as_str().is_some_and(|d| !d.is_empty()),
            "a failed dispatch has to say why: {handoff}"
        );
    }

    /// A thread the human started has no handoff, and saying otherwise would
    /// put a phantom sender on every conversation in the app.
    #[test]
    fn a_thread_nobody_handed_off_carries_no_handoff() {
        let (mut session, _dir) = host();
        chief_at_work(&mut session);
        let chief = ok(
            &mut session,
            THREAD_STATE,
            json!({ "threadId": standing::thread_id_for("chief") }),
        );
        assert!(chief["handoff"].is_null(), "{chief}");
    }

    #[test]
    fn handing_off_to_nobody_names_the_crew_instead_of_failing_silently() {
        let (mut session, _dir) = host();
        chief_at_work(&mut session);

        let refused = call(
            &mut session,
            "handoff_to_bot",
            json!({ "bot": "Gardener", "task": "water the plants" }),
        )
        .expect_err("no such bot");
        assert!(refused.contains("Gardener"), "{refused}");
        // The model has to be able to retry with a real name.
        assert!(refused.contains("Inbox Mgr"), "{refused}");
    }

    #[test]
    fn a_handoff_needs_a_task_and_cannot_be_to_yourself() {
        let (mut session, _dir) = host();
        chief_at_work(&mut session);

        let taskless =
            call(&mut session, "handoff_to_bot", json!({ "bot": "Writer" })).expect_err("no task");
        assert!(taskless.contains("task"), "{taskless}");

        let looped = call(
            &mut session,
            "handoff_to_bot",
            json!({ "bot": "chief", "task": "route this" }),
        )
        .expect_err("self handoff");
        assert!(looped.contains("Chief"), "{looped}");

        // Neither attempt wrote a thread or a trail.
        assert!(session
            .lifecycle_thread(&standing::thread_id_for("writer"))
            .unwrap()
            .is_none());
    }

    // ---- spawn_code_session ----------------------------------------------

    /// Decision #6: `spawn_code_session` is how a worker gets a repo `cwd` at
    /// all, and #23's worktree manager is what gives it a checkout. Both halves
    /// are asserted here, because either one alone is a coding session that
    /// cannot work.
    #[test]
    fn a_code_session_gets_a_registered_folder_a_worktree_and_the_code_bot() {
        let (mut session, _dir) = host();
        chief_at_work(&mut session);
        let repo = tempfile::tempdir().unwrap();
        testing::init_repo(repo.path(), Some("git@github.com:jabreeflor/jabot.git"));
        let folder = ok(
            &mut session,
            crate::host::protocol::FOLDER_REGISTER,
            json!({ "path": repo.path().to_string_lossy(), "name": "JaBot" }),
        );

        let result = call(
            &mut session,
            "spawn_code_session",
            json!({ "folder": "JaBot", "task": "Add a --version flag to the CLI" }),
        )
        .expect("spawn");

        assert_eq!(result["folderId"], folder["folderId"]);
        assert_eq!(result["folder"], "JaBot");

        let thread = ok(
            &mut session,
            THREAD_STATE,
            json!({ "threadId": result["threadId"] }),
        );
        // The thread belongs to the crew member that owns folder work…
        assert_eq!(thread["botId"], "code");
        // …it works in a host-owned tree, not the user's checkout (#23)…
        let worktree = thread["worktreePath"].as_str().expect("a worktree");
        assert!(std::path::Path::new(worktree).is_dir(), "{worktree}");
        assert_eq!(thread["cwd"], worktree);
        assert_ne!(
            thread["cwd"].as_str(),
            Some(repo.path().to_string_lossy().as_ref())
        );
        // …on its own branch…
        let branch = thread["branch"].as_str().expect("a branch");
        assert!(branch.starts_with("jabot/"), "{branch}");
        // …and the title came from the task the caller gave.
        assert_eq!(thread["title"], "Add a --version flag to the CLI");
        // …and the trail says this was Chief's doing, not the user's.
        assert_eq!(thread["handoff"]["kind"], "code_session");
        assert_eq!(thread["handoff"]["fromBotId"], "chief");
    }

    #[test]
    fn a_code_session_in_a_folder_nobody_registered_says_which_are() {
        let (mut session, _dir) = host();
        chief_at_work(&mut session);

        let empty = call(
            &mut session,
            "spawn_code_session",
            json!({ "folder": "~/src/jabot", "task": "fix it" }),
        )
        .expect_err("no folders");
        assert!(empty.contains("no folders are registered"), "{empty}");

        let repo = tempfile::tempdir().unwrap();
        testing::init_repo(repo.path(), None);
        ok(
            &mut session,
            crate::host::protocol::FOLDER_REGISTER,
            json!({ "path": repo.path().to_string_lossy(), "name": "JaBot" }),
        );
        let wrong = call(
            &mut session,
            "spawn_code_session",
            json!({ "folder": "Nowhere", "task": "fix it" }),
        )
        .expect_err("no such folder");
        assert!(wrong.contains("JaBot"), "{wrong}");
    }

    // ---- fold_thread -----------------------------------------------------

    #[test]
    fn folding_a_thread_hides_it_and_an_unknown_policy_is_refused() {
        let (mut session, _dir) = host();
        chief_at_work(&mut session);
        let writer = ok(&mut session, CREW_THREAD, json!({ "botId": "writer" }));
        let thread_id = writer["threadId"].as_str().unwrap().to_string();

        let folded = call(
            &mut session,
            "fold_thread",
            json!({ "threadId": thread_id, "policy": "wait_for_inbox" }),
        )
        .expect("fold");
        assert_eq!(folded["state"], "folded");
        assert_eq!(folded["foldPolicy"], "wait_for_inbox");

        let refused = call(
            &mut session,
            "fold_thread",
            json!({ "threadId": thread_id, "policy": "forever" }),
        )
        .expect_err("unknown policy");
        assert!(refused.contains("wait_for_inbox"), "{refused}");
    }

    /// Omitting the id folds the conversation the tool was called from, which
    /// is what "put this long job to sleep" means from inside it.
    #[test]
    fn folding_without_an_id_folds_the_calling_thread() {
        let (mut session, _dir) = host();
        chief_at_work(&mut session);
        let folded = call(&mut session, "fold_thread", json!({})).expect("fold");
        assert_eq!(folded["threadId"], standing::thread_id_for("chief"));
        assert_eq!(folded["state"], "folded");
    }

    // ---- list_crew_status ------------------------------------------------

    #[test]
    fn crew_status_reports_every_bot_and_what_it_is_working_on() {
        let (mut session, _dir) = host();
        chief_at_work(&mut session);
        call(
            &mut session,
            "handoff_to_bot",
            json!({ "bot": "Inbox Mgr", "task": "clear the overnight mail" }),
        )
        .expect("handoff");

        let status = call(&mut session, "list_crew_status", json!({})).expect("status");
        let crew = status["crew"].as_array().expect("crew");
        assert_eq!(crew.len(), 6);

        let inbox = crew
            .iter()
            .find(|bot| bot["name"] == "Inbox Mgr")
            .expect("Inbox Mgr");
        assert_eq!(inbox["idle"], false);
        assert_eq!(
            inbox["threads"][0]["threadId"],
            standing::thread_id_for("inboxm")
        );
        assert_eq!(inbox["threads"][0]["state"], "active");

        // A bot nobody has asked for anything is idle, and says so.
        let research = crew
            .iter()
            .find(|bot| bot["name"] == "Research")
            .expect("Research");
        assert_eq!(research["idle"], true);
        assert_eq!(research["threads"].as_array().unwrap().len(), 0);
    }

    /// Fold hides a thread from the human's sidebar. It must not hide it from
    /// the crew roster, or Chief hands a second job to a bot that is busy.
    #[test]
    fn a_folded_job_is_still_something_the_bot_is_working_on() {
        let (mut session, _dir) = host();
        chief_at_work(&mut session);
        call(
            &mut session,
            "handoff_to_bot",
            json!({ "bot": "Writer", "task": "the long one" }),
        )
        .expect("handoff");
        call(
            &mut session,
            "fold_thread",
            json!({ "threadId": standing::thread_id_for("writer") }),
        )
        .expect("fold");

        let status = call(&mut session, "list_crew_status", json!({})).expect("status");
        let writer = status["crew"]
            .as_array()
            .unwrap()
            .iter()
            .find(|bot| bot["name"] == "Writer")
            .expect("Writer");
        assert_eq!(writer["idle"], false);
        assert_eq!(writer["threads"][0]["state"], "folded");
    }

    // ---- the allowlist is the boundary -----------------------------------

    /// #18's rule, applied to the host's own tools: a tool the bot is not
    /// allowed to use is not offered, and calling it anyway is refused. The
    /// second half matters because the user can unchip a tool while the
    /// session is live.
    #[test]
    fn a_bot_only_gets_the_host_tools_its_record_carries() {
        let (mut session, _dir) = host();
        chief_at_work(&mut session);
        let chief_thread = standing::thread_id_for("chief");

        let offered: Vec<String> = session
            .chief_host_tools(&chief_thread)
            .iter()
            .map(|spec| spec.id.to_string())
            .collect();
        assert_eq!(
            offered,
            vec![
                "handoff_to_bot",
                "spawn_code_session",
                "fold_thread",
                "list_crew_status"
            ]
        );

        ok(
            &mut session,
            CREW_UPDATE,
            json!({ "botId": "chief", "tools": ["list_crew_status"] }),
        );
        assert_eq!(
            session
                .chief_host_tools(&chief_thread)
                .iter()
                .map(|spec| spec.id)
                .collect::<Vec<_>>(),
            vec!["list_crew_status"]
        );
        let refused = call(
            &mut session,
            "handoff_to_bot",
            json!({ "bot": "Writer", "task": "anything" }),
        )
        .expect_err("unchipped");
        assert!(refused.contains("not one of this bot's tools"), "{refused}");
    }

    /// A worker with no host-tool chips gets no bridge at all — no port bound,
    /// nothing on `session/new`. A thread that never needed one must not be
    /// paying for one.
    #[test]
    fn only_a_bot_with_host_tools_is_served_a_host_tool_server() {
        let (mut session, _dir) = host();
        ok(&mut session, CREW_THREAD, json!({ "botId": "writer" }));
        ok(&mut session, CREW_THREAD, json!({ "botId": "chief" }));

        assert!(session
            .chief_mcp_server(&standing::thread_id_for("writer"))
            .is_none());

        let server = session
            .chief_mcp_server(&standing::thread_id_for("chief"))
            .expect("Chief carries all four host tools");
        assert_eq!(server["type"], "http");
        assert_eq!(server["name"], MCP_SERVER_NAME);
        // Loopback and nothing else: the port is known only to this process
        // and to the session/new it goes out on.
        assert!(
            server["url"]
                .as_str()
                .unwrap()
                .starts_with("http://127.0.0.1:"),
            "{server}"
        );
        assert!(server["headers"][0]["value"]
            .as_str()
            .unwrap()
            .starts_with("Bearer "));

        // And the same thread gets the same server rather than a second port.
        let again = session
            .chief_mcp_server(&standing::thread_id_for("chief"))
            .expect("still there");
        assert_eq!(again["url"], server["url"]);
    }

    /// The array `session/new` actually receives: Chief's host tools ride
    /// alongside the provider servers #18 builds, because from the adapter's
    /// side one `mcpServers` list is the only seam there is.
    #[test]
    fn the_host_tool_server_travels_on_the_sessions_mcp_server_list() {
        let (mut session, _dir) = host();
        ok(&mut session, CREW_THREAD, json!({ "botId": "chief" }));
        let servers = session.mcp_servers_for_thread(&standing::thread_id_for("chief"));
        let names: Vec<&str> = servers
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|server| server["name"].as_str())
            .collect();
        assert_eq!(names, vec![MCP_SERVER_NAME]);
    }

    #[test]
    fn a_task_with_no_title_gets_one_short_enough_for_a_sidebar_row() {
        assert_eq!(
            summarize("Fix the flaky test\nand then some"),
            "Fix the flaky test"
        );
        let long = summarize(
            "Rework the permission broker so a withdrawn ask cannot be answered twice by two devices",
        );
        assert!(long.chars().count() <= 61, "{long}");
        assert!(long.ends_with('…'), "{long}");
    }
}
