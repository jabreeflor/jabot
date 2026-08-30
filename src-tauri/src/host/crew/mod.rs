//! The crew store: bots as data, and CRUD over the host API (#17).
//!
//! Decision #6 settled what a bot *is* — "every crew bot is an ACP harness
//! session", a scope (persona, tools, memory) over an engine from the harness
//! catalog. This module is that record made real: `bots` rows, four methods,
//! and a bot editor that **is** the record rather than a form that happens to
//! resemble one.
//!
//! Four rules run through everything here.
//!
//! **Chief is a seat, not a bot.** It is seeded, `bots_one_chief` allows
//! exactly one, and [`HostSession::crew_remove`] refuses it. Everything else
//! about Chief is editable — its persona, its tools, its harness — because the
//! thing that cannot go is the seat, not the user's ability to shape it.
//!
//! **Adding from a template is a snapshot.** The pack supplies the fields the
//! caller did not, the row keeps `template_id` as provenance, and nothing
//! reads back through it. Editing a bot afterwards is editing a bot; the pack
//! has no further say, and a pack that changes in a later release does not
//! reach into anybody's crew.
//!
//! **The vocabulary is closed.** A colour has to be one the UI can render and
//! a tool id has to be one of the catalogs, because a bot's chips are an
//! allowlist the session layer enforces (#18) — a tool id nothing recognises
//! is a capability that silently never arrives. The check is here, at the
//! write, so a bad value cannot reach the store at all. An uploaded icon is
//! the same rule applied to a bigger value: it goes into an `<img src>` on the
//! way back out, so the host takes only the three raster `data:` URLs its own
//! renderer produces and refuses anything that would fetch or execute —
//! however carefully the renderer already checked, a host does not trust its
//! caller.
//!
//! **Isolation is per bot, not per store.** One SQLite, one keychain, but each
//! bot gets its own memory directory ([`memory`]) and each bot's session is
//! its own ACP `sessionId` — harness session stores (`~/.claude`,
//! `HERMES_HOME`) are never shared between crew, and a credential never lands
//! in a bot directory.

mod memory;
pub(crate) mod standing;
mod templates;

use std::path::PathBuf;

use super::protocol::error::RpcError;
use super::protocol::methods::{
    BotTemplateView, BotView, CrewCreateParams, CrewHostToolView, CrewListResult, CrewRefParams,
    CrewRemoveResult, CrewUpdateParams,
};
use super::store::{BotPatch, BotRow, NewBot, Store, StoreError};
use super::tools::catalog::CATALOG;
use super::HostSession;

/// The colour tokens a bot may have. Closed on purpose: `bots.color` is a CSS
/// class in the renderer (`BotColor` in `src/components/types.ts`), so a value
/// outside this list is a blob with no gradient rather than a new colour.
pub const BOT_COLORS: &[&str] = &[
    "b-teal", "b-yellow", "b-purple", "b-violet", "b-blue", "b-orange", "b-pink", "b-green",
];

/// The media types an uploaded icon may claim to be — exactly the three the
/// renderer's canvas can encode.
///
/// `image/svg+xml` is deliberately not among them. An SVG is a document, it can
/// carry script and remote references, and this value is written to a row that
/// is later handed to an `<img src>`; the three below are pixels and nothing
/// else. A GIF the user picks is fine as *input* — it reaches the renderer's
/// canvas as pixels and leaves it as one of these.
const IMAGE_PREFIXES: &[&str] = &[
    "data:image/png;base64,",
    "data:image/jpeg;base64,",
    "data:image/webp;base64,",
];

/// The most an icon may weigh once decoded, matching the cap the renderer
/// applies before sending. 256 KiB is generous for a 256px square and small
/// enough that a crew of twelve is a couple of megabytes of SQLite in the
/// worst case.
const MAX_IMAGE_BYTES: usize = 256 * 1024;

/// Chief's host tools (decision #6). These are the host's own actions, not MCP
/// servers — routing a handoff is something JaBot does, not something a
/// provider does — so they are not in the `tools/list` catalog and no chip in
/// the editor offers them. They are here so the crew grid can name them
/// instead of printing `spawn_code_session` at the user. #24 implements them.
pub const HOST_TOOLS: &[CrewHostTool] = &[
    CrewHostTool {
        id: "handoff_to_bot",
        label: "Handoff",
        blurb: "Pass a job to another bot in the crew",
    },
    CrewHostTool {
        id: "spawn_code_session",
        label: "Spawn code session",
        blurb: "Start a coding thread in one of your folders",
    },
    CrewHostTool {
        id: "fold_thread",
        label: "Fold thread",
        blurb: "Put a long job to sleep until it has an answer",
    },
    CrewHostTool {
        id: "list_crew_status",
        label: "Crew status",
        blurb: "See what every bot is working on",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrewHostTool {
    pub id: &'static str,
    pub label: &'static str,
    pub blurb: &'static str,
}

/// Whether an allowlist entry names something the host can actually resolve:
/// an MCP catalog id (#18) or one of Chief's host tools.
pub fn is_known_tool(id: &str) -> bool {
    CATALOG.iter().any(|entry| entry.id == id) || HOST_TOOLS.iter().any(|tool| tool.id == id)
}

impl HostSession {
    /// The whole Crew view in one answer: the crew, the shipped templates, and
    /// the host tools whose ids Chief's chips carry.
    ///
    /// Listing also materialises each bot's memory directory. That is a write
    /// on a read, and it is deliberate: the directory *is* a worker's `cwd`
    /// (decision #6), so a crew whose workspaces do not exist is a crew that
    /// cannot be prompted. Failures are reported to the log and do not fail the
    /// list — a read-only data directory should cost the user their memory
    /// files, not their crew.
    pub fn crew_list(&mut self) -> Result<CrewListResult, RpcError> {
        let rows = self.crew_store()?.list_bots().map_err(internal)?;
        let bots = rows
            .into_iter()
            .map(|row| {
                self.ensure_memory(&row);
                self.bot_view(row)
            })
            .collect();
        Ok(CrewListResult {
            bots,
            templates: templates::templates(),
            host_tools: host_tool_views(),
        })
    }

    /// Add a bot, optionally copying a template's fields into it.
    ///
    /// The copy happens here and only here. What lands in the row is a
    /// snapshot: the template id is kept so the UI can say where the bot came
    /// from, and nothing ever follows it back.
    pub fn crew_create(&mut self, params: CrewCreateParams) -> Result<BotView, RpcError> {
        let template = match params.template_id.as_deref() {
            Some(id) => Some(
                templates::find(id)
                    .ok_or_else(|| RpcError::InvalidParams(format!("no such template: {id}")))?,
            ),
            None => None,
        };
        let default =
            |from: fn(&BotTemplateView) -> String| template.as_ref().map(from).unwrap_or_default();

        let name = params
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| default(|t| t.name.clone()));
        if name.is_empty() {
            return Err(RpcError::InvalidParams("name is required".into()));
        }
        let color = match params.color {
            Some(color) => color,
            // A blank template-less bot still needs a face; green is the one
            // the seeded crew does not use.
            None if template.is_none() => "b-green".to_string(),
            None => default(|t| t.color.clone()),
        };
        let instructions = params
            .instructions
            .unwrap_or_else(|| default(|t| t.instructions.clone()));
        let tools = match params.tools {
            Some(tools) => tools,
            None => template
                .as_ref()
                .map(|t| t.tools.clone())
                .unwrap_or_default(),
        };
        let harness_id = params
            .harness_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| default(|t| t.harness_id.clone()));

        let color = self.checked_color(&color)?;
        let tools_json = checked_tools_json(&tools)?;
        let harness_id = self.checked_harness(&harness_id)?;
        // No three-way spelling on create: a bot that does not exist yet has no
        // picture to keep, so "none" and "leave it alone" are one instruction.
        let image = match params.image.as_deref() {
            Some(image) => checked_image(image)?,
            None => None,
        };

        let store = self.crew_store()?;
        let new = NewBot {
            name,
            color,
            instructions: instructions.trim().to_string(),
            tools_json,
            harness_id,
            template_id: params.template_id,
            sort_order: store.next_bot_sort_order().map_err(internal)?,
            image,
        };
        let row = store.insert_bot(&new).map_err(store_error)?;
        self.ensure_memory(&row);
        Ok(self.bot_view(row))
    }

    /// Save the editor. Every field is optional and an omitted one is left
    /// alone, so changing a harness cannot quietly discard the instructions
    /// the user spent ten minutes on.
    ///
    /// Chief is editable here — only removal is refused.
    pub fn crew_update(&mut self, params: CrewUpdateParams) -> Result<BotView, RpcError> {
        let color = match params.color.as_deref() {
            Some(color) => Some(self.checked_color(color)?),
            None => None,
        };
        let tools_json = match params.tools.as_deref() {
            Some(tools) => Some(checked_tools_json(tools)?),
            None => None,
        };
        let harness_id = match params.harness_id.as_deref() {
            Some(id) => Some(self.checked_harness(id)?),
            None => None,
        };
        // The three-way field: absent leaves the picture alone, `""` clears it,
        // anything else has to be an icon.
        let image = match params.image.as_deref() {
            Some(image) => Some(checked_image(image)?),
            None => None,
        };
        let patch = BotPatch {
            name: params.name.as_deref().map(str::trim).map(str::to_string),
            color,
            instructions: params
                .instructions
                .as_deref()
                .map(str::trim)
                .map(str::to_string),
            tools_json,
            harness_id,
            image,
        };
        let row = self
            .crew_store()?
            .update_bot(&params.bot_id, &patch)
            .map_err(|err| match err {
                StoreError::NotFound(_) => bot_not_found(&params.bot_id),
                other => store_error(other),
            })?;
        // The persona is what a session reads out of the memory directory, so
        // saving the record and refreshing the file are one action.
        self.ensure_memory(&row);
        Ok(self.bot_view(row))
    }

    /// Remove a bot from the crew.
    ///
    /// Chief is refused. Everything else goes, and what it leaves behind is
    /// deliberate: its threads survive with their own cwd and harness, and its
    /// memory directory stays on disk. Deleting a user's markdown notes as a
    /// side effect of tidying the crew grid is not a thing a Remove button
    /// should do, and the result says where they are.
    pub fn crew_remove(&mut self, params: CrewRefParams) -> Result<CrewRemoveResult, RpcError> {
        let store = self.crew_store()?;
        let row = store
            .get_bot(&params.bot_id)
            .map_err(internal)?
            .ok_or_else(|| bot_not_found(&params.bot_id))?;
        if row.is_chief {
            return Err(RpcError::ChiefRequired {
                bot_id: params.bot_id,
            });
        }
        let detached_threads = store.delete_bot(&params.bot_id).map_err(|err| match err {
            StoreError::NotFound(_) => bot_not_found(&params.bot_id),
            other => store_error(other),
        })?;
        Ok(CrewRemoveResult {
            bot_id: params.bot_id,
            removed: true,
            detached_threads,
            memory_dir: self.memory_dir(&row.id).map(display),
        })
    }

    /// Where this bot's markdown lives, or `None` on a host with no data
    /// directory — an ephemeral host has nowhere to put one, and saying so is
    /// better than naming a path that will never exist.
    fn memory_dir(&self, bot_id: &str) -> Option<PathBuf> {
        self.data_dir
            .as_deref()
            .map(|data_dir| memory::dir_for(data_dir, bot_id))
    }

    /// Best-effort: the crew is the record, the files are a projection of it.
    fn ensure_memory(&self, row: &BotRow) {
        let Some(dir) = self.memory_dir(&row.id) else {
            return;
        };
        if let Err(err) = memory::ensure(&dir, &row.name, &row.instructions) {
            eprintln!(
                "bot {}: could not write memory files in {}: {err}",
                row.id,
                dir.display()
            );
        }
    }

    fn bot_view(&self, row: BotRow) -> BotView {
        // A row whose `tools_json` will not parse is a row nothing wrote —
        // every write here validates it — so an empty allowlist is the safe
        // reading: the bot gets no tools rather than all of them.
        let tools = serde_json::from_str::<Vec<String>>(&row.tools_json).unwrap_or_default();
        BotView {
            memory_dir: self.memory_dir(&row.id).map(display),
            bot_id: row.id,
            name: row.name,
            color: row.color,
            instructions: row.instructions,
            tools,
            image: row.image,
            harness_id: row.harness_id,
            is_chief: row.is_chief,
            template_id: row.template_id,
            sort_order: row.sort_order,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }

    fn checked_color(&self, color: &str) -> Result<String, RpcError> {
        let color = color.trim();
        if BOT_COLORS.contains(&color) {
            return Ok(color.to_string());
        }
        Err(RpcError::InvalidParams(format!(
            "unknown colour {color}; expected one of {}",
            BOT_COLORS.join(", ")
        )))
    }

    /// A harness the store does not have is a bot whose first prompt cannot
    /// spawn. `bots.harness_id` is a foreign key, so this would fail anyway —
    /// but as a constraint violation the user cannot read.
    fn checked_harness(&self, harness_id: &str) -> Result<String, RpcError> {
        let harness_id = harness_id.trim();
        if harness_id.is_empty() {
            return Err(RpcError::InvalidParams("harnessId is required".into()));
        }
        match self
            .crew_store()?
            .get_harness(harness_id)
            .map_err(internal)?
        {
            Some(_) => Ok(harness_id.to_string()),
            None => Err(RpcError::InvalidParams(format!(
                "no such harness: {harness_id}"
            ))),
        }
    }

    fn crew_store(&self) -> Result<&Store, RpcError> {
        self.store.as_ref().ok_or(RpcError::StoreUnavailable)
    }
}

/// Normalise and check an allowlist: trimmed, de-duplicated, order preserved,
/// and every id known to a catalog.
///
/// The order matters because it is the order the chips were pressed in and the
/// order the editor shows back. The duplicate check matters because a session's
/// `mcpServers` is built from this list and the same server twice is a server
/// the harness may or may not accept.
fn checked_tools_json(tools: &[String]) -> Result<String, RpcError> {
    let mut kept: Vec<String> = Vec::with_capacity(tools.len());
    for tool in tools {
        let tool = tool.trim();
        if tool.is_empty() {
            continue;
        }
        if !is_known_tool(tool) {
            return Err(RpcError::InvalidParams(format!(
                "unknown tool: {tool}; a bot can only be allowed tools the host has"
            )));
        }
        if !kept.iter().any(|kept| kept == tool) {
            kept.push(tool.to_string());
        }
    }
    serde_json::to_string(&kept).map_err(|err| RpcError::Internal(err.to_string()))
}

/// Check an uploaded icon, and read `""` as "take the picture away".
///
/// Three things have to hold and each has cost something somewhere before: the
/// media type has to be one that is only ever pixels, the payload has to be
/// base64 and nothing else (a `data:` URL whose body is a URL is how a fetch
/// gets smuggled into an `<img>`), and the whole thing has to be small enough
/// that a crew is a database rather than a photo album.
///
/// The renderer checks all three before it sends, and this checks them again,
/// because "the UI would never send that" is not a property of a JSON-RPC host
/// — anything on the socket is a caller.
fn checked_image(image: &str) -> Result<Option<String>, RpcError> {
    if image.is_empty() {
        return Ok(None);
    }
    let Some(payload) = IMAGE_PREFIXES
        .iter()
        .find_map(|prefix| image.strip_prefix(prefix))
    else {
        return Err(RpcError::InvalidParams(format!(
            "an icon must be a base64 data URL of type {}",
            IMAGE_PREFIXES.join(", ")
        )));
    };
    if !is_base64(payload) {
        return Err(RpcError::InvalidParams(
            "an icon's data URL must be base64".into(),
        ));
    }
    let bytes = decoded_len(payload);
    if bytes > MAX_IMAGE_BYTES {
        return Err(RpcError::InvalidParams(format!(
            "an icon may be at most {} KiB, and this one is {} KiB",
            MAX_IMAGE_BYTES / 1024,
            bytes / 1024
        )));
    }
    Ok(Some(image.to_string()))
}

/// Standard base64, padded, and nothing else — no whitespace, no URL-safe
/// alphabet, no `=` anywhere but the end.
fn is_base64(payload: &str) -> bool {
    let body = payload.trim_end_matches('=');
    let padding = payload.len() - body.len();
    !body.is_empty()
        && padding <= 2
        && payload.len() % 4 == 0
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// What the base64 decodes to, without decoding it: four characters carry
/// three bytes, less one per `=` of padding.
fn decoded_len(payload: &str) -> usize {
    payload.len() / 4 * 3 - payload.bytes().filter(|&b| b == b'=').count()
}

fn host_tool_views() -> Vec<CrewHostToolView> {
    HOST_TOOLS
        .iter()
        .map(|tool| CrewHostToolView {
            id: tool.id.to_string(),
            label: tool.label.to_string(),
            blurb: tool.blurb.to_string(),
        })
        .collect()
}

fn display(path: PathBuf) -> String {
    path.display().to_string()
}

fn bot_not_found(bot_id: &str) -> RpcError {
    RpcError::InvalidParams(format!("no such bot: {bot_id}"))
}

fn store_error(err: StoreError) -> RpcError {
    match err {
        StoreError::Invalid(detail) => RpcError::InvalidParams(detail),
        other => internal(other),
    }
}

fn internal(err: StoreError) -> RpcError {
    RpcError::Internal(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::error::{CHIEF_REQUIRED, INVALID_PARAMS};
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use crate::host::protocol::{
        CREW_CREATE, CREW_LIST, CREW_REMOVE, CREW_UPDATE, HOST_HELLO, THREAD_OPEN, THREAD_STATE,
    };
    use serde_json::{json, Value};

    fn host() -> (HostSession, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut session = HostSession::load(&dir.path().join("data"));
        session
            .handle_request(req(1, HOST_HELLO, None))
            .result
            .expect("hello");
        (session, dir)
    }

    fn req(id: i64, method: &str, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest::new(RequestId::Number(id), method, params)
    }

    fn ok(session: &mut HostSession, method: &str, params: Value) -> Value {
        let response = session.handle_request(req(7, method, Some(params)));
        assert!(response.error.is_none(), "{method}: {:?}", response.error);
        response.result.expect("result")
    }

    fn err(session: &mut HostSession, method: &str, params: Value) -> crate::host::JsonRpcError {
        session
            .handle_request(req(8, method, Some(params)))
            .error
            .unwrap_or_else(|| panic!("{method} was expected to fail"))
    }

    fn bots(session: &mut HostSession) -> Vec<Value> {
        ok(session, CREW_LIST, json!({}))["bots"]
            .as_array()
            .expect("bots")
            .clone()
    }

    fn find<'a>(bots: &'a [Value], name: &str) -> &'a Value {
        bots.iter()
            .find(|bot| bot["name"] == name)
            .unwrap_or_else(|| panic!("no bot named {name}"))
    }

    #[test]
    fn the_shipped_crew_is_chief_plus_five_workers() {
        let (mut session, _dir) = host();
        let crew = bots(&mut session);

        let names: Vec<&str> = crew
            .iter()
            .map(|bot| bot["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "Chief",
                "Code",
                "Inbox Mgr",
                "Scheduler",
                "Research",
                "Writer"
            ]
        );
        // Chief first, and the only one of them.
        assert_eq!(crew[0]["isChief"], true);
        assert_eq!(crew.iter().filter(|bot| bot["isChief"] == true).count(), 1);
        // Every bot names an engine — after #6 that is part of who it is.
        assert!(crew.iter().all(|bot| bot["harnessId"] == "claude"));
    }

    #[test]
    fn every_bot_gets_its_own_memory_directory_with_its_persona_in_it() {
        let (mut session, _dir) = host();
        let crew = bots(&mut session);

        let mut dirs: Vec<&str> = crew
            .iter()
            .map(|bot| bot["memoryDir"].as_str().expect("memoryDir"))
            .collect();
        let count = dirs.len();
        dirs.sort_unstable();
        dirs.dedup();
        assert_eq!(dirs.len(), count, "two bots shared a memory directory");

        let writer = find(&crew, "Writer");
        let dir = std::path::Path::new(writer["memoryDir"].as_str().unwrap());
        let instructions = std::fs::read_to_string(dir.join(memory::INSTRUCTIONS_FILE)).unwrap();
        assert!(instructions.contains("Draft in my voice"));
        assert!(dir.join(memory::MEMORY_FILE).exists());
    }

    #[test]
    fn adding_from_a_template_copies_it_and_then_forgets_it() {
        let (mut session, _dir) = host();

        let expense = ok(
            &mut session,
            CREW_CREATE,
            json!({ "templateId": "expense" }),
        );
        assert_eq!(expense["name"], "Expense Manager");
        assert_eq!(expense["color"], "b-green");
        assert_eq!(expense["tools"], json!(["gmail", "drive"]));
        assert_eq!(expense["templateId"], "expense");
        assert_eq!(expense["isChief"], false);

        // Edit the instance...
        let bot_id = expense["botId"].as_str().unwrap().to_string();
        let edited = ok(
            &mut session,
            CREW_UPDATE,
            json!({ "botId": bot_id, "name": "Receipts", "tools": ["gmail"] }),
        );
        assert_eq!(edited["name"], "Receipts");
        assert_eq!(edited["tools"], json!(["gmail"]));
        // ...and the pack is untouched, so a second copy is the original.
        let again = ok(
            &mut session,
            CREW_CREATE,
            json!({ "templateId": "expense" }),
        );
        assert_eq!(again["name"], "Expense Manager");
        assert_eq!(again["tools"], json!(["gmail", "drive"]));
        assert_ne!(again["botId"], edited["botId"]);
        assert_ne!(again["memoryDir"], edited["memoryDir"]);

        let listed = ok(&mut session, CREW_LIST, json!({}));
        assert_eq!(listed["templates"].as_array().unwrap().len(), 4);
        assert_eq!(listed["templates"][0]["templateId"], "expense");
    }

    /// The editor sends the whole form, so a save has to be able to override
    /// every field the template would have supplied.
    #[test]
    fn an_explicit_field_beats_the_template_it_came_from() {
        let (mut session, _dir) = host();

        let bot = ok(
            &mut session,
            CREW_CREATE,
            json!({
                "templateId": "ops",
                "name": "Night Watch",
                "harnessId": "claude",
            }),
        );
        assert_eq!(bot["name"], "Night Watch");
        assert_eq!(bot["harnessId"], "claude");
        // Untouched fields still come from the pack.
        assert_eq!(bot["color"], "b-orange");
        assert_eq!(bot["tools"], json!(["terminal", "slack"]));
    }

    #[test]
    fn a_blank_bot_needs_a_name_and_a_real_harness() {
        let (mut session, _dir) = host();

        let nameless = err(&mut session, CREW_CREATE, json!({}));
        assert_eq!(nameless.code, INVALID_PARAMS);

        let bad_harness = err(
            &mut session,
            CREW_CREATE,
            json!({ "name": "Ghost", "harnessId": "not-installed" }),
        );
        assert_eq!(bad_harness.code, INVALID_PARAMS);
        assert!(bad_harness.message.contains("no such harness"));

        // Nothing was written by either attempt.
        assert_eq!(bots(&mut session).len(), 6);
    }

    /// A chip the host cannot resolve is a capability that silently never
    /// arrives in the session (#18). Refuse it at the write.
    #[test]
    fn a_tool_no_catalog_knows_is_refused_and_duplicates_collapse() {
        let (mut session, _dir) = host();

        let unknown = err(
            &mut session,
            CREW_CREATE,
            json!({ "name": "Curious", "harnessId": "claude", "tools": ["telepathy"] }),
        );
        assert_eq!(unknown.code, INVALID_PARAMS);
        assert!(unknown.message.contains("telepathy"), "{}", unknown.message);

        let bot = ok(
            &mut session,
            CREW_CREATE,
            json!({
                "name": "Curious",
                "harnessId": "claude",
                "tools": ["gmail", " gmail ", "", "browser"],
            }),
        );
        assert_eq!(bot["tools"], json!(["gmail", "browser"]));

        // Chief's host tools are not MCP, and they are still legal ids.
        let chief = find(&bots(&mut session), "Chief").clone();
        let kept = ok(
            &mut session,
            CREW_UPDATE,
            json!({ "botId": chief["botId"], "tools": ["handoff_to_bot", "fold_thread"] }),
        );
        assert_eq!(kept["tools"], json!(["handoff_to_bot", "fold_thread"]));
    }

    #[test]
    fn an_unrenderable_colour_is_refused() {
        let (mut session, _dir) = host();
        let bad = err(
            &mut session,
            CREW_CREATE,
            json!({ "name": "Beige", "harnessId": "claude", "color": "beige" }),
        );
        assert_eq!(bad.code, INVALID_PARAMS);
        assert!(bad.message.contains("b-teal"), "{}", bad.message);
    }

    /// A patch moves the columns it names and nothing else — the bug this
    /// guards is a harness change wiping the persona.
    #[test]
    fn a_partial_save_leaves_the_rest_of_the_record_alone() {
        let (mut session, _dir) = host();
        let writer = find(&bots(&mut session), "Writer").clone();

        let after = ok(
            &mut session,
            CREW_UPDATE,
            json!({ "botId": writer["botId"], "harnessId": "codex" }),
        );
        assert_eq!(after["harnessId"], "codex");
        assert_eq!(after["instructions"], writer["instructions"]);
        assert_eq!(after["tools"], writer["tools"]);
        assert_eq!(after["color"], writer["color"]);
        assert_eq!(after["createdAt"], writer["createdAt"]);
    }

    #[test]
    fn saving_a_persona_rewrites_instructions_md_and_never_memory_md() {
        let (mut session, _dir) = host();
        let writer = find(&bots(&mut session), "Writer").clone();
        let dir = std::path::PathBuf::from(writer["memoryDir"].as_str().unwrap());

        let learned = "# Writer — memory\n\nJabree hates exclamation marks.\n";
        std::fs::write(dir.join(memory::MEMORY_FILE), learned).unwrap();

        ok(
            &mut session,
            CREW_UPDATE,
            json!({ "botId": writer["botId"], "instructions": "Short and plain." }),
        );

        let instructions = std::fs::read_to_string(dir.join(memory::INSTRUCTIONS_FILE)).unwrap();
        assert!(instructions.contains("Short and plain."));
        assert_eq!(
            std::fs::read_to_string(dir.join(memory::MEMORY_FILE)).unwrap(),
            learned
        );
    }

    #[test]
    fn chief_can_be_edited_but_never_removed() {
        let (mut session, _dir) = host();
        let chief = find(&bots(&mut session), "Chief").clone();

        let edited = ok(
            &mut session,
            CREW_UPDATE,
            json!({ "botId": chief["botId"], "instructions": "Route work. Ask before spending." }),
        );
        assert_eq!(edited["instructions"], "Route work. Ask before spending.");
        assert_eq!(edited["isChief"], true);

        let refused = err(
            &mut session,
            CREW_REMOVE,
            json!({ "botId": chief["botId"] }),
        );
        assert_eq!(refused.code, CHIEF_REQUIRED);
        assert_eq!(refused.data.unwrap()["botId"], chief["botId"]);
        assert_eq!(
            bots(&mut session)
                .iter()
                .filter(|bot| bot["isChief"] == true)
                .count(),
            1
        );
    }

    /// Removing a bot must not remove the work it started: `threads.bot_id` is
    /// `ON DELETE SET NULL` and every thread carries its own cwd and harness.
    #[test]
    fn removing_a_bot_detaches_its_threads_and_keeps_its_notes() {
        let (mut session, _dir) = host();
        let research = find(&bots(&mut session), "Research").clone();
        let bot_id = research["botId"].as_str().unwrap().to_string();
        let dir = std::path::PathBuf::from(research["memoryDir"].as_str().unwrap());

        ok(
            &mut session,
            THREAD_OPEN,
            json!({
                "threadId": "t-brief",
                "title": "Brief me on ACP",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "harnessId": "claude",
                "botId": bot_id,
            }),
        );

        let removed = ok(&mut session, CREW_REMOVE, json!({ "botId": bot_id }));
        assert_eq!(removed["removed"], true);
        assert_eq!(removed["detachedThreads"], 1);
        assert_eq!(removed["memoryDir"], research["memoryDir"]);

        assert!(bots(&mut session)
            .iter()
            .all(|bot| bot["name"] != json!("Research")));

        // The thread is still there, still knows where it works.
        let thread = ok(&mut session, THREAD_STATE, json!({ "threadId": "t-brief" }));
        assert!(thread["botId"].is_null());
        assert_eq!(thread["title"], "Brief me on ACP");
        // And the notes are where the result said they are.
        assert!(dir.join(memory::MEMORY_FILE).exists());
    }

    #[test]
    fn removing_a_bot_that_is_not_there_says_so() {
        let (mut session, _dir) = host();
        let missing = err(&mut session, CREW_REMOVE, json!({ "botId": "nobody" }));
        assert_eq!(missing.code, INVALID_PARAMS);
        assert!(missing.message.contains("nobody"));
    }

    /// A real 1x1 PNG, base64 and padded — the smallest thing that is
    /// genuinely an icon rather than a string that looks like one.
    const PIXEL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

    #[test]
    fn a_bot_can_be_given_a_picture_replace_it_and_take_it_away() {
        let (mut session, _dir) = host();

        // Created with one: the field comes back on the record the editor is.
        let made = ok(
            &mut session,
            CREW_CREATE,
            json!({ "name": "Mira", "harnessId": "claude", "image": PIXEL }),
        );
        assert_eq!(made["image"], PIXEL);
        let bot_id = made["botId"].as_str().unwrap().to_string();

        // A patch that does not mention the icon leaves it alone. This is the
        // case the nested `Option` in `BotPatch` exists for: renaming a bot
        // used to be the only way to lose its picture.
        let renamed = ok(
            &mut session,
            CREW_UPDATE,
            json!({ "botId": bot_id, "name": "Mira Ops" }),
        );
        assert_eq!(renamed["name"], "Mira Ops");
        assert_eq!(renamed["image"], PIXEL);

        // The empty string is how the editor says "take it away", and what
        // comes back has no icon at all rather than an icon of nothing.
        let cleared = ok(
            &mut session,
            CREW_UPDATE,
            json!({ "botId": bot_id, "image": "" }),
        );
        assert!(cleared.get("image").is_none(), "{cleared}");

        // And it is gone from the store, not just from the answer.
        let listed = bots(&mut session);
        assert!(find(&listed, "Mira Ops").get("image").is_none());
    }

    #[test]
    fn a_bot_without_a_picture_simply_has_no_icon_field() {
        let (mut session, _dir) = host();
        // The shipped crew wears colours and initials, so nothing seeded
        // carries an icon — `image: null` on every row would be six lies the
        // renderer has to unpick.
        for bot in bots(&mut session) {
            assert!(bot.get("image").is_none(), "{bot}");
        }
    }

    #[test]
    fn an_icon_that_could_fetch_or_execute_is_refused() {
        let (mut session, _dir) = host();

        // Each of these is a way to get something that is not pixels into an
        // `<img src>` the renderer builds from a host row. The renderer checks
        // them too; this is the check that holds when the caller is not the
        // renderer.
        for bad in [
            "javascript:alert(1)",
            "http://example.com/avatar.png",
            "data:image/svg+xml;base64,PHN2Zy8+",
            "data:text/html;base64,PGI+",
            "data:image/png;base64,not base64!",
            "data:image/png;base64,abcde",
        ] {
            let refused = err(
                &mut session,
                CREW_CREATE,
                json!({ "name": "Bad", "harnessId": "claude", "image": bad }),
            );
            assert_eq!(refused.code, INVALID_PARAMS, "{bad} was accepted");
        }

        // Nothing was written on the way past any of them.
        assert!(bots(&mut session).iter().all(|bot| bot["name"] != "Bad"));
    }

    #[test]
    fn an_icon_bigger_than_the_cap_is_refused_with_its_size() {
        let (mut session, _dir) = host();
        // Valid base64 of one byte over the cap, so the only thing wrong with
        // it is that it is too big.
        let oversize = format!(
            "data:image/png;base64,{}",
            "A".repeat((MAX_IMAGE_BYTES + 1).div_ceil(3) * 4)
        );

        let refused = err(
            &mut session,
            CREW_CREATE,
            json!({ "name": "Huge", "harnessId": "claude", "image": oversize }),
        );
        assert_eq!(refused.code, INVALID_PARAMS);
        assert!(refused.message.contains("256 KiB"), "{}", refused.message);
    }

    #[test]
    fn the_cap_is_measured_in_decoded_bytes_not_characters() {
        // Base64 is a third bigger than what it carries, so measuring the
        // string would refuse icons a third smaller than the cap says.
        let at_cap = "A".repeat(MAX_IMAGE_BYTES.div_ceil(3) * 4);
        assert_eq!(decoded_len(&at_cap), MAX_IMAGE_BYTES.div_ceil(3) * 3);
        assert_eq!(decoded_len("AAAA"), 3);
        assert_eq!(decoded_len("AAA="), 2);
        assert_eq!(decoded_len("AA=="), 1);
    }

    #[test]
    fn the_host_tools_the_grid_needs_to_name_come_with_the_crew() {
        let (mut session, _dir) = host();
        let listed = ok(&mut session, CREW_LIST, json!({}));

        let host_tools = listed["hostTools"].as_array().unwrap();
        let handoff = host_tools
            .iter()
            .find(|tool| tool["id"] == "handoff_to_bot")
            .expect("handoff_to_bot");
        assert_eq!(handoff["label"], "Handoff");
        // Chief's seeded chips are exactly ids this list can name.
        let chief = find(listed["bots"].as_array().unwrap(), "Chief");
        for tool in chief["tools"].as_array().unwrap() {
            assert!(
                host_tools.iter().any(|known| known["id"] == *tool),
                "Chief carries {tool}, which nothing can label"
            );
        }
    }
}
