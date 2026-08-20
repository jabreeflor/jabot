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
//! - `grandchild`: spawn a `sleep` grandchild in the same process group
//! - `old-acp`: answer `initialize` with a protocol version older than the
//!   host speaks, so the Doctor's deep probe has a real outdated adapter

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
                if let Some(id) = pending_prompt_id.take() {
                    reply(
                        &mut stdout,
                        Some(id),
                        serde_json::json!({ "stopReason": "end_turn" }),
                    );
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
                    "agentCapabilities": { "loadSession": false },
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
