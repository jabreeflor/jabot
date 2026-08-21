//! The loopback MCP endpoint Chief's own tools arrive on (#24).
//!
//! Decision #6 says Chief is "a harness session with extra **host** tools" —
//! not a third runtime, not a thin LLM loop, and not four more entries in
//! #18's provider catalog. That leaves one question: how does a tool the
//! *host* implements get in front of an agent running in a subprocess that
//! only speaks ACP?
//!
//! The answer here is the smallest one that is still real. The host binds a
//! `127.0.0.1` listener on an ephemeral port, speaks MCP over it, and passes
//! the session a single `{"type":"http"}` entry pointing at itself — the exact
//! shape `tools/servers.rs` already builds for a remote provider, so nothing
//! in the adapter has to know this server is us. Two things make that safe
//! rather than merely convenient:
//!
//! - **Loopback and an ephemeral port.** The port is known only to this
//!   process and to the `session/new` it just sent. Nothing off the machine
//!   can reach it, and nothing on the machine can guess it.
//! - **A per-thread bearer token.** One bridge per Chief thread, one random
//!   token each, checked on every request. Another local process that finds
//!   the port still cannot hand work to the crew, and a tool call can always
//!   be attributed to the thread that made it — which is what the handoff
//!   trail records.
//!
//! **The bridge never touches [`HostSession`].** It cannot: the host is a
//! single `&mut` owner driven by a pump, and a listener thread reaching into
//! it would need a lock around everything. So a request becomes a [`Pending`]
//! on a channel, the connection thread blocks on the answer, and the host
//! drains and answers it from the pump it already runs — the same shape the
//! ACP reader threads use (`acp::AcpConnection`). The wake is pinged so the
//! answer takes a millisecond rather than the pump's next tick.
//!
//! What this deliberately does **not** implement: SSE streaming, MCP sessions,
//! resources, prompts, and JSON-RPC batching (removed from MCP in 2025-06-18).
//! Every tool here answers in one round trip, so a second transport would be
//! two code paths proving the same thing.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use uuid::Uuid;

use super::super::acp::AdapterWake;

/// The MCP revision this server claims. Clients that ask for another are
/// answered with this one, which the spec allows and every SDK handles.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
/// The single path the server answers on.
const PATH: &str = "/mcp";
/// What the agent's `mcpServers` entry is called. It becomes the prefix a
/// harness puts on the tool names, so it is the product's name and not
/// `host_tools`: the model should read `jabot__handoff_to_bot` and know who
/// is being asked.
pub const SERVER_NAME: &str = "jabot";

/// How long a connection thread waits for the host's pump to answer. Long
/// because the answer can involve spawning an adapter and a git worktree;
/// bounded because a client blocked forever on a host that stopped pumping is
/// a session that looks hung with nothing to show for it.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const ACCEPT_POLL: Duration = Duration::from_millis(20);
/// An idle keep-alive connection is closed rather than held forever.
const READ_TIMEOUT: Duration = Duration::from_secs(600);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
/// Tool arguments are a few hundred bytes. A megabyte is already absurd, and
/// the cap is what stops a bad `Content-Length` from being an allocation.
const MAX_BODY: usize = 1 << 20;

/// What the agent asked the host for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Which of Chief's host tools this thread's bot is actually allowed to
    /// use. Asked every time rather than captured at bind: the allowlist is a
    /// row the user can edit while the session is live (#17).
    ListTools,
    Call {
        tool: String,
        arguments: Value,
    },
}

/// One request waiting on the host. Answering it is what unblocks the agent.
#[derive(Debug)]
pub struct Pending {
    pub thread_id: String,
    pub ask: Ask,
    reply: Sender<Result<Value, String>>,
}

impl Pending {
    /// `Err` is a tool error the model can read and act on, not a transport
    /// failure — MCP renders it as `isError` with the text, which is how a
    /// model learns "there is no bot called that" instead of retrying forever.
    pub fn answer(self, result: Result<Value, String>) {
        // A dropped receiver means the connection thread gave up or the agent
        // hung up. The work is already done; there is nobody to tell.
        let _ = self.reply.send(result);
    }
}

/// A live loopback MCP server for exactly one thread.
///
/// Dropping it stops the listener: the accept loop polls the flag, and every
/// connection thread notices the same flag between requests.
#[derive(Debug)]
pub struct Bridge {
    endpoint: String,
    token: String,
    rx: Receiver<Pending>,
    stop: Arc<AtomicBool>,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Bridge {
    /// Bind, and start serving. `wake` is pinged whenever a request lands so
    /// the host's pump answers immediately instead of on its next tick.
    pub fn start(thread_id: &str, wake: Arc<AdapterWake>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;

        let token = Uuid::new_v4().to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<Pending>();

        let server = Server {
            thread_id: thread_id.to_string(),
            token: token.clone(),
            tx,
            stop: Arc::clone(&stop),
            wake,
        };
        std::thread::Builder::new()
            .name(format!("jabot-chief-mcp-{thread_id}"))
            .spawn(move || server.accept_loop(listener))?;

        Ok(Self {
            endpoint: format!("http://127.0.0.1:{port}{PATH}"),
            token,
            rx,
            stop,
        })
    }

    /// The `mcpServers` element handed to `session/new`.
    ///
    /// Identical in shape to a remote provider's entry from #18's catalog,
    /// because from the adapter's side that is exactly what it is.
    pub fn server_json(&self) -> Value {
        json!({
            "type": "http",
            "name": SERVER_NAME,
            "url": self.endpoint,
            "headers": [{ "name": "Authorization", "value": format!("Bearer {}", self.token) }],
        })
    }

    /// Take the next request the host owes an answer to, if any.
    pub fn try_recv(&self) -> Result<Pending, TryRecvError> {
        self.rx.try_recv()
    }
}

/// The half that lives on the listener thread. Deliberately holds no reference
/// to the host — only a channel to it.
struct Server {
    thread_id: String,
    token: String,
    tx: Sender<Pending>,
    stop: Arc<AtomicBool>,
    wake: Arc<AdapterWake>,
}

impl Server {
    fn accept_loop(self, listener: TcpListener) {
        let shared = Arc::new(self);
        loop {
            if shared.stop.load(Ordering::Relaxed) {
                return;
            }
            match listener.accept() {
                Ok((stream, _peer)) => {
                    let handler = Arc::clone(&shared);
                    // A thread per connection so a client holding one open
                    // keep-alive socket cannot stop the next request from
                    // being served. Connections here are counted in ones.
                    if std::thread::Builder::new()
                        .name("jabot-chief-mcp-conn".into())
                        .spawn(move || handler.serve(stream))
                        .is_err()
                    {
                        // Out of threads: better to drop this connection than
                        // to take the listener down with it.
                        continue;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(ACCEPT_POLL);
                }
                Err(_) => return,
            }
        }
    }

    fn serve(&self, stream: TcpStream) {
        // The accepted socket may have inherited the listener's non-blocking
        // flag (BSD does, Linux does not). Say what we want rather than
        // depending on which one this is.
        if stream.set_nonblocking(false).is_err() {
            return;
        }
        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
        let Ok(write_half) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(stream);
        let mut writer = write_half;

        loop {
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
            match read_request(&mut reader) {
                Ok(None) => return,
                Ok(Some(request)) => {
                    let response = self.respond(&request);
                    if write_response(&mut writer, &response).is_err() {
                        return;
                    }
                    if !request.keep_alive {
                        return;
                    }
                }
                Err(status) => {
                    let _ = write_response(&mut writer, &HttpResponse::text(status, "bad request"));
                    return;
                }
            }
        }
    }

    fn respond(&self, request: &HttpRequest) -> HttpResponse {
        if request.path != PATH {
            return HttpResponse::text(404, "not found");
        }
        if !self.authorized(request) {
            // No `WWW-Authenticate` challenge: this is not an OAuth-protected
            // resource, and advertising a discovery flow that does not exist
            // sends a well-behaved client down a road with no end.
            return HttpResponse::text(401, "unauthorized");
        }
        match request.method.as_str() {
            // No SSE stream to open, and saying so is what makes a client fall
            // back to plain POSTs instead of waiting on a stream forever.
            "GET" => HttpResponse::text(405, "this endpoint does not stream"),
            // Session teardown. There is no session to tear down.
            "DELETE" => HttpResponse::text(200, ""),
            "POST" => self.rpc(&request.body),
            _ => HttpResponse::text(405, "method not allowed"),
        }
    }

    fn authorized(&self, request: &HttpRequest) -> bool {
        let expected = format!("Bearer {}", self.token);
        request
            .header("authorization")
            .is_some_and(|value| value.trim() == expected)
    }

    fn rpc(&self, body: &str) -> HttpResponse {
        let Ok(message) = serde_json::from_str::<Value>(body) else {
            return HttpResponse::json(&json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32700, "message": "parse error" }
            }));
        };
        // MCP dropped JSON-RPC batching in 2025-06-18 and every tool here
        // answers in one round trip, so an array is a client speaking an older
        // revision — told so, rather than half-answered.
        if message.is_array() {
            return HttpResponse::json(&json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32600, "message": "batched requests are not supported" }
            }));
        }
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        // A notification carries no id and gets no body — 202 is what the MCP
        // HTTP transport expects, and a JSON-RPC response to a notification is
        // a protocol error in the other direction.
        let Some(id) = id.filter(|id| !id.is_null()) else {
            return HttpResponse::text(202, "");
        };

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER_NAME, "title": "JaBot", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Host tools JaBot implements for Chief: route work to the crew, \
                                 start coding sessions, and fold long jobs away.",
            })),
            "ping" => Ok(json!({})),
            "tools/list" => self
                .ask(Ask::ListTools)
                .map(|tools| json!({ "tools": tools })),
            "tools/call" => return self.call_tool(id, &params),
            other => {
                return HttpResponse::json(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": format!("method not found: {other}") }
                }))
            }
        };
        HttpResponse::json(&match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(message) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32603, "message": message }
            }),
        })
    }

    /// A failed tool call is a **successful** JSON-RPC response carrying
    /// `isError` (MCP §tools). That is not pedantry: a protocol-level error is
    /// the client's problem and never reaches the model, so a refusal returned
    /// that way would leave the agent retrying a bot that does not exist.
    fn call_tool(&self, id: Value, params: &Value) -> HttpResponse {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let answer = self.ask(Ask::Call {
            tool: name,
            arguments,
        });
        let result = match answer {
            Ok(value) => json!({
                "content": [{ "type": "text", "text": render(&value) }],
                "structuredContent": value,
                "isError": false,
            }),
            Err(message) => json!({
                "content": [{ "type": "text", "text": message }],
                "isError": true,
            }),
        };
        HttpResponse::json(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    /// Hand the ask to the host and wait for the pump to answer it.
    fn ask(&self, ask: Ask) -> Result<Value, String> {
        let (reply, answers) = mpsc::channel();
        let pending = Pending {
            thread_id: self.thread_id.clone(),
            ask,
            reply,
        };
        if self.tx.send(pending).is_err() {
            return Err("JaBot is no longer listening on this session".into());
        }
        // Without this the answer waits for whatever the pump's next tick is.
        self.wake.ping();
        match answers.recv_timeout(CALL_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                Err("JaBot did not answer in time; the action may still be running".into())
            }
            Err(RecvTimeoutError::Disconnected) => Err("JaBot stopped before answering".into()),
        }
    }
}

/// The text half of a tool result. Models read this; `structuredContent`
/// carries the same answer for anything that parses.
fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
    keep_alive: bool,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl HttpResponse {
    fn json(value: &Value) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body: value.to_string(),
        }
    }

    fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: body.to_string(),
        }
    }
}

/// `Ok(None)` is a clean end of stream — the client hung up between requests.
fn read_request(reader: &mut BufReader<TcpStream>) -> Result<Option<HttpRequest>, u16> {
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(_) => return Ok(None),
    }
    // A stray blank line before the request line is legal in HTTP/1.1.
    while line.trim().is_empty() {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(None),
            Ok(_) => {}
            Err(_) => return Ok(None),
        }
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_uppercase();
    let target = parts.next().unwrap_or_default().to_string();
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();
    if method.is_empty() || target.is_empty() {
        return Err(400);
    }
    let path = target
        .split_once('?')
        .map(|(path, _)| path.to_string())
        .unwrap_or(target);

    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) => return Err(400),
            Ok(_) => {}
            Err(_) => return Err(400),
        }
        let header = header.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(400);
        };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        if headers.len() > 64 {
            return Err(431);
        }
    }

    let length: usize = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return Err(413);
    }
    let mut body = vec![0u8; length];
    if length > 0 && reader.read_exact(&mut body).is_err() {
        return Err(400);
    }
    let body = String::from_utf8_lossy(&body).into_owned();

    let connection = headers
        .iter()
        .find(|(name, _)| name == "connection")
        .map(|(_, value)| value.to_ascii_lowercase());
    let keep_alive = match connection.as_deref() {
        Some(value) if value.contains("close") => false,
        Some(value) if value.contains("keep-alive") => true,
        _ => version != "HTTP/1.0",
    };

    Ok(Some(HttpRequest {
        method,
        path,
        headers,
        body,
        keep_alive,
    }))
}

fn write_response(stream: &mut TcpStream, response: &HttpResponse) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(response.body.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bridge plus a thread answering its asks from a script, so the HTTP
    /// and MCP halves can be exercised without a host.
    struct Harness {
        endpoint: String,
        token: String,
        done: Arc<AtomicBool>,
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            // Ends the responder thread, which drops the bridge, which stops
            // the listener — so a test does not leave a port open behind it.
            self.done.store(true, Ordering::Relaxed);
        }
    }

    fn harness(answer: fn(&Pending) -> Result<Value, String>) -> Harness {
        let bridge = Bridge::start("t-chief", AdapterWake::new()).expect("bind");
        let endpoint = bridge.endpoint.clone();
        let token = bridge.token.clone();
        let done = Arc::new(AtomicBool::new(false));
        let stopper = Arc::clone(&done);
        std::thread::spawn(move || {
            // Owning the bridge here keeps the listener alive for the test.
            while !stopper.load(Ordering::Relaxed) {
                match bridge.try_recv() {
                    Ok(pending) => {
                        let result = answer(&pending);
                        pending.answer(result);
                    }
                    Err(TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(2)),
                    Err(TryRecvError::Disconnected) => return,
                }
            }
        });
        Harness {
            endpoint,
            token,
            done,
        }
    }

    fn respond(pending: &Pending) -> Result<Value, String> {
        assert_eq!(pending.thread_id, "t-chief");
        match &pending.ask {
            Ask::ListTools => Ok(json!([{ "name": "fold_thread" }])),
            Ask::Call { tool, arguments } => {
                if tool == "handoff_to_bot" {
                    Ok(json!({ "threadId": "bot-writer", "task": arguments["task"] }))
                } else {
                    Err(format!("no such tool: {tool}"))
                }
            }
        }
    }

    /// One request, one response. Raw sockets on purpose: the claim under test
    /// is that an ordinary HTTP client can talk to this.
    fn post(harness: &Harness, body: &Value, token: Option<&str>) -> (u16, String) {
        let addr = harness
            .endpoint
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let mut stream = TcpStream::connect(addr).expect("connect");
        let payload = body.to_string();
        let auth = token.unwrap_or(&harness.token);
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {auth}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            payload.len()
        );
        stream.write_all(request.as_bytes()).expect("write");
        let mut raw = String::new();
        stream.read_to_string(&mut raw).expect("read");
        let status: u16 = raw
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("status");
        let body = raw.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
        (status, body)
    }

    fn rpc(harness: &Harness, method: &str, params: Value) -> Value {
        let (status, body) = post(
            harness,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }),
            None,
        );
        assert_eq!(status, 200, "{method}: {body}");
        serde_json::from_str(&body).expect("json")
    }

    #[test]
    fn it_speaks_enough_mcp_to_be_initialised_and_listed() {
        let harness = harness(respond);

        let initialized = rpc(&harness, "initialize", json!({}));
        assert_eq!(
            initialized["result"]["protocolVersion"],
            MCP_PROTOCOL_VERSION
        );
        assert_eq!(initialized["result"]["serverInfo"]["name"], SERVER_NAME);
        assert!(initialized["result"]["capabilities"]["tools"].is_object());

        // The tool list is the host's answer, not a constant baked in here:
        // the allowlist can change while the session is live.
        let listed = rpc(&harness, "tools/list", json!({}));
        assert_eq!(listed["result"]["tools"][0]["name"], "fold_thread");
    }

    #[test]
    fn a_tool_call_reaches_the_host_and_comes_back_structured() {
        let harness = harness(respond);
        let called = rpc(
            &harness,
            "tools/call",
            json!({ "name": "handoff_to_bot", "arguments": { "task": "draft the note" } }),
        );
        let result = &called["result"];
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["threadId"], "bot-writer");
        assert_eq!(result["structuredContent"]["task"], "draft the note");
        // The text half is what a model without structured output reads.
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("bot-writer"));
    }

    /// A refused tool has to come back as a *result* the model can read. As a
    /// JSON-RPC error it would be swallowed by the client and the agent would
    /// keep asking for a bot that does not exist.
    #[test]
    fn a_refusal_is_an_is_error_result_not_a_jsonrpc_error() {
        let harness = harness(respond);
        let called = rpc(
            &harness,
            "tools/call",
            json!({ "name": "telepathy", "arguments": {} }),
        );
        assert!(called.get("error").is_none(), "{called}");
        assert_eq!(called["result"]["isError"], true);
        assert!(called["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("telepathy"));
    }

    /// The token is the whole access control story on a loopback port, so the
    /// test that it is actually checked is not optional.
    #[test]
    fn another_process_on_this_machine_cannot_hand_work_to_the_crew() {
        let harness = harness(respond);
        let (status, _) = post(
            &harness,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            Some("guessed-it"),
        );
        assert_eq!(status, 401);

        let (missing, _) = post(
            &harness,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
            Some(""),
        );
        assert_eq!(missing, 401);
    }

    #[test]
    fn a_notification_gets_no_body_and_an_unknown_method_says_so() {
        let harness = harness(respond);
        let (status, body) = post(
            &harness,
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            None,
        );
        assert_eq!(status, 202);
        assert!(body.is_empty(), "{body}");

        let unknown = rpc(&harness, "resources/list", json!({}));
        assert_eq!(unknown["error"]["code"], -32601);
    }

    /// Dropping the bridge has to free the port, or a host that opened and
    /// closed a hundred Chief sessions would be holding a hundred listeners.
    #[test]
    fn dropping_the_bridge_stops_the_listener() {
        let bridge = Bridge::start("t-chief", AdapterWake::new()).expect("bind");
        let addr = bridge
            .endpoint
            .trim_start_matches("http://")
            .trim_end_matches(PATH)
            .to_string();
        assert!(TcpStream::connect(&addr).is_ok());
        drop(bridge);

        // The accept loop notices the flag within a poll; give it a few.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if TcpStream::connect(&addr).is_err() {
                return;
            }
            std::thread::sleep(ACCEPT_POLL);
        }
        panic!("the listener is still accepting after the bridge was dropped");
    }
}
