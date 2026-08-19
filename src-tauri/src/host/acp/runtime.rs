//! Adapter command snapshot and PATH probe.
//!
//! Doctor / catalog UI is #13. This module only answers "can we spawn this
//! command?" so a missing binary becomes an install hint instead of a crash.

use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::super::protocol::methods::RuntimeSpec;
use super::super::store::HarnessRow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessRuntime {
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub install_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeResult {
    Installed(PathBuf),
    Missing {
        command: String,
        hint: Option<String>,
    },
}

impl HarnessRuntime {
    pub fn from_spec(id: impl Into<String>, spec: &RuntimeSpec) -> Result<Self, String> {
        let command = spec.command.trim();
        if command.is_empty() {
            return Err("runtime.command is required".into());
        }
        Ok(Self {
            id: id.into(),
            command: command.to_string(),
            args: spec.args.clone().unwrap_or_default(),
            env: spec.env.clone().unwrap_or_default(),
            install_hint: spec.install_hint.clone(),
        })
    }

    pub fn from_harness(row: &HarnessRow) -> Result<Self, String> {
        let args = parse_string_array(&row.args_json)?;
        let env = parse_env_object(&row.env_json)?;
        Ok(Self {
            id: row.id.clone(),
            command: row.command.clone(),
            args,
            env,
            install_hint: row.install_hint.clone(),
        })
    }

    pub fn from_runtime_json(id: impl Into<String>, raw: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(raw).map_err(|e| format!("runtime_json: {e}"))?;
        let obj = value
            .as_object()
            .ok_or_else(|| "runtime_json must be an object".to_string())?;
        let command = obj
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if command.is_empty() {
            return Err("runtime_json.command is required".into());
        }
        let args = match obj.get("args") {
            None => Vec::new(),
            Some(v) => json_string_array(v)?,
        };
        let env = match obj.get("env") {
            None => BTreeMap::new(),
            Some(v) => json_env_object(v)?,
        };
        let install_hint = obj
            .get("installHint")
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(Self {
            id: id.into(),
            command: command.to_string(),
            args,
            env,
            install_hint,
        })
    }

    pub fn probe(&self) -> ProbeResult {
        match find_in_path(&self.command) {
            Some(path) => ProbeResult::Installed(path),
            None => ProbeResult::Missing {
                command: self.command.clone(),
                hint: self.install_hint.clone(),
            },
        }
    }
}

pub fn find_in_path(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    let as_path = Path::new(command);
    if as_path.components().count() > 1 || command.contains('/') || command.contains('\\') {
        return as_path.exists().then(|| as_path.to_path_buf());
    }
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(command);
        if is_executable(&candidate) {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{command}.exe"));
            if is_executable(&exe) {
                return Some(exe);
            }
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn parse_string_array(raw: &str) -> Result<Vec<String>, String> {
    let value: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    json_string_array(&value)
}

fn parse_env_object(raw: &str) -> Result<BTreeMap<String, String>, String> {
    let value: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    json_env_object(&value)
}

fn json_string_array(value: &Value) -> Result<Vec<String>, String> {
    let arr = value
        .as_array()
        .ok_or_else(|| "args must be an array of strings".to_string())?;
    arr.iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| "args must be an array of strings".to_string())
        })
        .collect()
}

fn json_env_object(value: &Value) -> Result<BTreeMap<String, String>, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "env must be an object".to_string())?;
    let mut env = BTreeMap::new();
    for (k, v) in obj {
        let val = v
            .as_str()
            .ok_or_else(|| format!("env.{k} must be a string"))?;
        env.insert(k.clone(), val.to_string());
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_finds_sh() {
        let runtime = HarnessRuntime {
            id: "sh".into(),
            command: "sh".into(),
            args: vec![],
            env: BTreeMap::new(),
            install_hint: None,
        };
        match runtime.probe() {
            ProbeResult::Installed(path) => {
                assert!(path.ends_with("sh"), "{path:?}");
            }
            other => panic!("expected installed, got {other:?}"),
        }
    }

    #[test]
    fn probe_missing_is_not_a_crash() {
        let runtime = HarnessRuntime {
            id: "nope".into(),
            command: "jabot-definitely-not-on-path-xyz".into(),
            args: vec![],
            env: BTreeMap::new(),
            install_hint: Some("brew install nope".into()),
        };
        match runtime.probe() {
            ProbeResult::Missing { command, hint } => {
                assert_eq!(command, "jabot-definitely-not-on-path-xyz");
                assert_eq!(hint.as_deref(), Some("brew install nope"));
            }
            other => panic!("expected missing, got {other:?}"),
        }
    }

    #[test]
    fn runtime_json_roundtrip() {
        let raw = r#"{"command":"claude-agent-acp","args":["--foo"],"env":{"ACP_DEBUG":"1"}}"#;
        let runtime = HarnessRuntime::from_runtime_json("claude", raw).unwrap();
        assert_eq!(runtime.command, "claude-agent-acp");
        assert_eq!(runtime.args, vec!["--foo"]);
        assert_eq!(runtime.env.get("ACP_DEBUG").unwrap(), "1");
    }
}
