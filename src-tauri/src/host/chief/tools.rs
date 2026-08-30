//! What Chief's four host tools look like to a model, and how their arguments
//! are read (#24).
//!
//! The ids are not invented here. #17 compiled them into `crew::HOST_TOOLS` so
//! the crew grid could label Chief's chips instead of printing
//! `spawn_code_session` at the user, and said in the same breath that #24
//! implements them. This file is the other half of those four rows: the
//! description the model reads and the schema it fills in. The two lists are
//! held together by a test, because a chip the grid can name and the session
//! cannot call is worse than no chip at all.
//!
//! The descriptions carry the routing policy decision #6 settled — "Chief does
//! not call Gmail itself; it hands off to Inbox Mgr" — because that is a
//! sentence the model has to read at the moment it chooses, not one buried in
//! a persona file. And `handoff_to_bot` says out loud that the receiving bot
//! has no repository, since the whole reason `spawn_code_session` exists is
//! that a worker gets a checkout no other way.

use serde_json::{json, Value};

/// One tool as MCP describes it.
pub struct HostToolSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
}

pub const SPECS: &[HostToolSpec] = &[
    HostToolSpec {
        id: "handoff_to_bot",
        title: "Hand off to a crew member",
        description: "Give a job to another bot in the crew. The task is put on that bot's \
                      standing thread and it starts work immediately. Use this instead of \
                      doing a specialist's job yourself — hand mail to Inbox Mgr, calendars \
                      to Scheduler, drafts to Writer. The receiving bot works in its own \
                      memory directory and has no repository checkout; for anything that \
                      needs code, use spawn_code_session.",
        schema: handoff_schema,
    },
    HostToolSpec {
        id: "spawn_code_session",
        title: "Start a coding session",
        description: "Open a new coding thread in one of the user's registered folders. \
                      JaBot gives the thread its own git worktree and branch, so it never \
                      collides with the user's checkout or with another session. This is \
                      the only way a bot gets a repository to work in.",
        schema: spawn_schema,
    },
    HostToolSpec {
        id: "fold_thread",
        title: "Fold a thread away",
        description: "Put a long-running job to sleep. The thread disappears from the \
                      sidebar and keeps working; when it finishes, fails or needs the user, \
                      it comes back as an Inbox card. Fold anything that will take a while \
                      rather than making the user watch it.",
        schema: fold_schema,
    },
    HostToolSpec {
        id: "list_crew_status",
        title: "See what the crew is doing",
        description: "List every bot and what it is working on right now, including jobs \
                      that are folded away. Check this before handing off, so a bot that is \
                      already busy is not given a second job by mistake. Each thread carries \
                      `busy`: true while its work is queued, running, or waiting on the user. \
                      A bot's `idle` only means it has no threads at all, so read `busy` to \
                      tell a bot whose only job is asleep from one mid-run.",
        schema: no_args_schema,
    },
];

fn handoff_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "bot": {
                "type": "string",
                "description": "The crew member to hand the job to: its id or its name, \
                                as list_crew_status reports it."
            },
            "task": {
                "type": "string",
                "description": "What you want done, written as an instruction to that bot."
            },
            "context": {
                "type": "string",
                "description": "Anything the bot needs that it cannot see from its own \
                                thread — what the user actually said, a link, a decision \
                                already taken."
            }
        },
        "required": ["bot", "task"],
        "additionalProperties": false
    })
}

fn spawn_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "folder": {
                "type": "string",
                "description": "Which registered folder to work in: its id, its name, or \
                                its path."
            },
            "task": {
                "type": "string",
                "description": "What the coding session should do."
            },
            "title": {
                "type": "string",
                "description": "Short title for the thread. Defaults to the first line of \
                                the task."
            },
            "baseRef": {
                "type": "string",
                "description": "Branch, tag or commit the new worktree starts from. \
                                Defaults to the folder's default branch on origin — not \
                                the user's working copy."
            }
        },
        "required": ["folder", "task"],
        "additionalProperties": false
    })
}

fn fold_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "threadId": {
                "type": "string",
                "description": "The thread to fold. Defaults to this conversation."
            },
            "policy": {
                "type": "string",
                "enum": ["default", "wait_for_inbox"],
                "description": "wait_for_inbox lets JaBot answer read-only permission \
                                prompts on the user's behalf while the thread is asleep. \
                                Anything that writes, runs or deletes still wakes them."
            }
        },
        "additionalProperties": false
    })
}

fn no_args_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

/// The MCP `tools/list` entry for one spec.
pub fn describe(spec: &HostToolSpec) -> Value {
    json!({
        "name": spec.id,
        "title": spec.title,
        "description": spec.description,
        "inputSchema": (spec.schema)(),
    })
}

pub fn find(id: &str) -> Option<&'static HostToolSpec> {
    SPECS.iter().find(|spec| spec.id == id)
}

/// A required string argument, trimmed. The error text is what the model reads
/// and retries against, so it names the argument rather than the schema.
pub fn required(args: &Value, name: &str) -> Result<String, String> {
    match optional(args, name) {
        Some(value) => Ok(value),
        None => Err(format!("{name} is required")),
    }
}

pub fn optional(args: &Value, name: &str) -> Option<String> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::super::super::crew::HOST_TOOLS;
    use super::*;

    /// #17 ships the ids so the crew grid can label Chief's chips; this file
    /// ships the schemas so the session can call them. A chip the grid names
    /// and the session cannot call is a capability the user thinks they have.
    #[test]
    fn every_chip_the_crew_grid_names_is_a_tool_the_session_can_call() {
        let named: Vec<&str> = HOST_TOOLS.iter().map(|tool| tool.id).collect();
        let callable: Vec<&str> = SPECS.iter().map(|spec| spec.id).collect();
        assert_eq!(named, callable);
    }

    #[test]
    fn each_tool_describes_itself_well_enough_to_be_chosen() {
        for spec in SPECS {
            let described = describe(spec);
            assert_eq!(described["name"], spec.id);
            assert!(
                described["description"].as_str().unwrap().len() > 60,
                "{} has no description worth reading",
                spec.id
            );
            assert_eq!(described["inputSchema"]["type"], "object");
            // A model that invents an argument should be told, not silently
            // ignored — every schema is closed.
            assert_eq!(described["inputSchema"]["additionalProperties"], false);
        }
    }

    #[test]
    fn arguments_are_trimmed_and_blanks_are_missing_rather_than_empty() {
        let args = json!({ "task": "  draft it  ", "context": "   ", "bot": "writer" });
        assert_eq!(required(&args, "task").unwrap(), "draft it");
        assert_eq!(optional(&args, "context"), None);
        assert!(required(&args, "nope").unwrap_err().contains("nope"));
    }
}
