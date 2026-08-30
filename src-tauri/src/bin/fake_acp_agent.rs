//! Test-only ACP agent. Speaks ACP v1 JSON-RPC over newline-delimited stdio.
//!
//! Modes (first arg):
//! - `echo` (default): initialize, session/new, stream one agent chunk, return
//! - `permission`: request `session/request_permission` before completing
//! - `read-permission`: same, but a `read` tool call — the one kind Wait for
//!   Inbox is allowed to answer on the user's behalf
//! - `hang`: stream a chunk and then go silent forever, so the idle-timeout
//!   backstop has something real to fire on
//! - `fail`: return a non-`end_turn` stop reason
//! - `late-end`: stream a chunk, stay quiet long enough for the idle-timeout
//!   backstop to fire, and only then return `end_turn`
//! - `v2-idle`: report going idle with **no** stop reason before returning
//!   `end_turn` on the prompt response — the shape an ACP v2 adapter has, and
//!   the one where idleness alone must not be read as an outcome
//! - `tools`: stream a plan, a read tool call that completes, an edit with a
//!   diff, and a tool call whose `kind` is one nothing has ever heard of — the
//!   shapes #14's renderer has to map, including the one it must not die on
//! - `cancellable`: hold the turn open until `session/cancel`, then end it
//!   with `stopReason: cancelled` — what a real adapter does, and what an
//!   interrupt-then-redispatch depends on
//! - `grandchild`: spawn a `sleep` grandchild in the same process group
//! - `old-acp`: answer `initialize` with a protocol version older than the
//!   host speaks, so the Doctor's deep probe has a real outdated adapter
//! - `resumable`: advertise `sessionCapabilities.resume` + `close` and answer
//!   `session/resume` and `session/close` — an adapter the supervisor can hand
//!   a session back to instead of starting a new one (#21)
//! - `loadable`: advertise `loadSession` only, and answer `session/load` by
//!   replaying two messages the way a real agent replays its history
//! - `v2-cancel`: hold the turn open, and end it on `session/cancel` with an
//!   idle `state_update` carrying the stop reason and **no** prompt response —
//!   the ACP v2 completion shape, and the one where a turn ends without the
//!   response the v1 path hangs its bookkeeping on
//! - `orphan-stdout`: fork a grandchild that inherits stdout, then exit —
//!   a dead adapter whose stdout pipe never closes, so EOF never comes and
//!   only reaping the pid can tell the host the session is gone
//! - `gated`: hold the turn open until the test says what happens next, by
//!   writing the gate file named in the second argument. Fold is the one
//!   feature that cannot be proved against an agent that finishes on its own:
//!   the thread has to still be *running* at the moment it is folded, and then
//!   go on running, and only then end. A sleep would make that a race; a gate
//!   makes it an ordering. See [`wait_for_gate`] for the script it reads.
//! - `say`: stream the prompt back as an `agent_message_chunk` and end the
//!   turn — prose with no tool call and no URL. What an agent that only
//!   *claims* to have opened a pull request looks like, which is what arms the
//!   host's post-turn `gh` probe without proving anything (#28).

use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "echo".into());
    if mode == "grandchild" {
        let _ = Command::new("sleep")
            .arg("120")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut session_id: Option<String> = None;
    let mut pending_prompt_id: Option<serde_json::Value> = None;
    // What a `gated` turn still has to do. Empty for every other mode, which
    // is what keeps their behaviour exactly as it was.
    let mut steps: Vec<String> = Vec::new();
    let mut ask_seq: i64 = 9001;

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            eprintln!("fake-acp: bad json: {line}");
            continue;
        };

        if msg.get("method").is_none() {
            if pending_prompt_id.is_some() {
                eprintln!("permission_reply={msg}");
                notify(
                    &mut stdout,
                    "session/update",
                    serde_json::json!({
                        "sessionId": session_id,
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "type": "text", "text": "allowed" }
                    }),
                );
                // A gated script can ask for more than one thing in a turn —
                // the read Wait for Inbox answers by itself, and then the
                // execute it must not. Only the last step ends the turn.
                match next_step(&mut steps) {
                    Some(step) if is_tool_kind(&step) => {
                        ask_permission(&mut stdout, &session_id, &step, &mut ask_seq);
                    }
                    step => {
                        if let Some(id) = pending_prompt_id.take() {
                            reply(
                                &mut stdout,
                                Some(id),
                                serde_json::json!({
                                    "stopReason": step.unwrap_or_else(|| "end_turn".into())
                                }),
                            );
                        }
                    }
                }
            }
            continue;
        }

        let method = msg["method"].as_str().unwrap_or("");
        let id = msg.get("id").cloned();
        match method {
            "initialize" => reply(
                &mut stdout,
                id,
                serde_json::json!({
                    "protocolVersion": if mode == "old-acp" { 0 } else { 1 },
                    "agentCapabilities": {
                        "loadSession": mode == "loadable",
                        "sessionCapabilities": {
                            "resume": mode == "resumable",
                            "close": mode == "resumable"
                        }
                    },
                    "agentInfo": { "name": "fake-acp-agent", "version": "0.0.0" },
                    "authMethods": []
                }),
            ),
            "session/new" => {
                // Echo the params the host sent. The host decides which MCP
                // servers a session sees (#18), and the only honest place to
                // check that from a test is the agent's side of the wire.
                eprintln!("session_new={}", msg["params"]);
                session_id = Some("sess-fake-1".into());
                reply(
                    &mut stdout,
                    id,
                    serde_json::json!({ "sessionId": "sess-fake-1" }),
                );
            }
            "session/resume" => {
                // The host must send back the session it stored, the same
                // absolute cwd, and the same MCP list; a test reads this line.
                eprintln!("session_resume={}", msg["params"]);
                session_id = msg["params"]["sessionId"].as_str().map(str::to_string);
                reply(&mut stdout, id, serde_json::json!({}));
            }
            "session/load" => {
                eprintln!("session_load={}", msg["params"]);
                session_id = msg["params"]["sessionId"].as_str().map(str::to_string);
                // A load replays history *before* the response lands. That
                // ordering is the whole reason the host has to decide what to
                // do with the replay rather than just persisting it.
                for text in ["replayed one", "replayed two"] {
                    notify(
                        &mut stdout,
                        "session/update",
                        serde_json::json!({
                            "sessionId": session_id,
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "type": "text", "text": text }
                        }),
                    );
                }
                reply(&mut stdout, id, serde_json::json!({}));
            }
            "session/close" => {
                eprintln!("session_close={}", msg["params"]);
                reply(&mut stdout, id, serde_json::json!({}));
            }
            "session/prompt" => {
                notify(
                    &mut stdout,
                    "session/update",
                    serde_json::json!({
                        "sessionId": session_id,
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "type": "text", "text": "hello from fake-acp" }
                    }),
                );
                match mode.as_str() {
                    // Prose, and nothing else: the agent *says* it opened a
                    // pull request and never prints a URL. `pr-linkage.md` §4
                    // is explicit that this proves nothing — it only raises
                    // the flag that sends the host to ask `gh` at turn end —
                    // so it is the only way to reach rungs 2 and 3 from a real
                    // turn rather than from a unit test's synthetic link.
                    "say" => {
                        let said = prompt_text(&msg["params"]["prompt"]);
                        notify(
                            &mut stdout,
                            "session/update",
                            serde_json::json!({
                                "sessionId": session_id,
                                "sessionUpdate": "agent_message_chunk",
                                "content": { "type": "text", "text": said }
                            }),
                        );
                        reply(
                            &mut stdout,
                            id,
                            serde_json::json!({ "stopReason": "end_turn" }),
                        );
                    }
                    // A shell call whose stdout is whatever the user typed.
                    // `gh pr create` prints the new PR's URL and nothing else,
                    // so a test can hand this agent that one line and exercise
                    // the real linkage path end to end (#28).
                    "execute" => {
                        let said = prompt_text(&msg["params"]["prompt"]);
                        notify(
                            &mut stdout,
                            "session/update",
                            serde_json::json!({
                                "sessionId": session_id,
                                "sessionUpdate": "tool_call",
                                "toolCallId": "call-shell",
                                "title": "gh pr create --fill",
                                "kind": "execute",
                                "status": "in_progress"
                            }),
                        );
                        notify(
                            &mut stdout,
                            "session/update",
                            serde_json::json!({
                                "sessionId": session_id,
                                "sessionUpdate": "tool_call_update",
                                "toolCallId": "call-shell",
                                "status": "completed",
                                "content": [{
                                    "type": "content",
                                    "content": { "type": "text", "text": said }
                                }]
                            }),
                        );
                        reply(
                            &mut stdout,
                            id,
                            serde_json::json!({ "stopReason": "end_turn" }),
                        );
                    }
                    // Die with the pipe still open: the grandchild inherits
                    // stdout, so the host's reader never sees EOF and only a
                    // waitpid can notice the adapter is a corpse.
                    "orphan-stdout" => {
                        let _ = Command::new("sleep")
                            .arg("120")
                            .stdin(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn();
                        std::process::exit(0);
                    }
                    // One turn carrying every shape the chat has to draw.
                    "tools" => {
                        notify(
                            &mut stdout,
                            "session/update",
                            serde_json::json!({
                                "sessionId": session_id,
                                "sessionUpdate": "plan",
                                "entries": [
                                    { "content": "Read the module", "priority": "high", "status": "completed" },
                                    { "content": "Patch the guard", "priority": "high", "status": "in_progress" },
                                    { "content": "Run the tests", "priority": "medium", "status": "pending" }
                                ]
                            }),
                        );
                        notify(
                            &mut stdout,
                            "session/update",
                            serde_json::json!({
                                "sessionId": session_id,
                                "sessionUpdate": "tool_call",
                                "toolCallId": "call-read",
                                "title": "src/auth.ts",
                                "kind": "read",
                                "status": "pending"
                            }),
                        );
                        notify(
                            &mut stdout,
                            "session/update",
                            serde_json::json!({
                                "sessionId": session_id,
                                "sessionUpdate": "tool_call_update",
                                "toolCallId": "call-read",
                                "status": "completed"
                            }),
                        );
                        notify(
                            &mut stdout,
                            "session/update",
                            serde_json::json!({
                                "sessionId": session_id,
                                "sessionUpdate": "tool_call",
                                "toolCallId": "call-edit",
                                "title": "src/auth.ts",
                                "kind": "edit",
                                "status": "completed",
                                "content": [{
                                    "type": "diff",
                                    "path": "/repo/src/auth.ts",
                                    "oldText": "const a = 1;\nconst gone = 2;\n",
                                    "newText": "const a = 1;\nconst added = 3;\nconst also = 4;\n"
                                }]
                            }),
                        );
                        // A kind no client has a verb for. The transcript has
                        // to keep rendering (#11's review found this class of
                        // bug once already).
                        notify(
                            &mut stdout,
                            "session/update",
                            serde_json::json!({
                                "sessionId": session_id,
                                "sessionUpdate": "tool_call",
                                "toolCallId": "call-strange",
                                "title": "summon",
                                "kind": "sorcery",
                                "status": "in_progress"
                            }),
                        );
                        reply(
                            &mut stdout,
                            id,
                            serde_json::json!({ "stopReason": "end_turn" }),
                        );
                    }
                    "permission" | "read-permission" => {
                        let (title, kind) = if mode == "read-permission" {
                            ("Read src/auth.ts", "read")
                        } else {
                            ("Run ls", "execute")
                        };
                        request(
                            &mut stdout,
                            serde_json::json!(9001),
                            "session/request_permission",
                            serde_json::json!({
                                "sessionId": session_id,
                                "toolCall": {
                                    "toolCallId": "call-1",
                                    "title": title,
                                    "kind": kind
                                },
                                "options": [
                                    { "optionId": "allow_once", "name": "Allow", "kind": "allow_once" },
                                    { "optionId": "reject_once", "name": "Deny", "kind": "reject_once" }
                                ]
                            }),
                        );
                        pending_prompt_id = id;
                    }
                    // Never answers. The turn stays open and the host has to
                    // notice the silence on its own.
                    "hang" => {}
                    // The turn is genuinely in flight until the test opens the
                    // gate — which is what lets a test fold a *running*
                    // session and then watch it keep running.
                    "gated" => {
                        steps = wait_for_gate(std::env::args().nth(2).as_deref());
                        match next_step(&mut steps) {
                            Some(step) if is_tool_kind(&step) => {
                                ask_permission(&mut stdout, &session_id, &step, &mut ask_seq);
                                pending_prompt_id = id;
                            }
                            step => reply(
                                &mut stdout,
                                id,
                                serde_json::json!({
                                    "stopReason": step.unwrap_or_else(|| "end_turn".into())
                                }),
                            ),
                        }
                    }
                    // Holds the turn open, but ends it when told to.
                    "cancellable" | "v2-cancel" => pending_prompt_id = id,
                    // Quiet long enough to be called stuck, then finishes
                    // anyway — the agent was slow, not wedged.
                    "late-end" => {
                        std::thread::sleep(std::time::Duration::from_millis(400));
                        reply(
                            &mut stdout,
                            id,
                            serde_json::json!({ "stopReason": "end_turn" }),
                        );
                    }
                    "v2-idle" => {
                        notify(
                            &mut stdout,
                            "session/update",
                            serde_json::json!({
                                "sessionId": session_id,
                                "sessionUpdate": "state_update",
                                "sessionState": "idle"
                            }),
                        );
                        reply(
                            &mut stdout,
                            id,
                            serde_json::json!({ "stopReason": "end_turn" }),
                        );
                    }
                    "fail" => reply(
                        &mut stdout,
                        id,
                        serde_json::json!({ "stopReason": "max_tokens" }),
                    ),
                    _ => reply(
                        &mut stdout,
                        id,
                        serde_json::json!({ "stopReason": "end_turn" }),
                    ),
                }
            }
            "session/cancel" => {
                eprintln!("cancelled");
                if mode == "v2-cancel" {
                    // v2 reports completion as a state change, not as the
                    // prompt response. The response never comes.
                    pending_prompt_id = None;
                    notify(
                        &mut stdout,
                        "session/update",
                        serde_json::json!({
                            "sessionId": session_id,
                            "sessionUpdate": "state_update",
                            "sessionState": "idle",
                            "stopReason": "cancelled"
                        }),
                    );
                }
                if mode == "cancellable" {
                    if let Some(id) = pending_prompt_id.take() {
                        reply(
                            &mut stdout,
                            Some(id),
                            serde_json::json!({ "stopReason": "cancelled" }),
                        );
                    }
                }
            }
            _ => {
                if let Some(id) = id {
                    let err = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": format!("Method not found: {method}") }
                    });
                    writeln!(stdout, "{err}").ok();
                    stdout.flush().ok();
                }
            }
        }
    }
}

/// The text blocks of an ACP prompt, joined. The `execute` mode echoes them as
/// shell output, so a test can decide exactly what the agent "printed".
fn prompt_text(prompt: &serde_json::Value) -> String {
    match prompt {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| {
                block.as_str().map(str::to_string).or_else(|| {
                    block
                        .get("text")
                        .and_then(|t| t.as_str())
                        .map(str::to_string)
                })
            })
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

fn reply(stdout: &mut io::Stdout, id: Option<serde_json::Value>, result: serde_json::Value) {
    let Some(id) = id else { return };
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    });
    writeln!(stdout, "{msg}").ok();
    stdout.flush().ok();
}

fn notify(stdout: &mut io::Stdout, method: &str, params: serde_json::Value) {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    });
    writeln!(stdout, "{msg}").ok();
    stdout.flush().ok();
}

/// Block until the gate file exists, then read the turn's script out of it.
///
/// The file's contents are a comma-separated list: an ACP tool `kind`
/// (`read`, `execute`, `delete`) asks for that permission and waits; anything
/// else is the stop reason the turn ends with. An empty file means `end_turn`.
///
/// A gate that never opens replies `gate_timeout`, which the host classifies
/// as a failure — a test that forgot to open its gate should fail loudly
/// rather than hang until the suite's own timeout.
fn wait_for_gate(path: Option<&str>) -> Vec<String> {
    let Some(path) = path else {
        return vec!["gate_timeout".into()];
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        // Written whole by `rename`, so a read that sees the file sees all of
        // it; a partial read would turn into a stop reason nobody wrote.
        if let Ok(body) = std::fs::read_to_string(path) {
            let steps: Vec<String> = body
                .split(',')
                .map(|step| step.trim().to_string())
                .filter(|step| !step.is_empty())
                .collect();
            return steps;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    eprintln!("fake-acp: gate {path} never opened");
    vec!["gate_timeout".into()]
}

fn next_step(steps: &mut Vec<String>) -> Option<String> {
    if steps.is_empty() {
        None
    } else {
        Some(steps.remove(0))
    }
}

/// The three ACP tool kinds a gated script can ask permission for. Everything
/// else in a script is a stop reason.
fn is_tool_kind(step: &str) -> bool {
    matches!(step, "read" | "execute" | "delete")
}

/// One `session/request_permission`, with a fresh id so a turn can ask twice.
fn ask_permission(
    stdout: &mut io::Stdout,
    session_id: &Option<String>,
    kind: &str,
    ask_seq: &mut i64,
) {
    let title = match kind {
        "read" => "Read src/auth.ts",
        "delete" => "Delete src/legacy.ts",
        _ => "Run ls",
    };
    let id = *ask_seq;
    *ask_seq += 1;
    request(
        stdout,
        serde_json::json!(id),
        "session/request_permission",
        serde_json::json!({
            "sessionId": session_id,
            "toolCall": {
                "toolCallId": format!("call-{id}"),
                "title": title,
                "kind": kind
            },
            "options": [
                { "optionId": "allow_once", "name": "Allow", "kind": "allow_once" },
                { "optionId": "reject_once", "name": "Deny", "kind": "reject_once" }
            ]
        }),
    );
}

fn request(
    stdout: &mut io::Stdout,
    id: serde_json::Value,
    method: &str,
    params: serde_json::Value,
) {
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    });
    writeln!(stdout, "{msg}").ok();
    stdout.flush().ok();
}
