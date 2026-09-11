//! JaBot-owned ACP adapter that drives Aider's scripting CLI (#223).
//!
//! This process speaks ACP v1 over newline-delimited stdio. It does **not**
//! claim that Aider itself speaks ACP. Each `session/prompt` becomes one
//! `aider --message-file … --yes --no-auto-commits` invocation in the
//! session cwd (the JaBot worktree). Multi-turn context is Aider's
//! `--chat-history-file` + `--restore-chat-history`, not a native ACP
//! session store.
//!
//! Unsupported, and declared in `_meta.jabot`:
//! - `session/request_permission` — Aider's confirmations are TTY; `--yes`
//!   auto-accepts so the turn cannot hang on stdin.
//! - Native ACP from Aider.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use super::super::procgroup;
use super::aider::{scripting_args, ENV_FLOOR};

const AGENT_NAME: &str = "jabot-aider-acp";
const AGENT_TITLE: &str = "Aider (JaBot scripting adapter)";

#[derive(Debug)]
pub struct AiderInvocation {
    pub cwd: PathBuf,
    pub message: String,
    pub files: Vec<String>,
    pub history_file: PathBuf,
}

pub trait AiderChild: Send {
    fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>>;
    /// Non-blocking. `None` means the process is still running so a
    /// `session/cancel` can take the same lock and kill it.
    fn try_wait(&mut self) -> Option<Result<(i32, String), String>>;
    fn kill(&mut self);
}

pub trait AiderExec: Send + Sync + 'static {
    fn start(&self, spec: AiderInvocation) -> Result<Box<dyn AiderChild>, String>;
}

/// The production runner: `aider` on the augmented PATH.
#[derive(Clone, Copy, Default)]
pub struct RealAider;

struct RealChild {
    child: procgroup::GroupedChild,
    stderr: Option<thread::JoinHandle<String>>,
}

impl AiderChild for RealChild {
    fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>> {
        self.child
            .stdout
            .take()
            .map(|stdout| Box::new(stdout) as Box<dyn Read + Send>)
    }

    fn try_wait(&mut self) -> Option<Result<(i32, String), String>> {
        match self.child.try_wait() {
            Ok(Some(status)) => {
                let stderr = self
                    .stderr
                    .take()
                    .and_then(|handle| handle.join().ok())
                    .unwrap_or_default();
                Some(Ok((status.code().unwrap_or(-1), stderr)))
            }
            Ok(None) => None,
            Err(err) => Some(Err(err.to_string())),
        }
    }

    fn kill(&mut self) {
        procgroup::terminate(&mut self.child);
    }
}

impl AiderExec for RealAider {
    fn start(&self, spec: AiderInvocation) -> Result<Box<dyn AiderChild>, String> {
        let aider = super::resolve_command("aider").ok_or_else(|| {
            "aider is not on PATH. Install it with `python -m pip install aider-chat`.".to_string()
        })?;
        let session_dir = spec
            .history_file
            .parent()
            .ok_or_else(|| "chat history path has no parent".to_string())?;
        std::fs::create_dir_all(session_dir).map_err(|err| err.to_string())?;
        let message_file = session_dir.join("message.txt");
        std::fs::write(&message_file, spec.message.as_bytes()).map_err(|err| err.to_string())?;

        let args = scripting_args(
            &message_file.display().to_string(),
            &spec.history_file.display().to_string(),
            &spec.files,
        );
        let mut cmd = Command::new(aider);
        cmd.args(&args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PATH", super::path::joined());
        for (key, value) in ENV_FLOOR {
            cmd.env(key, value);
        }
        let mut child =
            procgroup::spawn(&mut cmd).map_err(|err| format!("failed to spawn aider: {err}"))?;
        let stderr_pipe = child.stderr.take();
        let stderr = Some(thread::spawn(move || {
            let mut buf = String::new();
            if let Some(mut pipe) = stderr_pipe {
                let _ = pipe.read_to_string(&mut buf);
            }
            buf
        }));
        Ok(Box::new(RealChild { child, stderr }))
    }
}

#[derive(Clone)]
struct Session {
    cwd: PathBuf,
    history: PathBuf,
}

struct Turn {
    id: Value,
    session_id: String,
    child: Mutex<Option<Box<dyn AiderChild>>>,
    done: AtomicBool,
}

/// stdio entry used by `jabot --aider-acp` and `jabot-hostd --aider-acp`.
pub fn run() -> i32 {
    match serve(std::io::stdin().lock(), std::io::stdout(), RealAider) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{AGENT_NAME}: {err}");
            1
        }
    }
}

pub fn serve<R, W, E>(stdin: R, stdout: W, exec: E) -> Result<(), String>
where
    R: Read,
    W: Write + Send + 'static,
    E: AiderExec,
{
    let stdout = Arc::new(Mutex::new(stdout));
    let exec = Arc::new(exec);
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut turn: Option<Arc<Turn>> = None;
    let mut minted: u32 = 0;

    for line in BufReader::new(stdin).lines() {
        let line = line.map_err(|err| err.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(msg) => msg,
            Err(err) => {
                eprintln!("{AGENT_NAME}: bad json: {err}");
                continue;
            }
        };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned();
        match method {
            "initialize" => {
                write_result(&stdout, id, initialize_result());
            }
            "session/new" => {
                minted += 1;
                let session_id = format!("aider-{minted}");
                let cwd = msg
                    .get("params")
                    .and_then(|p| p.get("cwd"))
                    .and_then(Value::as_str)
                    .unwrap_or(".")
                    .to_string();
                let history = history_path(&session_id);
                sessions.insert(
                    session_id.clone(),
                    Session {
                        cwd: PathBuf::from(cwd),
                        history,
                    },
                );
                write_result(&stdout, id, json!({ "sessionId": session_id }));
            }
            "session/resume" => {
                let session_id = msg
                    .get("params")
                    .and_then(|p| p.get("sessionId"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let cwd = msg
                    .get("params")
                    .and_then(|p| p.get("cwd"))
                    .and_then(Value::as_str)
                    .unwrap_or(".");
                let history = history_path(&session_id);
                sessions.insert(
                    session_id,
                    Session {
                        cwd: PathBuf::from(cwd),
                        history,
                    },
                );
                write_result(&stdout, id, json!({}));
            }
            "session/load" => {
                let session_id = msg
                    .get("params")
                    .and_then(|p| p.get("sessionId"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let cwd = msg
                    .get("params")
                    .and_then(|p| p.get("cwd"))
                    .and_then(Value::as_str)
                    .unwrap_or(".");
                let history = history_path(&session_id);
                if let Ok(text) = std::fs::read_to_string(&history) {
                    let replay = text.trim();
                    if !replay.is_empty() {
                        write_notify(
                            &stdout,
                            "session/update",
                            json!({
                                "sessionId": session_id,
                                "sessionUpdate": "agent_message_chunk",
                                "content": { "type": "text", "text": replay }
                            }),
                        );
                    }
                }
                sessions.insert(
                    session_id,
                    Session {
                        cwd: PathBuf::from(cwd),
                        history,
                    },
                );
                write_result(&stdout, id, json!({}));
            }
            "session/close" => {
                if let Some(session_id) = msg
                    .get("params")
                    .and_then(|p| p.get("sessionId"))
                    .and_then(Value::as_str)
                {
                    sessions.remove(session_id);
                }
                write_result(&stdout, id, json!({}));
            }
            "session/prompt" => {
                let Some(id) = id else { continue };
                let session_id = msg
                    .get("params")
                    .and_then(|p| p.get("sessionId"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let Some(session) = sessions.get(&session_id).cloned() else {
                    write_error(
                        &stdout,
                        Some(id),
                        -32000,
                        &format!("unknown session {session_id}"),
                    );
                    continue;
                };
                let prompt = msg.get("params").and_then(|p| p.get("prompt"));
                let text = prompt_text(prompt);
                let files = prompt_files(prompt);
                match exec.start(AiderInvocation {
                    cwd: session.cwd.clone(),
                    message: text,
                    files,
                    history_file: session.history.clone(),
                }) {
                    Ok(mut child) => {
                        let current = Arc::new(Turn {
                            id: id.clone(),
                            session_id: session_id.clone(),
                            child: Mutex::new(None),
                            done: AtomicBool::new(false),
                        });
                        let stdout_pipe = child.take_stdout();
                        if let Ok(mut slot) = current.child.lock() {
                            *slot = Some(child);
                        }
                        turn = Some(Arc::clone(&current));
                        start_turn(Arc::clone(&current), Arc::clone(&stdout), stdout_pipe);
                    }
                    Err(err) => {
                        fail_turn(&stdout, &session_id, id, &err);
                    }
                }
            }
            "session/cancel" => {
                if let Some(current) = turn.take() {
                    if let Ok(mut slot) = current.child.lock() {
                        if let Some(child) = slot.as_mut() {
                            child.kill();
                        }
                    }
                    finish_turn(&stdout, &current, "cancelled");
                }
            }
            "" => {}
            other => {
                if let Some(id) = id {
                    write_error(
                        &stdout,
                        Some(id),
                        -32601,
                        &format!("Method not found: {other}"),
                    );
                }
            }
        }
    }
    Ok(())
}

fn start_turn(
    turn: Arc<Turn>,
    stdout: Arc<Mutex<impl Write + Send + 'static>>,
    pipe: Option<Box<dyn Read + Send>>,
) {
    thread::spawn(move || {
        write_notify(
            &stdout,
            "session/update",
            json!({
                "sessionId": turn.session_id,
                "sessionUpdate": "tool_call",
                "toolCallId": "aider-run",
                "title": "aider --message",
                "kind": "execute",
                "status": "in_progress"
            }),
        );
        if let Some(pipe) = pipe {
            stream_stdout(&turn.session_id, pipe, &stdout);
        }
        let (code, stderr) = loop {
            if turn.done.load(Ordering::SeqCst) {
                break (-1, String::new());
            }
            let polled = {
                let mut slot = match turn.child.lock() {
                    Ok(slot) => slot,
                    Err(_) => return,
                };
                match slot.as_mut() {
                    Some(child) => child.try_wait(),
                    None => Some(Ok((-1, String::new()))),
                }
            };
            match polled {
                Some(Ok(exit)) => break exit,
                Some(Err(_)) => break (-1, String::new()),
                None => thread::sleep(Duration::from_millis(15)),
            }
        };
        let (tool_status, stop) = if turn.done.load(Ordering::SeqCst) {
            ("cancelled", "cancelled")
        } else if code == 0 {
            ("completed", "end_turn")
        } else {
            let mut text = format!("Aider exited {code}.");
            let stderr = stderr.trim();
            if !stderr.is_empty() {
                text.push('\n');
                text.push_str(stderr);
            }
            write_notify(
                &stdout,
                "session/update",
                json!({
                    "sessionId": turn.session_id,
                    "sessionUpdate": "agent_message_chunk",
                    "content": {
                        "type": "text",
                        "text": text
                    }
                }),
            );
            ("failed", "error")
        };
        write_notify(
            &stdout,
            "session/update",
            json!({
                "sessionId": turn.session_id,
                "sessionUpdate": "tool_call_update",
                "toolCallId": "aider-run",
                "status": tool_status
            }),
        );
        finish_turn(&stdout, &turn, stop);
    });
}

fn stream_stdout(session_id: &str, pipe: Box<dyn Read + Send>, stdout: &Mutex<impl Write>) {
    let reader = BufReader::new(pipe);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let text = strip_ansi(&line);
        if text.trim().is_empty() {
            continue;
        }
        write_notify(
            stdout,
            "session/update",
            json!({
                "sessionId": session_id,
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": format!("{text}\n") }
            }),
        );
    }
}

fn fail_turn(stdout: &Mutex<impl Write>, session_id: &str, id: Value, err: &str) {
    write_notify(
        stdout,
        "session/update",
        json!({
            "sessionId": session_id,
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": err }
        }),
    );
    write_result(stdout, Some(id), json!({ "stopReason": "error" }));
}

fn finish_turn(stdout: &Mutex<impl Write>, turn: &Turn, stop: &str) {
    if turn.done.swap(true, Ordering::SeqCst) {
        return;
    }
    write_result(stdout, Some(turn.id.clone()), json!({ "stopReason": stop }));
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": 1,
        "agentCapabilities": {
            "loadSession": true,
            "promptCapabilities": {
                "image": false,
                "audio": false,
                "embeddedContext": false
            },
            "sessionCapabilities": {
                "resume": true,
                "close": true
            }
        },
        "agentInfo": {
            "name": AGENT_NAME,
            "title": AGENT_TITLE,
            "version": env!("CARGO_PKG_VERSION")
        },
        "authMethods": [],
        "_meta": {
            "jabot": {
                "nativeAcp": false,
                "transport": "aider-cli-scripting",
                "autoCommits": false,
                "unsupported": [
                    "session/request_permission",
                    "interactive_approvals",
                    "native_aider_acp"
                ]
            }
        }
    })
}

fn history_path(session_id: &str) -> PathBuf {
    std::env::temp_dir()
        .join("jabot-aider")
        .join(session_id)
        .join("chat.history.md")
}

fn prompt_text(prompt: Option<&Value>) -> String {
    let Some(prompt) = prompt else {
        return String::new();
    };
    if let Some(text) = prompt.as_str() {
        return text.to_string();
    }
    let Some(blocks) = prompt.as_array() else {
        return prompt
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
    };
    blocks
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str) {
            Some("text") | None => block.get("text").and_then(Value::as_str),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn prompt_files(prompt: Option<&Value>) -> Vec<String> {
    let mut files = Vec::new();
    let Some(prompt) = prompt else {
        return files;
    };
    if let Some(blocks) = prompt.as_array() {
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("resource_link") => {
                    if let Some(uri) = block.get("uri").and_then(Value::as_str) {
                        files.push(uri_to_path(uri));
                    }
                }
                Some("resource") => {
                    if let Some(uri) = block
                        .get("resource")
                        .and_then(|r| r.get("uri"))
                        .and_then(Value::as_str)
                    {
                        files.push(uri_to_path(uri));
                    }
                }
                Some("text") | None => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        files.extend(mention_paths(text));
                    }
                }
                _ => {}
            }
        }
    } else if let Some(text) = prompt.as_str() {
        files.extend(mention_paths(text));
    }
    files.sort();
    files.dedup();
    files
}

fn uri_to_path(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
}

fn mention_paths(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|token| {
            let token = token.trim_matches(|c: char| ".,;:()[]{}\"'`".contains(c));
            let path = token.strip_prefix('@').unwrap_or(token);
            if path.contains('/') || looks_like_file(path) {
                Some(path.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn looks_like_file(token: &str) -> bool {
    Path::new(token)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext,
                "rs" | "ts"
                    | "tsx"
                    | "js"
                    | "jsx"
                    | "py"
                    | "md"
                    | "go"
                    | "json"
                    | "toml"
                    | "css"
                    | "html"
            )
        })
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn write_result(stdout: &Mutex<impl Write>, id: Option<Value>, result: Value) {
    let Some(id) = id else { return };
    write_line(
        stdout,
        json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    );
}

fn write_error(stdout: &Mutex<impl Write>, id: Option<Value>, code: i64, message: &str) {
    write_line(
        stdout,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message }
        }),
    );
}

fn write_notify(stdout: &Mutex<impl Write>, method: &str, params: Value) {
    write_line(
        stdout,
        json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }),
    );
}

fn write_line(stdout: &Mutex<impl Write>, value: Value) {
    let Ok(mut out) = stdout.lock() else { return };
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::io::Cursor;
    #[cfg(unix)]
    use std::sync::mpsc;
    #[cfg(unix)]
    use std::time::Instant;

    #[cfg(unix)]
    #[derive(Clone, Copy)]
    enum Mode {
        Success,
        AuthFail,
        Empty,
        Hang,
        CliFail,
    }

    #[cfg(unix)]
    #[derive(Clone)]
    struct FakeExec {
        mode: Mode,
        started: Arc<Mutex<Vec<AiderInvocation>>>,
    }

    #[cfg(unix)]
    struct FakeChild {
        mode: Mode,
        stdout: Option<Cursor<Vec<u8>>>,
        killed: Arc<AtomicBool>,
    }

    #[cfg(unix)]
    impl AiderChild for FakeChild {
        fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>> {
            self.stdout
                .take()
                .map(|cursor| Box::new(cursor) as Box<dyn Read + Send>)
        }

        fn try_wait(&mut self) -> Option<Result<(i32, String), String>> {
            match self.mode {
                Mode::Hang => {
                    if self.killed.load(Ordering::SeqCst) {
                        Some(Ok((9, String::new())))
                    } else {
                        None
                    }
                }
                Mode::AuthFail | Mode::CliFail => Some(Ok((1, "No API key provided.".into()))),
                Mode::Success | Mode::Empty => Some(Ok((0, String::new()))),
            }
        }

        fn kill(&mut self) {
            self.killed.store(true, Ordering::SeqCst);
        }
    }

    #[cfg(unix)]
    impl AiderExec for FakeExec {
        fn start(&self, spec: AiderInvocation) -> Result<Box<dyn AiderChild>, String> {
            self.started.lock().unwrap().push(AiderInvocation {
                cwd: spec.cwd.clone(),
                message: spec.message.clone(),
                files: spec.files.clone(),
                history_file: spec.history_file.clone(),
            });
            match self.mode {
                Mode::AuthFail => {
                    Err("No API key provided. Set OPENAI_API_KEY or ANTHROPIC_API_KEY.".into())
                }
                other => {
                    let body = match other {
                        Mode::Success => b"hello from aider\n".to_vec(),
                        Mode::Empty | Mode::Hang | Mode::CliFail => Vec::new(),
                        Mode::AuthFail => unreachable!(),
                    };
                    Ok(Box::new(FakeChild {
                        mode: other,
                        stdout: Some(Cursor::new(body)),
                        killed: Arc::new(AtomicBool::new(false)),
                    }))
                }
            }
        }
    }

    #[cfg(unix)]
    fn drive(exec: FakeExec, messages: &[Value], extra: Option<Value>) -> Vec<Value> {
        use std::os::unix::net::UnixStream;
        let (adapter, client) = UnixStream::pair().unwrap();
        let adapter_read = adapter.try_clone().unwrap();
        let reader = client.try_clone().unwrap();
        thread::spawn(move || {
            let _ = serve(BufReader::new(adapter_read), adapter, exec);
        });
        let mut client = client;
        for msg in messages {
            writeln!(client, "{msg}").unwrap();
            client.flush().unwrap();
        }
        if let Some(msg) = extra {
            // Give the turn a moment to start before cancel.
            thread::sleep(Duration::from_millis(50));
            writeln!(client, "{msg}").unwrap();
            client.flush().unwrap();
        }
        drop(client);
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    let _ = tx.send(value);
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut out = Vec::new();
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(value) => out.push(value),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if out
                        .iter()
                        .any(|v| v.get("result").is_some() && v.get("id") == Some(&json!(3)))
                        || out
                            .iter()
                            .any(|v| v.get("error").is_some() && v.get("id") == Some(&json!(3)))
                    {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        out
    }

    #[cfg(unix)]
    fn handshake() -> Vec<Value> {
        vec![
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}),
            json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}),
        ]
    }

    #[cfg(unix)]
    fn prompt(text: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session/prompt",
            "params": {
                "sessionId": "aider-1",
                "prompt": [{ "type": "text", "text": text }]
            }
        })
    }

    #[cfg(unix)]
    fn stop_reason(frames: &[Value]) -> Option<&str> {
        frames.iter().rev().find_map(|frame| {
            frame
                .get("result")
                .and_then(|r| r.get("stopReason"))
                .and_then(Value::as_str)
        })
    }

    #[cfg(unix)]
    fn agent_text(frames: &[Value]) -> String {
        frames
            .iter()
            .filter(|frame| frame.get("method").and_then(Value::as_str) == Some("session/update"))
            .filter(|frame| {
                frame
                    .get("params")
                    .and_then(|p| p.get("sessionUpdate"))
                    .and_then(Value::as_str)
                    == Some("agent_message_chunk")
            })
            .filter_map(|frame| {
                frame
                    .get("params")
                    .and_then(|p| p.get("content"))
                    .and_then(|c| c.get("text"))
                    .and_then(Value::as_str)
            })
            .collect::<Vec<_>>()
            .join("")
    }

    #[cfg(unix)]
    #[test]
    fn success_streams_a_reply_and_ends_the_turn() {
        let exec = FakeExec {
            mode: Mode::Success,
            started: Arc::new(Mutex::new(Vec::new())),
        };
        let mut msgs = handshake();
        msgs.push(prompt("hello"));
        let frames = drive(exec.clone(), &msgs, None);
        assert_eq!(stop_reason(&frames), Some("end_turn"));
        assert!(
            agent_text(&frames).contains("hello from aider"),
            "{frames:?}"
        );
        let started = exec.started.lock().unwrap();
        assert_eq!(started[0].message, "hello");
        assert_eq!(started[0].cwd, PathBuf::from("/tmp"));
    }

    #[cfg(unix)]
    #[test]
    fn a_nonzero_aider_exit_is_an_error_with_stderr() {
        let exec = FakeExec {
            mode: Mode::CliFail,
            started: Arc::new(Mutex::new(Vec::new())),
        };
        let mut msgs = handshake();
        msgs.push(prompt("hello"));
        let frames = drive(exec, &msgs, None);
        assert_eq!(stop_reason(&frames), Some("error"));
        assert!(
            agent_text(&frames).contains("Aider exited 1."),
            "{frames:?}"
        );
        assert!(agent_text(&frames).contains("No API key"), "{frames:?}");
    }

    #[cfg(unix)]
    #[test]
    fn auth_failure_is_an_error_not_success() {
        let exec = FakeExec {
            mode: Mode::AuthFail,
            started: Arc::new(Mutex::new(Vec::new())),
        };
        let mut msgs = handshake();
        msgs.push(prompt("hello"));
        let frames = drive(exec, &msgs, None);
        assert_eq!(stop_reason(&frames), Some("error"));
        assert!(agent_text(&frames).contains("No API key"), "{frames:?}");
    }

    #[cfg(unix)]
    #[test]
    fn empty_stdout_still_ends_the_turn() {
        let exec = FakeExec {
            mode: Mode::Empty,
            started: Arc::new(Mutex::new(Vec::new())),
        };
        let mut msgs = handshake();
        msgs.push(prompt("hello"));
        let frames = drive(exec, &msgs, None);
        // Host maps an end_turn with no visible text to empty_response.
        assert_eq!(stop_reason(&frames), Some("end_turn"));
        assert!(agent_text(&frames).trim().is_empty(), "{frames:?}");
    }

    #[cfg(unix)]
    #[test]
    fn cancel_kills_a_running_turn() {
        let exec = FakeExec {
            mode: Mode::Hang,
            started: Arc::new(Mutex::new(Vec::new())),
        };
        let mut msgs = handshake();
        msgs.push(prompt("hang"));
        let frames = drive(
            exec,
            &msgs,
            Some(json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": "aider-1" }
            })),
        );
        assert_eq!(stop_reason(&frames), Some("cancelled"));
    }

    #[cfg(unix)]
    #[test]
    fn initialize_declares_no_native_acp_and_no_permissions() {
        let exec = FakeExec {
            mode: Mode::Empty,
            started: Arc::new(Mutex::new(Vec::new())),
        };
        let frames = drive(exec, &handshake()[..1], None);
        let init = frames
            .iter()
            .find(|f| f.get("id") == Some(&json!(1)))
            .expect("initialize result");
        let meta = &init["result"]["_meta"]["jabot"];
        assert_eq!(meta["nativeAcp"], false);
        assert_eq!(meta["autoCommits"], false);
        let unsupported = meta["unsupported"].as_array().unwrap();
        assert!(unsupported
            .iter()
            .any(|v| v == "session/request_permission"));
        assert_eq!(init["result"]["agentInfo"]["name"], AGENT_NAME);
        assert!(
            init["result"]["agentCapabilities"]["sessionCapabilities"]["resume"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn prompt_files_take_resource_links_and_at_mentions() {
        let prompt = json!([
            { "type": "text", "text": "edit @src/lib.rs please" },
            { "type": "resource_link", "uri": "file:///tmp/app/main.rs" }
        ]);
        let files = prompt_files(Some(&prompt));
        assert!(files.contains(&"src/lib.rs".into()), "{files:?}");
        assert!(files.contains(&"/tmp/app/main.rs".into()), "{files:?}");
    }

    #[test]
    fn strip_ansi_drops_color_but_keeps_words() {
        assert_eq!(strip_ansi("\u{1b}[32mgreen\u{1b}[0m text"), "green text");
    }
}
