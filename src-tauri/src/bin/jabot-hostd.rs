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

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jabot_lib::host::{
    decode_frames, encode_frame, AdapterWake, HostSession, JsonRpcMessage, JsonRpcResponse,
    RequestId,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut data_dir: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => match args.next() {
                Some(path) => data_dir = Some(PathBuf::from(path)),
                None => fatal("--data-dir requires a path"),
            },
            other => fatal(&format!("unknown argument {other}")),
        }
    }

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
    spawn_acp_pump(Arc::clone(&session), wake);

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
            let response = guard.handle_request(request);
            let outbound = guard.take_outbound();
            write_message(&stdout, &JsonRpcMessage::Response(response));
            for notification in outbound {
                write_message(&stdout, &JsonRpcMessage::Notification(notification));
            }
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
fn spawn_acp_pump(session: Arc<Mutex<HostSession>>, wake: Arc<AdapterWake>) {
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
                for notification in outbound {
                    write_message(&stdout, &JsonRpcMessage::Notification(notification));
                }
                drop(guard);
            }
        })
        .expect("acp pump thread");
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
