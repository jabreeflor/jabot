//! Host metadata returned by the socket-shaped API.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HostInfo {
    pub version: String,
    pub platform: String,
    pub host_mode: &'static str,
}

pub fn health() -> HostInfo {
    HostInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        host_mode: "in-process",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_returns_in_process_mode() {
        let info = health();
        assert_eq!(info.host_mode, "in-process");
        assert!(!info.version.is_empty());
    }
}
