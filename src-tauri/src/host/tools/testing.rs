//! A real authorization server on loopback, for the tests.
//!
//! The OAuth flow cannot be exercised against Google or Slack from a test —
//! there is no registered client and no human to click Allow — but everything
//! between JaBot and the provider is protocol, and protocol can be answered.
//! This is a small HTTP/1.1 server that speaks the same four documents a real
//! one does: protected-resource metadata, authorization-server metadata,
//! dynamic client registration, and the token endpoint.
//!
//! It **checks** rather than rubber-stamps. The token endpoint recomputes
//! `S256(code_verifier)` and compares it against the challenge from the
//! authorize request, so a test that passes proves JaBot's PKCE is right, not
//! that a mock was told to say yes.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::crypto::{base64url, sha256};
use super::http::{CurlClient, HttpClient};

const ACCESS_TOKEN: &str = "local-access-token-1";
const CODE: &str = "local-auth-code-1";

#[derive(Debug, Default)]
struct Pending {
    challenge: Option<String>,
    redirect_uri: Option<String>,
    state: Option<String>,
    client_id: Option<String>,
}

#[derive(Debug)]
pub struct LocalAuthServer {
    origin: String,
    pending: Arc<Mutex<Pending>>,
    dynamic_registration: Arc<AtomicBool>,
    corrupt_verifier: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl LocalAuthServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("bind local authorization server");
        listener.set_nonblocking(true).expect("nonblocking");
        let origin = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

        let server = Self {
            origin: origin.clone(),
            pending: Arc::new(Mutex::new(Pending::default())),
            dynamic_registration: Arc::new(AtomicBool::new(true)),
            corrupt_verifier: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
        };

        let pending = Arc::clone(&server.pending);
        let dcr = Arc::clone(&server.dynamic_registration);
        let corrupt = Arc::clone(&server.corrupt_verifier);
        let stop = Arc::clone(&server.stop);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream, &origin, &pending, &dcr, &corrupt),
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
        });
        server
    }

    pub fn mcp_url(&self) -> String {
        format!("{}/mcp", self.origin)
    }

    pub fn authorize_endpoint(&self) -> String {
        format!("{}/authorize", self.origin)
    }

    pub fn issued_access_token(&self) -> &'static str {
        ACCESS_TOKEN
    }

    pub fn disable_dynamic_registration(&self) {
        self.dynamic_registration.store(false, Ordering::Relaxed);
    }

    /// Make the token endpoint compare against a challenge JaBot never sent,
    /// so a passing PKCE check cannot be a coincidence.
    pub fn corrupt_next_verifier(&self) {
        self.corrupt_verifier.store(true, Ordering::Relaxed);
    }

    /// Stand in for the browser: follow the authorize URL and then the
    /// redirect it answers with, which is what delivers the code to JaBot's
    /// loopback listener.
    pub fn consent(&self, authorize_url: &str) {
        let http = CurlClient;
        let response = http.get(authorize_url, &[]).expect("authorize");
        let location = response
            .header("Location")
            .unwrap_or_else(|| panic!("authorize did not redirect: {}", response.status))
            .to_string();
        let _ = http.get(&location, &[]);
    }
}

impl Drop for LocalAuthServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(
    mut stream: TcpStream,
    origin: &str,
    pending: &Arc<Mutex<Pending>>,
    dcr: &Arc<AtomicBool>,
    corrupt: &Arc<AtomicBool>,
) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(clone) => clone,
        Err(_) => return,
    });
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut parts = request_line.split_whitespace();
    let (_method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));

    let mut length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; length];
    if length > 0 && reader.read_exact(&mut body).is_err() {
        return;
    }
    let body = String::from_utf8_lossy(&body).into_owned();

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match path {
        "/mcp" => {
            let challenge = format!(
                "Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\""
            );
            write_response(
                &mut stream,
                401,
                &[("WWW-Authenticate", challenge.as_str())],
                "",
            );
        }
        "/.well-known/oauth-protected-resource/mcp" => write_json(
            &mut stream,
            json!({ "resource": format!("{origin}/mcp"), "authorization_servers": [origin] }),
        ),
        "/.well-known/oauth-authorization-server" => {
            let mut metadata = json!({
                "issuer": origin,
                "authorization_endpoint": format!("{origin}/authorize"),
                "token_endpoint": format!("{origin}/token"),
                "code_challenge_methods_supported": ["S256"],
            });
            if dcr.load(Ordering::Relaxed) {
                metadata["registration_endpoint"] = json!(format!("{origin}/register"));
            }
            write_json(&mut stream, metadata);
        }
        "/register" => write_json(&mut stream, json!({ "client_id": "dcr-client-1" })),
        "/authorize" => {
            let params = parse_query(query);
            let redirect_uri = param(&params, "redirect_uri").unwrap_or_default();
            let state = param(&params, "state").unwrap_or_default();
            {
                let mut slot = pending.lock().expect("pending");
                slot.challenge = param(&params, "code_challenge");
                slot.redirect_uri = Some(redirect_uri.clone());
                slot.state = Some(state.clone());
                slot.client_id = param(&params, "client_id");
            }
            let location = format!("{redirect_uri}?code={CODE}&state={state}");
            write_response(&mut stream, 302, &[("Location", location.as_str())], "");
        }
        "/token" => {
            let form = parse_query(&body);
            let expected = {
                let slot = pending.lock().expect("pending");
                slot.challenge.clone()
            };
            let verifier = param(&form, "code_verifier").unwrap_or_default();
            let mut computed = base64url(&sha256(verifier.as_bytes()));
            if corrupt.load(Ordering::Relaxed) {
                computed.push_str("-tampered");
            }
            if param(&form, "grant_type").as_deref() == Some("authorization_code")
                && param(&form, "code").as_deref() != Some(CODE)
            {
                write_error(&mut stream, "invalid_grant", "unknown authorization code");
                return;
            }
            if param(&form, "grant_type").as_deref() == Some("authorization_code")
                && expected.as_deref() != Some(computed.as_str())
            {
                write_error(&mut stream, "invalid_grant", "code verifier does not match");
                return;
            }
            let id_token = format!(
                "header.{}.signature",
                base64url(br#"{"email":"you@example.com"}"#)
            );
            write_json(
                &mut stream,
                json!({
                    "access_token": ACCESS_TOKEN,
                    "refresh_token": "refresh-1",
                    "token_type": "Bearer",
                    "expires_in": 3600,
                    "id_token": id_token,
                }),
            );
        }
        _ => write_response(&mut stream, 404, &[], ""),
    }
}

fn write_json(stream: &mut TcpStream, value: Value) {
    write_response(
        stream,
        200,
        &[("Content-Type", "application/json")],
        &value.to_string(),
    );
}

fn write_error(stream: &mut TcpStream, error: &str, description: &str) {
    write_response(
        stream,
        400,
        &[("Content-Type", "application/json")],
        &json!({ "error": error, "error_description": description }).to_string(),
    );
}

fn write_response(stream: &mut TcpStream, status: u16, headers: &[(&str, &str)], body: &str) {
    let mut response = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\n", body.len());
    for (name, value) in headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("Connection: close\r\n\r\n");
    response.push_str(body);
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn parse_query(raw: &str) -> Vec<(String, String)> {
    raw.split('&')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((decode(key), decode(value)))
        })
        .collect()
}

fn param(params: &[(String, String)], key: &str) -> Option<String> {
    params
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
}

fn decode(value: &str) -> String {
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
