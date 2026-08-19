//! Durable host and local-device identity.
//!
//! Pairing (#19) is MVP2. The protocol still carries `deviceId` in MVP1 so
//! the desktop webview is device #1 rather than a special case.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::protocol::{DeviceInfo, DeviceRole};

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
    pub local_device: DeviceRecord,
}

impl HostIdentity {
    pub fn generate() -> Self {
        let now = timestamp_now();
        Self {
            host_id: Uuid::new_v4().to_string(),
            host_name: default_host_name(),
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
            Ok(data) => serde_json::from_str(&data)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
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
    fn load_or_create_is_stable_across_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("host-identity.json");
        let first = HostIdentity::load_or_create(&path).unwrap();
        let second = HostIdentity::load_or_create(&path).unwrap();
        assert_eq!(first, second);
        assert!(path.exists());
    }
}
