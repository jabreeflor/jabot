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

use jabot_lib::host::{
    decode_frames, encode_frame, HostSession, JsonRpcMessage, JsonRpcResponse, RequestId,
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

    let mut session = match &data_dir {
        Some(dir) => {
            if let Err(err) = std::fs::create_dir_all(dir) {
                fatal(&format!("create data dir {}: {err}", dir.display()));
            }
            HostSession::load(dir)
        }
        None => HostSession::ephemeral(),
    };

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
            let response = session.handle_request(request);
            write_message(&stdout, &JsonRpcMessage::Response(response));
            for notification in session.take_outbound() {
                write_message(&stdout, &JsonRpcMessage::Notification(notification));
            }
        }
    }

    session.checkpoint_store();
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
