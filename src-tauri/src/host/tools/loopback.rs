//! The loopback redirect the browser comes back to (RFC 8252 §7.3).
//!
//! A desktop app cannot receive a redirect on a hosted URL, and a custom
//! scheme can be claimed by any other app on the machine. A listener on
//! `127.0.0.1` with an ephemeral port cannot be: the port is known only to
//! this process and to the authorization request it just sent.
//!
//! The listener answers exactly one authorization response and then stops.
//! Three things it refuses to do, each of them a real attack or a real
//! confusion rather than defensive noise:
//!
//! - Return a code for a mismatched `state` — that is the CSRF check.
//! - Keep waiting past its deadline, so an abandoned browser tab does not
//!   leave a socket open forever.
//! - Report the provider's `error=` as success.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CALLBACK_PATH: &str = "/callback";
const POLL: Duration = Duration::from_millis(50);

#[derive(Debug, thiserror::Error)]
pub enum LoopbackError {
    #[error("could not listen on 127.0.0.1: {0}")]
    Bind(String),
    #[error("the sign-in window was not completed in time")]
    TimedOut,
    #[error("sign-in was cancelled")]
    Cancelled,
    /// The redirect did not carry the `state` we sent. Someone else's
    /// authorization response, or a forged one; either way, not ours.
    #[error("the sign-in response did not match this request")]
    StateMismatch,
    #[error("{0}")]
    Provider(String),
}

#[derive(Debug)]
pub struct Loopback {
    listener: TcpListener,
    redirect_uri: String,
}

impl Loopback {
    pub fn bind() -> Result<Self, LoopbackError> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .map_err(|e| LoopbackError::Bind(e.to_string()))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| LoopbackError::Bind(e.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|e| LoopbackError::Bind(e.to_string()))?
            .port();
        Ok(Self {
            listener,
            redirect_uri: format!("http://127.0.0.1:{port}{CALLBACK_PATH}"),
        })
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Block until the browser comes back, the deadline passes, or the flow is
    /// cancelled. Returns the authorization code.
    pub fn wait(
        &self,
        expected_state: &str,
        deadline: Instant,
        cancel: &Arc<AtomicBool>,
    ) -> Result<String, LoopbackError> {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(LoopbackError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(LoopbackError::TimedOut);
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    // Anything on this port that is not the callback — a
                    // stray probe, a favicon fetch from the same tab — is
                    // answered and ignored, not treated as the response.
                    match handle(stream, expected_state) {
                        Some(outcome) => return outcome,
                        None => continue,
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(POLL);
                }
                Err(err) => return Err(LoopbackError::Bind(err.to_string())),
            }
        }
    }
}

/// `Some` once a request on the callback path has been answered; `None` for
/// anything else on the socket.
fn handle(mut stream: TcpStream, expected_state: &str) -> Option<Result<String, LoopbackError>> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    if BufReader::new(stream.try_clone().ok()?)
        .read_line(&mut line)
        .is_err()
    {
        return None;
    }
    let target = line.split_whitespace().nth(1)?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != CALLBACK_PATH {
        respond(&mut stream, 404, "Not found");
        return None;
    }

    let params = parse_query(query);
    let outcome = match (
        params.iter().find(|(k, _)| k == "code"),
        params.iter().find(|(k, _)| k == "error"),
    ) {
        (_, Some((_, error))) => {
            let description = params
                .iter()
                .find(|(k, _)| k == "error_description")
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| error.clone());
            Err(LoopbackError::Provider(description))
        }
        (Some((_, code)), None) => {
            let state = params.iter().find(|(k, _)| k == "state");
            if state.map(|(_, value)| value.as_str()) == Some(expected_state) {
                Ok(code.clone())
            } else {
                Err(LoopbackError::StateMismatch)
            }
        }
        (None, None) => Err(LoopbackError::Provider(
            "the sign-in response carried no authorization code".into(),
        )),
    };

    match &outcome {
        Ok(_) => respond(
            &mut stream,
            200,
            "JaBot is connected. You can close this tab.",
        ),
        Err(err) => respond(&mut stream, 400, &err.to_string()),
    }
    Some(outcome)
}

fn respond(stream: &mut TcpStream, status: u16, message: &str) {
    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>JaBot</title>\
         <body style=\"font:16px -apple-system,sans-serif;padding:3rem\">{}</body>",
        escape(message)
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&value[i + 1..i + 3], 16) {
                Ok(byte) => {
                    out.push(byte);
                    i += 3;
                }
                Err(_) => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// Drive the listener the way a browser does: a real GET on a real socket.
    fn call(redirect_uri: &str, query: &str) -> String {
        let authority = redirect_uri
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let mut stream = TcpStream::connect(authority).expect("connect to the loopback listener");
        stream
            .write_all(
                format!("GET /callback?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes(),
            )
            .unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
        response
    }

    fn spawn_wait(
        state: &str,
    ) -> (
        String,
        Arc<AtomicBool>,
        std::thread::JoinHandle<Result<String, LoopbackError>>,
    ) {
        let loopback = Loopback::bind().expect("bind");
        let redirect_uri = loopback.redirect_uri().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        let expected = state.to_string();
        let flag = Arc::clone(&cancel);
        let handle = std::thread::spawn(move || {
            loopback.wait(&expected, Instant::now() + Duration::from_secs(10), &flag)
        });
        (redirect_uri, cancel, handle)
    }

    #[test]
    fn redirect_uri_is_loopback_with_an_ephemeral_port() {
        let loopback = Loopback::bind().unwrap();
        let uri = loopback.redirect_uri();
        assert!(uri.starts_with("http://127.0.0.1:"), "{uri}");
        assert!(uri.ends_with("/callback"), "{uri}");
        let port: u16 = uri
            .trim_start_matches("http://127.0.0.1:")
            .trim_end_matches("/callback")
            .parse()
            .expect("a port");
        assert!(port > 0);
    }

    #[test]
    fn a_matching_state_yields_the_code() {
        let (redirect_uri, _cancel, handle) = spawn_wait("state-abc");
        let response = call(&redirect_uri, "code=auth-code-1&state=state-abc");
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert_eq!(handle.join().unwrap().unwrap(), "auth-code-1");
    }

    /// The CSRF check. A response carrying someone else's state must not
    /// become a token exchange, and the browser must be told so.
    #[test]
    fn a_mismatched_state_is_refused() {
        let (redirect_uri, _cancel, handle) = spawn_wait("state-abc");
        let response = call(&redirect_uri, "code=auth-code-1&state=state-forged");
        assert!(response.starts_with("HTTP/1.1 400"), "{response}");
        assert!(matches!(
            handle.join().unwrap(),
            Err(LoopbackError::StateMismatch)
        ));
    }

    #[test]
    fn a_provider_error_keeps_its_description() {
        let (redirect_uri, _cancel, handle) = spawn_wait("state-abc");
        call(
            &redirect_uri,
            "error=access_denied&error_description=You%20said%20no&state=state-abc",
        );
        match handle.join().unwrap() {
            Err(LoopbackError::Provider(detail)) => assert_eq!(detail, "You said no"),
            other => panic!("expected the provider's error, got {other:?}"),
        }
    }

    #[test]
    fn traffic_on_another_path_does_not_end_the_wait() {
        let loopback = Loopback::bind().expect("bind");
        let redirect_uri = loopback.redirect_uri().to_string();
        let authority = redirect_uri
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let handle = std::thread::spawn(move || {
            loopback.wait("state-abc", Instant::now() + Duration::from_secs(10), &flag)
        });

        let mut stray = TcpStream::connect(&authority).unwrap();
        stray
            .write_all(b"GET /favicon.ico HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .unwrap();
        let mut ignored = String::new();
        let _ = stray.read_to_string(&mut ignored);
        assert!(ignored.starts_with("HTTP/1.1 404"), "{ignored}");

        let response = call(&redirect_uri, "code=late-but-real&state=state-abc");
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert_eq!(handle.join().unwrap().unwrap(), "late-but-real");
    }

    #[test]
    fn cancelling_stops_the_wait() {
        let (_redirect_uri, cancel, handle) = spawn_wait("state-abc");
        cancel.store(true, Ordering::Relaxed);
        assert!(matches!(
            handle.join().unwrap(),
            Err(LoopbackError::Cancelled)
        ));
    }

    #[test]
    fn query_parsing_decodes_percent_escapes() {
        let params = parse_query("code=a%2Fb&state=x%20y&empty=");
        assert_eq!(params[0], ("code".into(), "a/b".into()));
        assert_eq!(params[1], ("state".into(), "x y".into()));
        assert_eq!(params[2], ("empty".into(), String::new()));
    }
}
