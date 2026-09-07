//! Conversational bot drafts (#237).
//!
//! `draft_bot` validates and persists a proposal. Only a full-device UI Save
//! commits it, through the same `insert_bot` path `crew/create` uses. The
//! model never supplies identity, ownership, or Save.

use serde::Deserialize;
use serde_json::{json, Value};

use super::super::protocol::error::RpcError;
use super::super::protocol::methods::{
    BotDraftView, CrewDraftDismissParams, CrewDraftEventParams, CrewDraftGetParams,
    CrewDraftSaveParams, CrewDraftSaveResult, CrewDraftsResult, CREW_DRAFT,
};
use super::super::store::{
    BotDraftPatch, BotDraftRow, NewBotDraft, DRAFT_PENDING, DRAFT_SAVED, DRAFT_STALE,
};
use super::{checked_tools_json, HOST_TOOLS};
use super::super::HostSession;

pub const MAX_NAME: usize = 80;
pub const MAX_INSTRUCTIONS: usize = 32_768;
pub const MAX_REQUEST_KEY: usize = 128;
pub const MAX_TOOLS: usize = 32;
const DEFAULT_COLOR: &str = "b-green";

/// Child bots never inherit creation tools. Granting them would let a
/// proposal approve more proposers.
const FORBIDDEN_CHILD_TOOLS: &[&str] = &["draft_bot", "get_bot_draft"];

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftBotArgs {
    pub request_key: String,
    pub name: String,
    pub instructions: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub harness_id: Option<String>,
    #[serde(default)]
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetBotDraftArgs {
    #[serde(default)]
    pub request_key: Option<String>,
    #[serde(default)]
    pub draft_id: Option<String>,
}

impl HostSession {
    pub fn crew_drafts(&self) -> Result<CrewDraftsResult, RpcError> {
        let store = self.crew_store()?;
        let rows = store.list_reviewable_drafts().map_err(internal)?;
        let drafts = rows
            .into_iter()
            .map(|row| self.draft_view(self.refresh_stale(row)?))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CrewDraftsResult { drafts })
    }

    pub fn crew_draft_get(&self, params: CrewDraftGetParams) -> Result<BotDraftView, RpcError> {
        let row = if let Some(id) = params.draft_id.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            self.crew_store()?
                .get_bot_draft(id)
                .map_err(internal)?
                .ok_or_else(|| RpcError::InvalidParams(format!("no such draft: {id}")))?
        } else {
            let key = params
                .request_key
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| RpcError::InvalidParams("draftId or requestKey is required".into()))?;
            let source = params
                .source_bot_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    RpcError::InvalidParams("sourceBotId is required with requestKey".into())
                })?;
            self.crew_store()?
                .get_bot_draft_by_request(source, key)
                .map_err(internal)?
                .ok_or_else(|| RpcError::InvalidParams("no such draft".into()))?
        };
        self.draft_view(self.refresh_stale(row)?)
    }

    pub fn crew_draft_save(
        &mut self,
        params: CrewDraftSaveParams,
    ) -> Result<CrewDraftSaveResult, RpcError> {
        let store = self.crew_store()?;
        let current = store
            .get_bot_draft(&params.draft_id)
            .map_err(internal)?
            .ok_or_else(|| RpcError::InvalidParams(format!("no such draft: {}", params.draft_id)))?;
        let current = self.refresh_stale(current)?;
        if current.status == DRAFT_SAVED {
            let bot_id = current
                .bot_id
                .clone()
                .ok_or_else(|| RpcError::Internal("saved draft is missing its bot".into()))?;
            let bot = store
                .get_bot(&bot_id)
                .map_err(internal)?
                .ok_or_else(|| RpcError::Internal(format!("saved bot {bot_id} is gone")))?;
            self.ensure_memory(&bot);
            return Ok(CrewDraftSaveResult {
                draft: self.draft_view(current)?,
                bot: self.bot_view(bot, 0, None),
                run_started: false,
            });
        }
        if current.status != DRAFT_PENDING && current.status != DRAFT_STALE {
            return Err(RpcError::InvalidParams(
                "a dismissed draft cannot be saved".into(),
            ));
        }

        let name = params
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(current.name.as_str());
        check_name(name)?;
        let instructions = params
            .instructions
            .as_deref()
            .unwrap_or(current.instructions.as_str());
        check_instructions(instructions)?;
        let color = match params.color.as_deref() {
            Some(color) => self.checked_color(color)?,
            None => current.color.clone(),
        };
        let tools = match params.tools.as_ref() {
            Some(tools) => parse_child_tools(tools)?,
            None => serde_json::from_str::<Vec<String>>(&current.tools_json).unwrap_or_default(),
        };
        let tools_json = checked_tools_json(&tools)?;
        let harness_id = match params.harness_id.as_deref() {
            Some(id) => self.checked_harness(id)?,
            None => self.checked_harness(&current.harness_id)?,
        };

        let patch = BotDraftPatch {
            name: Some(name.to_string()),
            instructions: Some(instructions.to_string()),
            tools_json: Some(tools_json),
            harness_id: Some(harness_id),
            color: Some(color),
        };
        let device_id = self
            .connected_device
            .as_ref()
            .map(|device| device.device_id.clone());
        let (row, bot) = self
            .crew_store()?
            .save_bot_draft(
                &params.draft_id,
                params.revision,
                &patch,
                device_id.as_deref(),
            )
            .map_err(save_error)?;
        self.ensure_memory(&bot);
        let workspace_warning = self.memory_dir(&bot.id).and_then(|dir| {
            if dir.join("MEMORY.md").exists() {
                None
            } else {
                Some(format!(
                    "bot saved; memory files still need repair in {}",
                    dir.display()
                ))
            }
        });
        let mut draft = self.draft_view(row)?;
        draft.workspace_warning = workspace_warning;
        self.notify_crew_draft(&draft);
        Ok(CrewDraftSaveResult {
            bot: self.bot_view(bot, 0, None),
            draft,
            run_started: false,
        })
    }

    pub fn crew_draft_dismiss(
        &mut self,
        params: CrewDraftDismissParams,
    ) -> Result<BotDraftView, RpcError> {
        let row = self
            .crew_store()?
            .dismiss_bot_draft(&params.draft_id, params.revision)
            .map_err(save_error)?;
        let view = self.draft_view(row)?;
        self.notify_crew_draft(&view);
        Ok(view)
    }

    /// MCP `draft_bot`. Identity comes from the authenticated thread.
    pub(crate) fn tool_draft_bot(&mut self, thread_id: &str, args: &Value) -> Result<Value, String> {
        let parsed: DraftBotArgs = parse_args(args)?;
        let source_bot = self
            .thread_bot(thread_id)
            .ok_or_else(|| "this thread has no bot; draft_bot needs a crew member".to_string())?;
        check_request_key(&parsed.request_key).map_err(|e| e.to_string())?;
        check_name(&parsed.name).map_err(|e| e.to_string())?;
        check_instructions(&parsed.instructions).map_err(|e| e.to_string())?;
        let tools = parse_child_tools(&parsed.tools).map_err(|e| e.to_string())?;
        let tools_json = checked_tools_json(&tools).map_err(|e| e.to_string())?;
        if let Some(template) = parsed.template_id.as_deref() {
            if super::templates::find(template).is_none() {
                return Err(format!("no such template: {template}"));
            }
        }
        let harness_id = self
            .resolve_draft_harness(
                thread_id,
                parsed.harness_id.as_deref(),
                parsed.template_id.as_deref(),
            )
            .map_err(|e| e.to_string())?;
        let payload_hash = hash_payload(
            &parsed.name,
            &parsed.instructions,
            &tools,
            &harness_id,
            parsed.template_id.as_deref(),
        );
        let run_id = self.current_run_id(thread_id);
        let row = self
            .crew_store()
            .map_err(|e| e.to_string())?
            .insert_bot_draft(&NewBotDraft {
                request_key: parsed.request_key.trim().to_string(),
                payload_hash,
                source_bot_id: source_bot,
                source_thread_id: Some(thread_id.to_string()),
                source_run_id: run_id,
                name: parsed.name.trim().to_string(),
                instructions: parsed.instructions.to_string(),
                tools_json,
                harness_id,
                color: DEFAULT_COLOR.to_string(),
                template_id: parsed.template_id,
            })
            .map_err(|err| err.to_string())?;
        let view = self.draft_view(row).map_err(|e| e.to_string())?;
        self.record_draft_in_transcript(thread_id, &view);
        self.notify_crew_draft(&view);
        Ok(json!({
            "draftId": view.draft_id,
            "status": view.status,
            "saved": false,
            "name": view.name,
            "nameWarning": view.name_warning,
        }))
    }

    pub(crate) fn tool_get_bot_draft(
        &self,
        thread_id: &str,
        args: &Value,
    ) -> Result<Value, String> {
        let parsed: GetBotDraftArgs = parse_args(args)?;
        let source_bot = self
            .thread_bot(thread_id)
            .ok_or_else(|| "this thread has no bot".to_string())?;
        let row = if let Some(id) = parsed.draft_id.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            self.crew_store()
                .map_err(|e| e.to_string())?
                .get_bot_draft(id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("no such draft: {id}"))?
        } else if let Some(key) = parsed
            .request_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            self.crew_store()
                .map_err(|e| e.to_string())?
                .get_bot_draft_by_request(&source_bot, key)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "no such draft".to_string())?
        } else {
            return Err("draftId or requestKey is required".into());
        };
        if row.source_bot_id != source_bot {
            return Err("that draft is not one you proposed".into());
        }
        let view = self.draft_view(row).map_err(|e| e.to_string())?;
        let mut result = json!({
            "draftId": view.draft_id,
            "status": view.status,
            "saved": view.status == DRAFT_SAVED,
            "name": view.name,
        });
        if let Some(bot_id) = view.bot_id {
            result["botId"] = json!(bot_id);
        }
        Ok(result)
    }

    fn resolve_draft_harness(
        &self,
        thread_id: &str,
        requested: Option<&str>,
        template_id: Option<&str>,
    ) -> Result<String, RpcError> {
        if let Some(id) = requested.map(str::trim).filter(|s| !s.is_empty()) {
            return self.checked_harness(id);
        }
        if let Some(template) = template_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .and_then(super::templates::find)
        {
            return self.checked_harness(&template.harness_id);
        }
        let thread = self
            .crew_store()?
            .get_thread(thread_id)
            .map_err(internal)?
            .ok_or_else(|| RpcError::InvalidParams("this thread is gone".into()))?;
        if let Some(bot_id) = thread.bot_id {
            if let Some(bot) = self.crew_store()?.get_bot(&bot_id).map_err(internal)? {
                return self.checked_harness(&bot.harness_id);
            }
        }
        self.checked_harness(&thread.harness_id)
    }

    fn refresh_stale(&self, row: BotDraftRow) -> Result<BotDraftRow, RpcError> {
        if row.status != DRAFT_PENDING {
            return Ok(row);
        }
        let reason = self.stale_reason(&row)?;
        if let Some(reason) = reason {
            return self
                .crew_store()?
                .mark_bot_draft_stale(&row.id, &reason)
                .map_err(internal);
        }
        Ok(row)
    }

    fn stale_reason(&self, row: &BotDraftRow) -> Result<Option<String>, RpcError> {
        let store = self.crew_store()?;
        match store.get_bot(&row.source_bot_id).map_err(internal)? {
            None => Ok(Some("the proposing bot is gone".into())),
            Some(bot) => {
                let tools: Vec<String> =
                    serde_json::from_str(&bot.tools_json).unwrap_or_default();
                if tools.iter().any(|id| id == "draft_bot") {
                    Ok(None)
                } else {
                    Ok(Some("the proposing bot no longer has draft_bot".into()))
                }
            }
        }
    }

    fn draft_view(&self, row: BotDraftRow) -> Result<BotDraftView, RpcError> {
        let tools = serde_json::from_str::<Vec<String>>(&row.tools_json).unwrap_or_default();
        let source_bot_name = self
            .store
            .as_ref()
            .and_then(|store| store.get_bot(&row.source_bot_id).ok().flatten())
            .map(|bot| bot.name);
        let name_warning = self.duplicate_name_warning(&row.name, row.bot_id.as_deref())?;
        Ok(BotDraftView {
            draft_id: row.id,
            request_key: row.request_key,
            status: row.status,
            revision: row.revision,
            name: row.name,
            instructions: row.instructions,
            tools,
            harness_id: row.harness_id,
            color: row.color,
            template_id: row.template_id,
            source_bot_id: row.source_bot_id,
            source_bot_name,
            source_thread_id: row.source_thread_id,
            bot_id: row.bot_id,
            name_warning,
            stale_reason: row.stale_reason,
            workspace_warning: None,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }

    fn duplicate_name_warning(
        &self,
        name: &str,
        saved_bot_id: Option<&str>,
    ) -> Result<Option<String>, RpcError> {
        let Some(store) = self.store.as_ref() else {
            return Ok(None);
        };
        let matches = store
            .list_bots()
            .map_err(internal)?
            .into_iter()
            .filter(|bot| bot.name.eq_ignore_ascii_case(name))
            .filter(|bot| saved_bot_id != Some(bot.id.as_str()))
            .count();
        if matches == 0 {
            return Ok(None);
        }
        Ok(Some(format!(
            "another crew member is already named {name}; drafts are identified by id, not name"
        )))
    }

    fn notify_crew_draft(&mut self, view: &BotDraftView) {
        let params = CrewDraftEventParams {
            draft_id: view.draft_id.clone(),
            status: view.status.clone(),
            name: view.name.clone(),
            source_bot_id: view.source_bot_id.clone(),
            source_bot_name: view.source_bot_name.clone(),
            source_thread_id: view.source_thread_id.clone(),
            bot_id: view.bot_id.clone(),
        };
        self.push_unlogged(CREW_DRAFT, params);
    }

    fn record_draft_in_transcript(&mut self, thread_id: &str, view: &BotDraftView) {
        let acp = json!({
            "sessionUpdate": "state_update",
            "jabot": {
                "event": "bot_draft_submitted",
                "draftId": view.draft_id,
                "name": view.name,
                "status": view.status,
                "saved": false,
            }
        });
        let seq = self.persist_transcript_event(thread_id, "session/update", &acp);
        self.notify_session_update_at(thread_id, acp, seq);
    }

    fn current_run_id(&self, thread_id: &str) -> Option<String> {
        self.open_run(thread_id).map(|(id, _)| id)
    }
}

fn parse_args<T: for<'de> Deserialize<'de>>(args: &Value) -> Result<T, String> {
    serde_json::from_value(args.clone()).map_err(|err| {
        if err.to_string().contains("unknown field") {
            format!("unexpected argument: {err}")
        } else {
            err.to_string()
        }
    })
}

fn parse_child_tools(tools: &[String]) -> Result<Vec<String>, RpcError> {
    if tools.len() > MAX_TOOLS {
        return Err(RpcError::InvalidParams(format!(
            "tools: at most {MAX_TOOLS} ids"
        )));
    }
    for tool in tools {
        let tool = tool.trim();
        if FORBIDDEN_CHILD_TOOLS.contains(&tool) {
            return Err(RpcError::InvalidParams(format!(
                "{tool} cannot be granted to a proposed bot; creation is a reviewable proposal"
            )));
        }
    }
    Ok(tools.to_vec())
}

fn check_request_key(key: &str) -> Result<(), RpcError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(RpcError::InvalidParams("requestKey is required".into()));
    }
    if key.len() > MAX_REQUEST_KEY {
        return Err(RpcError::InvalidParams(format!(
            "requestKey is longer than {MAX_REQUEST_KEY} characters"
        )));
    }
    Ok(())
}

fn check_name(name: &str) -> Result<(), RpcError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(RpcError::InvalidParams("name is required".into()));
    }
    if name.len() > MAX_NAME {
        return Err(RpcError::InvalidParams(format!(
            "name is longer than {MAX_NAME} characters"
        )));
    }
    Ok(())
}

fn check_instructions(instructions: &str) -> Result<(), RpcError> {
    if instructions.len() > MAX_INSTRUCTIONS {
        return Err(RpcError::InvalidParams(format!(
            "instructions are longer than {MAX_INSTRUCTIONS} characters"
        )));
    }
    if instructions.trim().is_empty() {
        return Err(RpcError::InvalidParams("instructions are required".into()));
    }
    Ok(())
}

fn hash_payload(
    name: &str,
    instructions: &str,
    tools: &[String],
    harness_id: &str,
    template_id: Option<&str>,
) -> String {
    let tools_json = serde_json::to_string(tools).unwrap_or_else(|_| "[]".into());
    let canonical = format!(
        "harness={harness_id}\ninstructions={instructions}\nname={}\ntemplate={}\ntools={tools_json}",
        name.trim(),
        template_id.unwrap_or("")
    );
    fnv1a_hex(&canonical)
}

fn fnv1a_hex(input: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn internal(err: crate::host::store::StoreError) -> RpcError {
    RpcError::Internal(err.to_string())
}

fn save_error(err: crate::host::store::StoreError) -> RpcError {
    let message = err.to_string();
    if message.contains("conflict") {
        RpcError::DraftConflict(message)
    } else if matches!(err, crate::host::store::StoreError::NotFound(_))
        || message.contains("not found")
        || message.contains("dismissed")
    {
        RpcError::InvalidParams(message)
    } else if matches!(err, crate::host::store::StoreError::Invalid(_)) {
        if message.contains("conflict") {
            RpcError::DraftConflict(message)
        } else {
            RpcError::InvalidParams(message)
        }
    } else {
        RpcError::Internal(message)
    }
}

/// Compile-time reminder: the forbidden child tools are real host tools.
#[allow(dead_code)]
fn _forbidden_are_real() {
    for id in FORBIDDEN_CHILD_TOOLS {
        assert!(HOST_TOOLS.iter().any(|tool| tool.id == *id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::error::{DRAFT_CONFLICT, INVALID_PARAMS};
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use crate::host::protocol::{
        CREW_CREATE, CREW_DRAFT_DISMISS, CREW_DRAFT_GET, CREW_DRAFT_SAVE, CREW_DRAFTS, CREW_LIST,
        CREW_THREAD, CREW_UPDATE, HOST_HELLO,
    };
    use crate::host::crew::standing;
    use serde_json::{json, Value};

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

    fn err(session: &mut HostSession, method: &str, params: Value) -> crate::host::JsonRpcError {
        session
            .handle_request(JsonRpcRequest::new(
                RequestId::Number(8),
                method,
                Some(params),
            ))
            .error
            .unwrap_or_else(|| panic!("{method} was expected to fail"))
    }

    fn propose(session: &mut HostSession, key: &str, name: &str) -> Value {
        ok(session, CREW_THREAD, json!({ "botId": "chief" }));
        session
            .tool_draft_bot(
                &standing::thread_id_for("chief"),
                &json!({
                    "requestKey": key,
                    "name": name,
                    "instructions": "Research topics and cite primary sources.",
                    "tools": ["browser"]
                }),
            )
            .expect("draft_bot")
    }

    #[test]
    fn draft_bot_persists_pending_and_does_not_create_a_bot() {
        let (mut session, _dir) = host();
        let before = ok(&mut session, CREW_LIST, json!({}))["bots"]
            .as_array()
            .unwrap()
            .len();
        let proposed = propose(&mut session, "research-1", "Researcher");
        assert_eq!(proposed["saved"], false);
        assert_eq!(proposed["status"], "pending_review");
        assert!(proposed["draftId"].as_str().unwrap().len() > 8);
        let after = ok(&mut session, CREW_LIST, json!({}))["bots"]
            .as_array()
            .unwrap()
            .len();
        assert_eq!(after, before);

        let listed = ok(&mut session, CREW_DRAFTS, json!({}));
        assert_eq!(listed["drafts"].as_array().unwrap().len(), 1);
        assert_eq!(listed["drafts"][0]["name"], "Researcher");
        assert_eq!(listed["drafts"][0]["sourceBotName"], "Chief");
    }

    #[test]
    fn the_same_request_key_and_payload_is_idempotent() {
        let (mut session, _dir) = host();
        let first = propose(&mut session, "k1", "Researcher");
        let second = session
            .tool_draft_bot(
                &standing::thread_id_for("chief"),
                &json!({
                    "requestKey": "k1",
                    "name": "Researcher",
                    "instructions": "Research topics and cite primary sources.",
                    "tools": ["browser"]
                }),
            )
            .expect("retry");
        assert_eq!(first["draftId"], second["draftId"]);
    }

    #[test]
    fn the_same_request_key_with_a_changed_payload_conflicts() {
        let (mut session, _dir) = host();
        propose(&mut session, "k1", "Researcher");
        let refused = session
            .tool_draft_bot(
                &standing::thread_id_for("chief"),
                &json!({
                    "requestKey": "k1",
                    "name": "Other",
                    "instructions": "Different instructions.",
                    "tools": ["browser"]
                }),
            )
            .expect_err("conflict");
        assert!(refused.contains("already has a different draft"), "{refused}");
    }

    #[test]
    fn save_creates_one_bot_and_a_retry_returns_it() {
        let (mut session, _dir) = host();
        let proposed = propose(&mut session, "k-save", "Researcher");
        let draft_id = proposed["draftId"].as_str().unwrap().to_string();
        let got = ok(
            &mut session,
            CREW_DRAFT_GET,
            json!({ "draftId": draft_id }),
        );
        let saved = ok(
            &mut session,
            CREW_DRAFT_SAVE,
            json!({
                "draftId": draft_id,
                "revision": got["revision"],
            }),
        );
        assert_eq!(saved["runStarted"], false);
        assert_eq!(saved["bot"]["name"], "Researcher");
        assert_eq!(saved["bot"]["tools"], json!(["browser"]));
        assert_eq!(saved["draft"]["status"], "saved");
        let bot_id = saved["bot"]["botId"].as_str().unwrap().to_string();

        let again = ok(
            &mut session,
            CREW_DRAFT_SAVE,
            json!({
                "draftId": draft_id,
                "revision": got["revision"],
            }),
        );
        assert_eq!(again["bot"]["botId"], bot_id);
        let listed = ok(&mut session, CREW_LIST, json!({}));
        let names: Vec<_> = listed["bots"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|bot| bot["name"] == "Researcher")
            .collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn a_stale_revision_does_not_overwrite_edits() {
        let (mut session, _dir) = host();
        let proposed = propose(&mut session, "k-rev", "Researcher");
        let draft_id = proposed["draftId"].as_str().unwrap().to_string();
        let refused = err(
            &mut session,
            CREW_DRAFT_SAVE,
            json!({ "draftId": draft_id, "revision": 99 }),
        );
        assert_eq!(refused.code, DRAFT_CONFLICT);
    }

    #[test]
    fn dismiss_does_not_create_a_bot_and_blocks_later_save() {
        let (mut session, _dir) = host();
        let proposed = propose(&mut session, "k-d", "Researcher");
        let draft_id = proposed["draftId"].as_str().unwrap().to_string();
        let got = ok(
            &mut session,
            CREW_DRAFT_GET,
            json!({ "draftId": draft_id }),
        );
        ok(
            &mut session,
            CREW_DRAFT_DISMISS,
            json!({ "draftId": draft_id, "revision": got["revision"] }),
        );
        let refused = err(
            &mut session,
            CREW_DRAFT_SAVE,
            json!({ "draftId": draft_id, "revision": got["revision"] }),
        );
        assert_eq!(refused.code, INVALID_PARAMS);
        assert!(ok(&mut session, CREW_LIST, json!({}))["bots"]
            .as_array()
            .unwrap()
            .iter()
            .all(|bot| bot["name"] != "Researcher"));
    }

    #[test]
    fn child_creation_tools_are_refused_and_unknown_fields_are_rejected() {
        let (mut session, _dir) = host();
        ok(&mut session, CREW_THREAD, json!({ "botId": "chief" }));
        let refused = session
            .tool_draft_bot(
                &standing::thread_id_for("chief"),
                &json!({
                    "requestKey": "k-bad",
                    "name": "Clone",
                    "instructions": "Recruit more bots.",
                    "tools": ["draft_bot"]
                }),
            )
            .expect_err("no self-replication");
        assert!(refused.contains("cannot be granted"), "{refused}");

        let unknown = session
            .tool_draft_bot(
                &standing::thread_id_for("chief"),
                &json!({
                    "requestKey": "k-unk",
                    "name": "X",
                    "instructions": "Y",
                    "isChief": true
                }),
            )
            .expect_err("closed schema");
        assert!(unknown.contains("unexpected") || unknown.contains("unknown"), "{unknown}");
    }

    #[test]
    fn a_bot_without_the_grant_cannot_call_draft_bot() {
        let (mut session, _dir) = host();
        let writer = ok(
            &mut session,
            CREW_CREATE,
            json!({ "name": "Writer", "instructions": "Draft.", "tools": [], "harnessId": "claude" }),
        );
        let writer_id = writer["botId"].as_str().unwrap();
        ok(&mut session, CREW_THREAD, json!({ "botId": writer_id }));
        let refused = session
            .chief_tool_call(
                &standing::thread_id_for(writer_id),
                "draft_bot",
                &json!({
                    "requestKey": "nope",
                    "name": "X",
                    "instructions": "Y"
                }),
            )
            .expect_err("no grant");
        assert!(refused.contains("not one of this bot's tools"), "{refused}");
    }

    #[test]
    fn revoking_the_grant_marks_a_pending_draft_stale() {
        let (mut session, _dir) = host();
        let proposed = propose(&mut session, "k-stale", "Researcher");
        let draft_id = proposed["draftId"].as_str().unwrap().to_string();
        ok(
            &mut session,
            CREW_UPDATE,
            json!({
                "botId": "chief",
                "tools": ["handoff_to_bot", "list_crew_status"]
            }),
        );
        let got = ok(
            &mut session,
            CREW_DRAFT_GET,
            json!({ "draftId": draft_id }),
        );
        assert_eq!(got["status"], "stale");
        assert!(got["staleReason"].as_str().unwrap().contains("draft_bot"));
    }

    #[test]
    fn get_bot_draft_is_scoped_to_the_proposer() {
        let (mut session, _dir) = host();
        let proposed = propose(&mut session, "k-scope", "Researcher");
        let recruiter = ok(&mut session, CREW_THREAD, json!({ "botId": "bot-recruiter" }));
        let _ = recruiter;
        let refused = session
            .tool_get_bot_draft(
                &standing::thread_id_for("bot-recruiter"),
                &json!({ "draftId": proposed["draftId"] }),
            )
            .expect_err("other bot");
        assert!(refused.contains("not one you proposed"), "{refused}");
    }

    #[test]
    fn blank_and_oversized_fields_are_refused() {
        let (mut session, _dir) = host();
        ok(&mut session, CREW_THREAD, json!({ "botId": "chief" }));
        for args in [
            json!({ "requestKey": "k", "name": "  ", "instructions": "ok" }),
            json!({ "requestKey": "", "name": "X", "instructions": "ok" }),
            json!({ "requestKey": "k", "name": "X", "instructions": "   " }),
        ] {
            session
                .tool_draft_bot(&standing::thread_id_for("chief"), &args)
                .expect_err(&args.to_string());
        }
        let long = "n".repeat(MAX_NAME + 1);
        session
            .tool_draft_bot(
                &standing::thread_id_for("chief"),
                &json!({
                    "requestKey": "k-long",
                    "name": long,
                    "instructions": "ok"
                }),
            )
            .expect_err("name too long");
    }
}
