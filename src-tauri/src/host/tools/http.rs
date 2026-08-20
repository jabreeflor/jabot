//! The little HTTP client the OAuth flow needs.
//!
//! JaBot has no HTTP dependency: the only one in the tree is `reqwest`, pulled
//! in by the macOS-only updater plugin, and adding a TLS stack to every target
//! for four requests per grant is not a trade this host wants to make. So the
//! metadata GETs and the token POSTs go through `curl`, which macOS ships and
//! which every CI image has.
//!
//! Two rules make that safe rather than merely convenient:
//!
//! - **Nothing secret goes in argv.** Form bodies — the authorization code,
//!   the refresh token, a client secret — are written to the child's stdin.
//!   `ps` shows the URL and nothing else.
//! - **https, except loopback.** [`require_safe_url`] rejects plaintext to
//!   anywhere but 127.0.0.1/::1/localhost. The loopback exception is what lets
//!   the whole flow be exercised against a local authorization server; it can
//!   never widen to a real provider.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("{0}")]
    Url(String),
    #[error("could not run curl: {0}")]
    Spawn(String),
    #[error("{method} {url} failed: {detail}")]
    Transport {
        method: &'static str,
        url: String,
        detail: String,
    },
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// The requests the OAuth flow makes. A trait so the flow can be read without
/// a process in the way — the only implementation that ships is [`CurlClient`].
pub trait HttpClient: Send + Sync + std::fmt::Debug {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, HttpError>;
    fn post_form(&self, url: &str, form: &[(&str, String)]) -> Result<HttpResponse, HttpError>;
    fn post_json(&self, url: &str, body: &str) -> Result<HttpResponse, HttpError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CurlClient;

impl HttpClient for CurlClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse, HttpError> {
        require_safe_url(url)?;
        let mut args = base_args(url);
        for (name, value) in headers {
            args.push("-H".into());
            args.push(format!("{name}: {value}"));
        }
        run("GET", url, &args, None)
    }

    fn post_form(&self, url: &str, form: &[(&str, String)]) -> Result<HttpResponse, HttpError> {
        require_safe_url(url)?;
        let mut args = base_args(url);
        args.push("-X".into());
        args.push("POST".into());
        args.push("-H".into());
        args.push("Content-Type: application/x-www-form-urlencoded".into());
        args.push("--data-binary".into());
        args.push("@-".into());
        run("POST", url, &args, Some(&encode_form(form)))
    }

    fn post_json(&self, url: &str, body: &str) -> Result<HttpResponse, HttpError> {
        require_safe_url(url)?;
        let mut args = base_args(url);
        args.push("-X".into());
        args.push("POST".into());
        args.push("-H".into());
        args.push("Content-Type: application/json".into());
        args.push("--data-binary".into());
        args.push("@-".into());
        run("POST", url, &args, Some(body))
    }
}

fn base_args(url: &str) -> Vec<String> {
    vec![
        "-sS".into(),
        // Headers then body on stdout, so one read gets both.
        "-D".into(),
        "-".into(),
        "-o".into(),
        "-".into(),
        // Redirects are not followed: a 302 away from a token endpoint is a
        // misconfiguration, not something to chase with a bearer in hand.
        "--max-time".into(),
        TIMEOUT.as_secs().to_string(),
        "-H".into(),
        "Accept: application/json".into(),
        // curl adds `Expect: 100-continue` above 1KB, which doubles the
        // round trips and confuses the header split below.
        "-H".into(),
        "Expect:".into(),
        url.into(),
    ]
}

fn run(
    method: &'static str,
    url: &str,
    args: &[String],
    body: Option<&str>,
) -> Result<HttpResponse, HttpError> {
    let mut child = Command::new("curl")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| HttpError::Spawn(e.to_string()))?;
    {
        let mut stdin = child.stdin.take().expect("stdin piped");
        if let Some(body) = body {
            stdin
                .write_all(body.as_bytes())
                .map_err(|e| HttpError::Spawn(e.to_string()))?;
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|e| HttpError::Spawn(e.to_string()))?;
    if !output.status.success() {
        return Err(HttpError::Transport {
            method,
            url: url.to_string(),
            detail: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    parse_response(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| HttpError::Transport {
        method,
        url: url.to_string(),
        detail: "no HTTP status line in the response".into(),
    })
}

/// Split `-D -` output into the last header block and the body.
///
/// The last block, not the first: a 1xx informational response arrives as its
/// own block ahead of the real one.
fn parse_response(raw: &str) -> Option<HttpResponse> {
    let mut rest = raw;
    loop {
        let (block, body) = split_header_block(rest)?;
        let mut lines = block.lines();
        let status: u16 = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
            .collect();
        rest = body;
        if !(100..200).contains(&status) {
            return Some(HttpResponse {
                status,
                headers,
                body: rest.to_string(),
            });
        }
    }
}

fn split_header_block(raw: &str) -> Option<(&str, &str)> {
    if let Some(index) = raw.find("\r\n\r\n") {
        return Some((&raw[..index], &raw[index + 4..]));
    }
    let index = raw.find("\n\n")?;
    Some((&raw[..index], &raw[index + 2..]))
}

/// Plaintext HTTP is refused unless the host is loopback.
///
/// A token endpoint reached over `http://` on a real network hands the bearer
/// to anyone on the path, so this is a hard stop rather than a warning.
pub fn require_safe_url(url: &str) -> Result<(), HttpError> {
    if url.starts_with("https://") {
        return Ok(());
    }
    if let Some(rest) = url.strip_prefix("http://") {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host = match authority.strip_prefix('[') {
            // IPv6 literal: the port colon is the one after the bracket.
            Some(inner) => inner.split_once(']').map(|(host, _)| host).unwrap_or(inner),
            None => authority.split(':').next().unwrap_or(authority),
        };
        if host == "127.0.0.1" || host == "localhost" || host == "::1" {
            return Ok(());
        }
        return Err(HttpError::Url(format!(
            "refusing plaintext http to {host}: OAuth endpoints must be https"
        )));
    }
    Err(HttpError::Url(format!("unsupported URL scheme: {url}")))
}

pub fn encode_form(form: &[(&str, String)]) -> String {
    form.iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `application/x-www-form-urlencoded` and query-string escaping.
///
/// Unreserved set from RFC 3986 only. Space becomes `%20`, not `+`: it is
/// valid in both contexts, where `+` is valid in only one.
pub fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_is_refused_off_loopback() {
        assert!(require_safe_url("https://accounts.example.com/token").is_ok());
        assert!(require_safe_url("http://127.0.0.1:53119/token").is_ok());
        assert!(require_safe_url("http://localhost/token").is_ok());
        assert!(require_safe_url("http://[::1]:9000/token").is_ok());
        let err = require_safe_url("http://accounts.example.com/token").unwrap_err();
        assert!(err.to_string().contains("must be https"), "{err}");
        assert!(require_safe_url("ftp://example.com").is_err());
        // A loopback-looking prefix on a real host is still a real host.
        assert!(require_safe_url("http://127.0.0.1.evil.example.com/token").is_err());
    }

    #[test]
    fn form_encoding_escapes_reserved_characters() {
        let encoded = encode_form(&[
            ("grant_type", "authorization_code".into()),
            ("code", "a+b/c=d e".into()),
            (
                "scope",
                "https://www.googleapis.com/auth/gmail.compose".into(),
            ),
        ]);
        assert!(encoded.contains("code=a%2Bb%2Fc%3Dd%20e"), "{encoded}");
        assert!(
            encoded.contains("scope=https%3A%2F%2Fwww.googleapis.com"),
            "{encoded}"
        );
        assert!(!encoded.contains('+'));
    }

    #[test]
    fn response_parsing_takes_the_final_header_block() {
        let raw = "HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}";
        let response = parse_response(raw).expect("parsed");
        assert_eq!(response.status, 200);
        assert_eq!(response.header("content-type"), Some("application/json"));
        assert_eq!(response.body, "{\"a\":1}");
    }

    #[test]
    fn www_authenticate_survives_parsing() {
        let raw = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer resource_metadata=\"https://mcp.example.com/.well-known/oauth-protected-resource\"\r\n\r\n";
        let response = parse_response(raw).expect("parsed");
        assert_eq!(response.status, 401);
        assert!(response
            .header("WWW-Authenticate")
            .expect("challenge")
            .contains("resource_metadata="));
        assert!(!response.is_success());
    }
}
