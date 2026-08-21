//! `jabot-hostd` — the host session spoken over NDJSON on stdio.
//!
//! Decision #4 keeps the host in-process inside the Tauri binary and calls the
//! API "socket-shaped" so extracting it later is packaging, not a rewrite.
//! This binary is that claim under test: it wraps the exact same
//! [`HostSession`] the `host_rpc` Tauri command wraps, and frames it with the
//! exact same [`encode_frame`] / [`decode_frames`] codec a Unix socket will
//! use. `tests/e2e/` drives it from TypeScript through the real protocol.
//!
//! It is a test/dev entrypoint, not the shipping sidecar — the app still runs
//! the host in-process. See `DEVIATIONS.md` (D-001).
//!
//! Requests and responses are correlated by `id`; host-initiated notifications
//! are written to the same stream as they are produced. One JSON value per
//! line, in both directions. `--data-dir <path>` opens a real SQLite store and
//! identity under `<path>`; without it the session is ephemeral.
//!
//! `--listen <path>` additionally binds a Unix domain socket and serves the
//! *same* frames to every client that connects to it (#29). That is the reach
//! ladder's rung 0 from `remote-and-mobile/protocol-and-reach.md` and the
//! thing decision #4 said would be needed "when a second client exists": a
//! paired phone is a second client, and it needs a connection of its own so
//! `host/hello` can bind it to its own device and its own role. Requests are
//! answered on the connection that asked; notifications — `permission/ask`
//! above all — are **broadcast to every** connection, which is what the
//! research means by "the host broadcasts to every connected client; the first
//! authentic reply wins".
//!
//! This is still the dev/test binary (it only exists under the `dev-bins`
//! feature). The shipping app keeps the host in-process behind Tauri IPC, as
//! decision #4 requires; nothing here puts a listener inside JaBot.app.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Stdout, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jabot_lib::host::{
    decode_frames, encode_frame, AdapterWake, HostSession, JsonRpcMessage, JsonRpcResponse,
    RequestId, LOCAL_CONNECTION,
};

/// One attached socket client's write end.
///
/// On a platform with no Unix sockets the map is simply never populated —
/// `--listen` refuses before it could be — so the rest of the file needs no
/// second code path.
#[cfg(unix)]
type ClientSink = std::os::unix::net::UnixStream;
#[cfg(not(unix))]
type ClientSink = std::io::Sink;

/// One attached socket client: the connection id it is dispatched under, and
/// the write end of its stream.
struct Client {
    connection: String,
    sink: ClientSink,
}

/// Everyone currently listening: stdio, plus each open socket.
///
/// Notifications go to every client the host has *identified*. A socket is a
/// connection, not a subscription: `require_hello` covers requests, but a
/// notification is pushed, so nothing about answering a request governs it. A
/// stream carrying prompt text and live `bash` output is exactly what
/// `pairing-security-mobile.md` rule 2 says must not leak, so a connection
/// that has not completed a `host/hello` is skipped until it has — and
/// skipped again the moment its device is dropped or revoked, because the
/// answer is read off the session on every frame rather than latched at
/// accept time.
///
/// stdio is the exception, and deliberately: it is the process that spawned
/// this host, which is the same colocated trust `host/hello` grants it.
///
/// A write that fails means that client hung up mid-frame; it is dropped
/// rather than retried, because a partially written frame has already desynced
/// that stream and no other client should wait for it.
#[derive(Default)]
struct Clients {
    next_id: u64,
    sinks: HashMap<u64, Client>,
}

impl Clients {
    fn add(&mut self, connection: &str, sink: ClientSink) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.sinks.insert(
            id,
            Client {
                connection: connection.to_string(),
                sink,
            },
        );
        id
    }

    fn remove(&mut self, id: u64) {
        self.sinks.remove(&id);
    }

    fn broadcast(&mut self, stdout: &Stdout, session: &HostSession, message: &JsonRpcMessage) {
        let Ok(frame) = encode_frame(message) else {
            eprintln!("jabot-hostd: encode failed");
            return;
        };
        {
            let mut handle = stdout.lock();
            let _ = handle
                .write_all(frame.as_bytes())
                .and_then(|()| handle.flush());
        }
        self.sinks.retain(|_, client| {
            if !session.connection_has_device(&client.connection) {
                // Nothing it has said entitles it to the stream — but it is
                // still connected, and a hello later would.
                return true;
            }
            client
                .sink
                .write_all(frame.as_bytes())
                .and_then(|()| client.sink.flush())
                .is_ok()
        });
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut data_dir: Option<PathBuf> = None;
    let mut socket_path: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => match args.next() {
                Some(path) => data_dir = Some(PathBuf::from(path)),
                None => fatal("--data-dir requires a path"),
            },
            "--listen" => match args.next() {
                Some(path) => socket_path = Some(PathBuf::from(path)),
                None => fatal("--listen requires a socket path"),
            },
            other => fatal(&format!("unknown argument {other}")),
        }
    }

    // Bound *before* the first byte of stdio is read, and that ordering is the
    // whole readiness protocol: a client that has had an answer on stdio knows
    // the socket is accepting, so nothing has to poll for the file or parse a
    // banner line out of the protocol stream.
    let listener = socket_path.as_deref().map(bind_listener);

    let session = match &data_dir {
        Some(dir) => {
            if let Err(err) = std::fs::create_dir_all(dir) {
                fatal(&format!("create data dir {}: {err}", dir.display()));
            }
            HostSession::load(dir)
        }
        None => HostSession::ephemeral(),
    };

    // Adapter events arrive whenever a child process feels like writing, not
    // when a client sends a request, so draining only after a request would
    // strand every session/update and permission/ask until the client happened
    // to ask something else. The Tauri host solves this with a `jabot-acp-pump`
    // thread (`lib.rs`); this is that thread, with stdout in place of the Tauri
    // event bus. Without it the stdio host looks alive but never streams.
    let wake = session.adapter_wake();
    let session = Arc::new(Mutex::new(session));
    let clients = Arc::new(Mutex::new(Clients::default()));
    spawn_acp_pump(Arc::clone(&session), Arc::clone(&clients), wake);
    if let Some(listener) = listener {
        spawn_accept_loop(Arc::clone(&session), Arc::clone(&clients), listener);
    }

    let stdin = BufReader::new(std::io::stdin());
    let stdout = std::io::stdout();
    let mut buffer = String::new();

    for line in stdin.lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                eprintln!("jabot-hostd: stdin read failed: {err}");
                break;
            }
        };
        buffer.push_str(&line);
        buffer.push('\n');

        let messages = match decode_frames(&buffer) {
            Ok((messages, rest)) => {
                buffer = rest;
                messages
            }
            Err(err) => {
                // A malformed line must not desync the stream: report it and
                // drop the buffer rather than reparsing the same bad bytes.
                buffer.clear();
                write_message(
                    &stdout,
                    &JsonRpcMessage::Response(JsonRpcResponse::from_rpc_error(
                        RequestId::Null,
                        err,
                    )),
                );
                continue;
            }
        };

        for message in messages {
            let JsonRpcMessage::Request(request) = message else {
                // The host is the server: it answers requests and emits
                // notifications. Anything else on the inbound stream is noise.
                continue;
            };
            // Written while the session lock is still held, and the pump
            // thread does the same. Two drainers that release the lock first
            // can interleave their writes, and a client would then see `seq`
            // 3 before `seq` 1 — which is exactly the order the envelope
            // exists to promise (#14 relies on it to de-duplicate a replay
            // against the live stream).
            let mut guard = lock(&session);
            let response = guard.handle_request_on(LOCAL_CONNECTION, request);
            let outbound = guard.take_outbound();
            write_message(&stdout, &JsonRpcMessage::Response(response));
            // Notifications reach every attached client, not only this one:
            // the phone must see `permission/ask` even though the desktop is
            // the connection that provoked it.
            let mut sinks = lock_clients(&clients);
            for notification in outbound {
                sinks.broadcast(&stdout, &guard, &JsonRpcMessage::Notification(notification));
            }
            drop(sinks);
            drop(guard);
        }
    }

    lock(&session).checkpoint_store();
}

/// Drain ACP inbound events and write the notifications they produce.
///
/// Mirrors `spawn_acp_pump` in `lib.rs`. The timeout matters as much as the
/// wake: `AdapterWake::ping` can land while the pump is mid-cycle, so a purely
/// event-driven wait would miss it and stall until the next unrelated event.
fn spawn_acp_pump(
    session: Arc<Mutex<HostSession>>,
    clients: Arc<Mutex<Clients>>,
    wake: Arc<AdapterWake>,
) {
    std::thread::Builder::new()
        .name("jabot-acp-pump".into())
        .spawn(move || {
            let stdout = std::io::stdout();
            loop {
                wake.wait_timeout(Duration::from_millis(50));
                let mut guard = lock(&session);
                guard.pump_acp();
                let outbound = guard.take_outbound();
                // Under the lock, for the ordering reason above.
                let mut sinks = lock_clients(&clients);
                for notification in outbound {
                    sinks.broadcast(&stdout, &guard, &JsonRpcMessage::Notification(notification));
                }
                drop(sinks);
                drop(guard);
            }
        })
        .expect("acp pump thread");
}

/// Bind the listening socket, removing whatever a previous run left behind.
///
/// A stale socket file is not a running host — it is a file — and refusing to
/// start because of one would make a crashed host unrestartable.
///
/// The mode is the point, not housekeeping. `pairing-security-mobile.md` rule
/// 1 lets the local socket skip TLS *because* it can be "`0700` in a user
/// dir", and D-016 leans on the same sentence when it calls rung 0 "loopback,
/// filesystem permissions". Default umask makes that `0755` — every account on
/// the machine may connect — so the one control the design rests on has to be
/// set explicitly, or the prose is describing a socket that does not exist.
///
/// `umask` rather than a `chmod` after the fact: `bind` publishes the socket
/// in the directory the instant it returns, and a `chmod` on the next line is
/// a window in which it is world-connectable. This runs before any thread is
/// spawned, so the process-wide umask cannot race another one of ours.
#[cfg(unix)]
fn bind_listener(path: &std::path::Path) -> std::os::unix::net::UnixListener {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let _ = std::fs::remove_file(path);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        // A directory we create is ours; one that already exists is the user's
        // choice and not ours to tighten. `0700` matters even with the socket
        // at `0600`, because some Unixes have historically ignored permissions
        // on a socket inode — a directory nobody else may traverse is the
        // check every kernel does agree on.
        if let Err(err) = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
        {
            fatal(&format!("create {}: {err}", parent.display()));
        }
    }
    // SAFETY: a libc call with no pointers and no failure mode; single-threaded
    // here, and restored before anything else can observe it.
    let previous_umask = unsafe { libc::umask(0o177) };
    let bound = std::os::unix::net::UnixListener::bind(path);
    unsafe {
        libc::umask(previous_umask);
    }
    let listener = match bound {
        Ok(listener) => listener,
        Err(err) => fatal(&format!("listen on {}: {err}", path.display())),
    };
    // Belt and braces: a umask can only clear bits, so this changes nothing on
    // a sane platform — but it makes the guarantee true by assertion rather
    // than by inheritance, which is what the e2e checks.
    if let Err(err) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        fatal(&format!("chmod {}: {err}", path.display()));
    }
    listener
}

#[cfg(not(unix))]
fn bind_listener(path: &std::path::Path) -> ! {
    fatal(&format!(
        "--listen {} needs a Unix domain socket; this platform has none",
        path.display()
    ))
}

/// Accept clients forever, one reader thread each.
///
/// Each connection gets its own id, and that id is what
/// `HostSession::handle_request_on` binds a device to. Two clients on one host
/// therefore have two `host/hello`s and two roles — the property the whole
/// approver story rests on (#19, #29).
#[cfg(unix)]
fn spawn_accept_loop(
    session: Arc<Mutex<HostSession>>,
    clients: Arc<Mutex<Clients>>,
    listener: std::os::unix::net::UnixListener,
) {
    std::thread::Builder::new()
        .name("jabot-hostd-accept".into())
        .spawn(move || {
            let mut next_connection = 0_u64;
            for stream in listener.incoming() {
                let stream = match stream {
                    Ok(stream) => stream,
                    Err(err) => {
                        eprintln!("jabot-hostd: accept failed: {err}");
                        continue;
                    }
                };
                next_connection += 1;
                let connection = format!("socket-{next_connection}");
                let session = Arc::clone(&session);
                let clients = Arc::clone(&clients);
                if let Err(err) = std::thread::Builder::new()
                    .name(connection.clone())
                    .spawn(move || serve_connection(session, clients, stream, connection))
                {
                    eprintln!("jabot-hostd: connection thread: {err}");
                }
            }
        })
        .expect("accept thread");
}

/// One socket client, until it hangs up.
///
/// Responses go back on the connection that asked; notifications go to
/// everyone. When the client leaves, its device binding is dropped, so
/// `device/list` stops claiming the phone is connected.
#[cfg(unix)]
fn serve_connection(
    session: Arc<Mutex<HostSession>>,
    clients: Arc<Mutex<Clients>>,
    stream: std::os::unix::net::UnixStream,
    connection: String,
) {
    let (reader, mut writer, sink) = match (stream.try_clone(), stream.try_clone()) {
        (Ok(reader), Ok(sink)) => (BufReader::new(reader), stream, sink),
        _ => {
            eprintln!("jabot-hostd: could not split socket");
            return;
        }
    };
    let sink_id = lock_clients(&clients).add(&connection, sink);
    let stdout = std::io::stdout();
    let mut buffer = String::new();

    for line in reader.lines() {
        let Ok(line) = line else { break };
        buffer.push_str(&line);
        buffer.push('\n');
        let messages = match decode_frames(&buffer) {
            Ok((messages, rest)) => {
                buffer = rest;
                messages
            }
            Err(err) => {
                buffer.clear();
                write_to(
                    &mut writer,
                    &JsonRpcMessage::Response(JsonRpcResponse::from_rpc_error(
                        RequestId::Null,
                        err,
                    )),
                );
                continue;
            }
        };
        for message in messages {
            let JsonRpcMessage::Request(request) = message else {
                continue;
            };
            let mut guard = lock(&session);
            let response = guard.handle_request_on(&connection, request);
            let outbound = guard.take_outbound();
            write_to(&mut writer, &JsonRpcMessage::Response(response));
            let mut sinks = lock_clients(&clients);
            for notification in outbound {
                sinks.broadcast(&stdout, &guard, &JsonRpcMessage::Notification(notification));
            }
            drop(sinks);
            drop(guard);
        }
    }

    lock_clients(&clients).remove(sink_id);
    lock(&session).drop_connection(&connection);
}

fn lock_clients(clients: &Arc<Mutex<Clients>>) -> std::sync::MutexGuard<'_, Clients> {
    clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_to<W: Write>(writer: &mut W, message: &JsonRpcMessage) {
    match encode_frame(message) {
        Ok(frame) => {
            if writer.write_all(frame.as_bytes()).is_err() || writer.flush().is_err() {
                // The client hung up; the read loop will notice.
            }
        }
        Err(err) => eprintln!("jabot-hostd: encode failed: {err}"),
    }
}

/// A poisoned host mutex means another thread panicked mid-update, not that the
/// session is unusable — the Tauri host takes the same view.
fn lock(session: &Arc<Mutex<HostSession>>) -> std::sync::MutexGuard<'_, HostSession> {
    session
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_message(stdout: &std::io::Stdout, message: &JsonRpcMessage) {
    match encode_frame(message) {
        Ok(frame) => {
            let mut handle = stdout.lock();
            if handle.write_all(frame.as_bytes()).is_err() || handle.flush().is_err() {
                // The client hung up; nothing left to say.
            }
        }
        Err(err) => eprintln!("jabot-hostd: encode failed: {err}"),
    }
}

fn fatal(message: &str) -> ! {
    eprintln!("jabot-hostd: {message}");
    std::process::exit(2);
}
