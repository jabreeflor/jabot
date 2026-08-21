//! Durable host and local-device identity.
//!
//! The protocol carries `deviceId` from MVP1 so the desktop webview is device
//! #1 rather than a special case; #19 makes that literal by giving the host
//! long-term key material of its own, so a second device has something stable
//! to be told about and to check on every later connection.
//!
//! `host_key` is 256 bits generated once and kept beside the host id. It is
//! never sent anywhere: what travels is [`HostIdentity::host_fingerprint`], a
//! commitment to it. Two properties follow, and both are the reason the field
//! exists rather than the fingerprint being stored directly.
//!
//! **A reinstall is visible.** `pairing-security-mobile.md` says a host key
//! that changes must scream rather than be silently accepted, because silent
//! replacement is what a man in the middle looks like. Regenerating identity
//! regenerates the key, which changes the fingerprint, which changes every
//! safety number derived from it.
//!
//! **It is a commitment, not a verifying key.** There is no signature scheme
//! in this host (see `host/pairing/mod.rs` on what that costs). The fingerprint
//! binds the pairing transcript to *this* installation; it does not by itself
//! let a device authenticate the host, and nothing here pretends otherwise.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::protocol::{DeviceInfo, DeviceRole};
use super::tools::crypto::random_token;

/// Domain separator for the host fingerprint, so the same 256 bits hashed for
/// another purpose can never collide with it.
const HOST_FINGERPRINT_DOMAIN: &str = "jabot/host-fingerprint/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    pub device_id: String,
    pub name: String,
    pub role: DeviceRole,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostIdentity {
    pub host_id: String,
    pub host_name: String,
    /// Long-term host key material. Never leaves this file; see the module
    /// docs. `default` so an identity written before #19 loads and is filled
    /// in on the next open rather than being thrown away.
    #[serde(default)]
    pub host_key: String,
    pub local_device: DeviceRecord,
}

impl HostIdentity {
    pub fn generate() -> Self {
        let now = timestamp_now();
        Self {
            host_id: Uuid::new_v4().to_string(),
            host_name: default_host_name(),
            host_key: random_token(),
            local_device: DeviceRecord {
                device_id: Uuid::new_v4().to_string(),
                name: default_host_name(),
                role: DeviceRole::Full,
                created_at: now,
            },
        }
    }

    pub fn load_or_create(path: &Path) -> std::io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(data) => {
                let mut identity: Self = serde_json::from_str(&data)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                // An identity file written before this host had key material.
                // Filling it in and saving is a one-way upgrade: the host id
                // is unchanged, so nothing that referenced this host breaks,
                // and the fingerprint simply comes into existence.
                if identity.host_key.trim().is_empty() {
                    identity.host_key = random_token();
                    identity.save(path)?;
                }
                Ok(identity)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let identity = Self::generate();
                identity.save(path)?;
                Ok(identity)
            }
            Err(err) => Err(err),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        fs::write(path, data)
    }

    /// The public name for this host's key material — what a QR carries and
    /// what both ends fold into the safety number.
    pub fn host_fingerprint(&self) -> String {
        super::pairing::crypto::fingerprint(HOST_FINGERPRINT_DOMAIN, &self.host_key)
    }

    pub fn local_device_info(&self) -> DeviceInfo {
        DeviceInfo {
            device_id: self.local_device.device_id.clone(),
            name: self.local_device.name.clone(),
            role: self.local_device.role,
            created_at: Some(self.local_device.created_at.clone()),
        }
    }
}

fn default_host_name() -> String {
    "This Mac".to_string()
}

fn timestamp_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_has_distinct_host_and_device_ids() {
        let id = HostIdentity::generate();
        assert_ne!(id.host_id, id.local_device.device_id);
        assert_eq!(id.local_device.role, DeviceRole::Full);
        assert!(!id.host_id.is_empty());
    }

    #[test]
    fn a_fingerprint_follows_the_key_and_hides_it() {
        let one = HostIdentity::generate();
        let two = HostIdentity::generate();
        assert_ne!(one.host_fingerprint(), two.host_fingerprint());
        // Stable for a given key: two devices paired a year apart must derive
        // the same safety-number input.
        assert_eq!(one.host_fingerprint(), one.host_fingerprint());
        // A reinstall keeps the name and changes the fingerprint. That is the
        // signal `pairing-security-mobile.md` says must not be silent.
        let mut reinstalled = one.clone();
        reinstalled.host_key = random_token();
        assert_ne!(one.host_fingerprint(), reinstalled.host_fingerprint());
        // And the key itself is not recoverable from what is published.
        assert!(!one.host_fingerprint().contains(&one.host_key));
    }

    /// An identity file from before #19 has no `host_key`. It must load, keep
    /// its host id, and come back with key material — not be discarded, which
    /// would orphan every thread that names this host.
    #[test]
    fn an_identity_written_before_pairing_gains_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("host-identity.json");
        fs::write(
            &path,
            r#"{"hostId":"host-1","hostName":"This Mac","localDevice":{
                 "deviceId":"dev-1","name":"This Mac","role":"full",
                 "createdAt":"2026-01-01T00:00:00.000Z"}}"#,
        )
        .unwrap();

        let identity = HostIdentity::load_or_create(&path).unwrap();
        assert_eq!(identity.host_id, "host-1");
        assert!(!identity.host_key.is_empty());
        // Persisted, so the fingerprint does not change on the next launch.
        let again = HostIdentity::load_or_create(&path).unwrap();
        assert_eq!(again.host_key, identity.host_key);
    }

    #[test]
    fn load_or_create_is_stable_across_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("host-identity.json");
        let first = HostIdentity::load_or_create(&path).unwrap();
        let second = HostIdentity::load_or_create(&path).unwrap();
        assert_eq!(first, second);
        assert!(path.exists());
    }
}
