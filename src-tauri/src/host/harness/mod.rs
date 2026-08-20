//! Harness catalog and Doctor (#13).
//!
//! Every bot is an ACP harness session (decision #6), so this is the list the
//! New Chat picker, the crew editor, and the thread spawner all read from. It
//! has three tiers — compiled-in cards, compiled-in presets, and user JSON —
//! and one readiness story that applies to all of them, because the interesting
//! failures (logged out, daemon down, adapter missing) are not tier-specific.
//!
//! The host never installs anything. A card can say how to install a harness
//! and link to instructions; it cannot run an installer, from any tier.

pub mod catalog;
pub mod custom;
pub mod doctor;
pub mod path;

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::protocol::error::RpcError;
use super::protocol::methods::{
    CatalogIssue, HarnessDoctorParams, HarnessDoctorResult, HarnessListResult, HarnessReport,
    HarnessStatus, RuntimeSpec,
};
use super::HostSession;
use catalog::HarnessDescriptor;
use doctor::{Diagnosis, SystemProbe};

/// The ACP major version the host speaks (`initialize` in `acp/connection.rs`).
/// An adapter that answers with less than this cannot run a JaBot session.
const REQUIRED_ACP_VERSION: u64 = 1;

/// Resolve a command against the augmented PATH.
///
/// Everything that asks "is this harness here?" goes through one function, so
/// the Doctor and the spawner can never disagree about what is installed.
pub fn resolve_command(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    let as_path = std::path::Path::new(command);
    if as_path.components().count() > 1 {
        return as_path.is_file().then(|| as_path.to_path_buf());
    }
    for dir in path::search_path() {
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

fn is_executable(path: &std::path::Path) -> bool {
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

/// Catalog env is a **floor**, not an override: a value the user already
/// exported wins, so someone debugging Hermes can set
/// `HERMES_ACP_SKIP_CONFIGURED_MCP=0` in their shell and have it stick. `PATH`
/// is the exception — a harness that names its own PATH means it, and ours is
/// only a default for the GUI-launch case.
pub(crate) fn floor_env(
    env: &BTreeMap<String, String>,
    already_set: impl Fn(&str) -> bool,
) -> Vec<(&str, &str)> {
    env.iter()
        .filter(|(key, _)| key.as_str() == "PATH" || !already_set(key))
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

impl HostSession {
    /// Where tier-3 JSON lives. `None` for an ephemeral host: without a data
    /// directory there is nowhere for a user file to have been put.
    pub(crate) fn custom_harness_dir(&self) -> Option<&std::path::Path> {
        self.custom_harness_dir.as_deref()
    }

    /// Mirror the catalog into `harnesses` so a thread can reference any of it.
    ///
    /// `threads.harness_id` is a foreign key, so a custom harness has to exist
    /// as a row before New Chat can open a thread on it. Rows are never
    /// deleted here: a file the user removed still has threads pointing at it,
    /// and dropping the row would take their history with it. It simply stops
    /// appearing in the catalog.
    pub(crate) fn sync_harness_catalog(&mut self) {
        let (descriptors, _) = self.harness_catalog_with_issues();
        let Some(store) = &self.store else { return };
        for descriptor in descriptors
            .iter()
            .filter(|d| d.tier == super::protocol::methods::HarnessTier::Custom)
        {
            let launch = descriptor.primary();
            if let Err(err) = store.upsert_custom_harness(
                &descriptor.id,
                &descriptor.label,
                &launch.command,
                &launch.args,
                &descriptor.env,
                descriptor.install_hint.as_deref(),
            ) {
                eprintln!("failed to register custom harness {}: {err}", descriptor.id);
            }
        }
    }

    /// Compiled-in tiers plus whatever tier-3 files parse today, and what was
    /// wrong with the ones that did not.
    ///
    /// Re-read on every call rather than cached: dropping a file in
    /// `custom_harnesses/` and hitting refresh is the whole tier-3 workflow,
    /// and the read is a handful of small files.
    fn harness_catalog_with_issues(&self) -> (Vec<HarnessDescriptor>, Vec<CatalogIssue>) {
        let mut descriptors = catalog::compiled_in();
        let mut issues = Vec::new();
        if let Some(dir) = self.custom_harness_dir() {
            let (loaded, found) = custom::load_dir(dir);
            issues = found;
            for entry in loaded {
                for warning in entry.warnings {
                    issues.push(CatalogIssue {
                        file: format!("{}.json", entry.descriptor.id),
                        reason: warning,
                    });
                }
                descriptors.push(entry.descriptor);
            }
        }
        (descriptors, issues)
    }

    /// The runtime the catalog would spawn for an id today, if it knows the id.
    ///
    /// Used by `thread/open` to snapshot `runtime_json`, so a thread records
    /// the launch that resolves on this machine rather than the first name in
    /// the catalog table.
    pub(crate) fn catalog_runtime_spec(&self, harness_id: &str) -> Option<RuntimeSpec> {
        let (descriptors, _) = self.harness_catalog_with_issues();
        resolved_runtime_spec(&descriptors, harness_id)
    }

    /// The picker's list. Cheap on purpose — no probing, so opening New Chat
    /// never waits on a vendor CLI. Readiness is `harness/doctor`.
    pub fn harness_list(&mut self) -> Result<HarnessListResult, RpcError> {
        let (descriptors, issues) = self.harness_catalog_with_issues();
        self.sync_harness_catalog();
        Ok(HarnessListResult {
            harnesses: descriptors.iter().map(HarnessDescriptor::card).collect(),
            issues,
        })
    }

    /// Why each harness is or is not ready.
    pub fn harness_doctor(
        &mut self,
        params: HarnessDoctorParams,
    ) -> Result<HarnessDoctorResult, RpcError> {
        let (all, issues) = self.harness_catalog_with_issues();
        let descriptors: Vec<HarnessDescriptor> = match params.harness_id.as_deref() {
            None => all,
            Some(id) => {
                let found: Vec<_> = all.into_iter().filter(|d| d.id == id).collect();
                if found.is_empty() {
                    return Err(RpcError::InvalidParams(format!("unknown harness {id}")));
                }
                found
            }
        };

        let probe = SystemProbe;
        let mut diagnoses = doctor::diagnose_all(&descriptors, &probe);
        if params.deep.unwrap_or(false) {
            self.deep_probe(&descriptors, &mut diagnoses);
        }

        let reports = descriptors
            .iter()
            .zip(diagnoses)
            .map(|(descriptor, diagnosis)| report(descriptor, diagnosis))
            .collect();
        Ok(HarnessDoctorResult {
            reports,
            issues,
            // The PATH is part of the diagnosis: "works in my terminal" is
            // usually a PATH the app never inherited, and the only way for a
            // user to see ours is for us to show it.
            path: path::search_path()
                .iter()
                .map(|dir| dir.display().to_string())
                .collect(),
        })
    }

    /// Spawn each ready adapter and run the ACP handshake.
    ///
    /// This is the only honest source of "adapter outdated": a binary being
    /// present says nothing about which protocol it speaks, and the version it
    /// prints is its own, not ACP's. `initialize` is the question, and the
    /// answer is the number the session would have to run on.
    fn deep_probe(&self, descriptors: &[HarnessDescriptor], diagnoses: &mut [Diagnosis]) {
        let targets: Vec<(usize, RuntimeSpec, PathBuf)> = descriptors
            .iter()
            .zip(diagnoses.iter())
            .enumerate()
            .filter(|(_, (_, diagnosis))| diagnosis.ready())
            .filter_map(|(index, (descriptor, diagnosis))| {
                let launch = diagnosis.launch.as_ref()?;
                let mut spec = descriptor.runtime_spec(launch);
                // Spawn the binary the probe resolved, not the bare name: the
                // child gets our PATH anyway, but this keeps the thing we
                // tested and the thing we ran identical.
                if let Some(resolved) = &diagnosis.resolved_path {
                    spec.command = resolved.display().to_string();
                }
                Some((
                    index,
                    spec,
                    self.log_dir
                        .join(format!("doctor-{}.stderr.log", descriptor.id)),
                ))
            })
            .collect();
        if targets.is_empty() {
            return;
        }

        let wake = self.adapter_wake();
        let outcomes: Vec<(usize, Result<u64, String>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = targets
                .iter()
                .map(|(index, spec, log_path)| {
                    let wake = std::sync::Arc::clone(&wake);
                    scope.spawn(move || (*index, handshake(spec, log_path, wake)))
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .collect()
        });

        for (index, outcome) in outcomes {
            let Some(diagnosis) = diagnoses.get_mut(index) else {
                continue;
            };
            match outcome {
                Ok(version) if version >= REQUIRED_ACP_VERSION => {
                    diagnosis.detail = format!("{} Speaks ACP v{version}.", diagnosis.detail);
                }
                Ok(version) => {
                    diagnosis.status = HarnessStatus::AdapterOutdated;
                    diagnosis.detail = format!(
                        "the adapter answered ACP v{version}; JaBot needs v{REQUIRED_ACP_VERSION}."
                    );
                    diagnosis.remedy = Some("Update the ACP adapter.".into());
                }
                Err(err) => {
                    diagnosis.status = HarnessStatus::Unknown;
                    diagnosis.detail = format!("the adapter did not complete a handshake: {err}");
                }
            }
        }
    }
}

/// One `initialize` round trip against a throwaway adapter process.
fn handshake(
    spec: &RuntimeSpec,
    log_path: &std::path::Path,
    wake: std::sync::Arc<super::acp::AdapterWake>,
) -> Result<u64, String> {
    let runtime = super::acp::HarnessRuntime::from_spec("doctor", spec)?;
    let mut connection = super::acp::AcpConnection::spawn(&runtime, None, log_path, wake)
        .map_err(|err| err.to_string())?;
    let result = connection.initialize().map_err(|err| err.to_string());
    // Always reap: a Doctor that leaves five agents running would be worse
    // than one that reports nothing.
    connection.kill();
    let value = result?;
    Ok(value
        .get("protocolVersion")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0))
}

fn report(descriptor: &HarnessDescriptor, diagnosis: Diagnosis) -> HarnessReport {
    HarnessReport {
        id: descriptor.id.clone(),
        label: descriptor.label.clone(),
        tier: descriptor.tier,
        status: diagnosis.status,
        ready: diagnosis.status == HarnessStatus::Ready,
        detail: diagnosis.detail,
        remedy: diagnosis.remedy,
        command: diagnosis
            .resolved_path
            .map(|path| path.display().to_string()),
        args: diagnosis
            .launch
            .map(|launch| launch.args)
            .unwrap_or_default(),
        install_hint: descriptor.install_hint.clone(),
        install_url: descriptor.install_url.clone(),
        elapsed_ms: diagnosis.elapsed_ms,
    }
}

/// What `thread/open` should snapshot for a catalog id: the first launch that
/// actually resolves on this machine, with the catalog's env floor.
pub fn resolved_runtime_spec(
    descriptors: &[HarnessDescriptor],
    harness_id: &str,
) -> Option<RuntimeSpec> {
    let descriptor = descriptors.iter().find(|d| d.id == harness_id)?;
    let launch = descriptor
        .launches
        .iter()
        .find(|launch| resolve_command(&launch.command).is_some())
        .unwrap_or_else(|| descriptor.primary());
    Some(descriptor.runtime_spec(launch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::protocol::jsonrpc::{JsonRpcRequest, RequestId};
    use crate::host::protocol::{HARNESS_DOCTOR, HARNESS_LIST, HOST_HELLO};
    use crate::host::HostSession;
    use serde_json::json;

    fn host_with_custom(files: &[(&str, serde_json::Value)]) -> (HostSession, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("custom_harnesses");
        std::fs::create_dir_all(&custom).unwrap();
        for (name, body) in files {
            std::fs::write(custom.join(name), body.to_string()).unwrap();
        }
        let mut session = HostSession::load(dir.path());
        session
            .handle_request(JsonRpcRequest::new(RequestId::Number(1), HOST_HELLO, None))
            .result
            .expect("hello");
        (session, dir)
    }

    fn call(
        session: &mut HostSession,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> serde_json::Value {
        let response =
            session.handle_request(JsonRpcRequest::new(RequestId::Number(2), method, params));
        response
            .result
            .unwrap_or_else(|| panic!("{method} failed: {:?}", response.error))
    }

    #[test]
    fn the_catalog_carries_all_three_tiers() {
        let (mut session, _dir) = host_with_custom(&[(
            "my-agent.json",
            json!({
                "id": "my-agent",
                "label": "My Agent",
                "command": "my-agent-bin",
                "args": ["acp"],
                "installHint": "Download from example.com"
            }),
        )]);

        let result = call(&mut session, HARNESS_LIST, None);
        let tiers: Vec<(&str, &str)> = result["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|card| (card["id"].as_str().unwrap(), card["tier"].as_str().unwrap()))
            .collect();
        assert!(tiers.contains(&("claude", "shipped")));
        assert!(tiers.contains(&("hermes", "preset")));
        assert!(tiers.contains(&("my-agent", "custom")));
        assert!(result["issues"].as_array().unwrap().is_empty());

        // A custom card is only usable if a thread can name it, and
        // `threads.harness_id` is a foreign key — so the row has to exist.
        let row = session
            .store()
            .unwrap()
            .get_harness("my-agent")
            .unwrap()
            .expect("custom harness row");
        assert_eq!(row.command, "my-agent-bin");
        assert!(!row.is_builtin);
    }

    #[test]
    fn a_broken_custom_file_is_reported_not_swallowed() {
        let (mut session, _dir) = host_with_custom(&[
            (
                "shadow.json",
                json!({ "id": "claude", "label": "Not Claude", "command": "nope" }),
            ),
            (
                "good.json",
                json!({ "id": "good", "label": "Good", "command": "good-acp" }),
            ),
        ]);

        let result = call(&mut session, HARNESS_LIST, None);
        let issues = result["issues"].as_array().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0]["file"], "shadow.json");
        assert!(issues[0]["reason"].as_str().unwrap().contains("reserved"));

        let ids: Vec<&str> = result["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|card| card["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"good"), "one bad file must not hide the rest");
        // The reserved card is still the real one.
        let claude = result["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .find(|card| card["id"] == "claude")
            .unwrap();
        assert_eq!(claude["label"], "Claude Code");
        assert_eq!(claude["reserved"], true);
    }

    #[test]
    fn the_doctor_answers_for_every_card_and_shows_the_path_it_searched() {
        let (mut session, _dir) = host_with_custom(&[]);
        let result = call(&mut session, HARNESS_DOCTOR, None);

        let reports = result["reports"].as_array().unwrap();
        assert_eq!(reports.len(), catalog::compiled_in().len());
        for report in reports {
            // Every card gets a reason, whatever the answer was. A blank
            // detail is the "not installed" non-answer this issue exists to
            // replace.
            assert!(!report["detail"].as_str().unwrap().is_empty(), "{report}");
            assert!(report["status"].is_string());
        }
        assert!(!result["path"].as_array().unwrap().is_empty());
    }

    #[test]
    fn the_doctor_can_be_asked_about_one_card() {
        let (mut session, _dir) = host_with_custom(&[]);
        let result = call(
            &mut session,
            HARNESS_DOCTOR,
            Some(json!({ "harnessId": "codex" })),
        );
        assert_eq!(result["reports"].as_array().unwrap().len(), 1);
        assert_eq!(result["reports"][0]["id"], "codex");

        let unknown = session.handle_request(JsonRpcRequest::new(
            RequestId::Number(3),
            HARNESS_DOCTOR,
            Some(json!({ "harnessId": "nope" })),
        ));
        assert!(unknown.error.is_some());
    }

    #[test]
    fn resolve_finds_a_binary_on_the_augmented_path() {
        assert!(resolve_command("sh").is_some());
        assert!(resolve_command("jabot-definitely-not-on-path-xyz").is_none());
        assert!(resolve_command("  ").is_none());
    }

    #[test]
    fn an_absolute_path_is_taken_at_its_word() {
        assert!(resolve_command("/bin/sh").is_some());
        assert!(resolve_command("/bin/definitely-not-here").is_none());
    }

    /// The floor exists so a user can override policy from their own shell.
    #[test]
    fn floor_yields_to_an_exported_value_but_not_for_path() {
        let env = BTreeMap::from([
            (
                "HERMES_ACP_SKIP_CONFIGURED_MCP".to_string(),
                "1".to_string(),
            ),
            ("QUIET".to_string(), "1".to_string()),
            ("PATH".to_string(), "/custom/bin".to_string()),
        ]);
        let applied = floor_env(&env, |key| {
            key == "HERMES_ACP_SKIP_CONFIGURED_MCP" || key == "PATH"
        });
        assert_eq!(applied, [("PATH", "/custom/bin"), ("QUIET", "1")]);
    }

    #[test]
    fn resolved_runtime_prefers_a_launch_that_exists() {
        let mut descriptors = catalog::compiled_in();
        let claude = descriptors.iter_mut().find(|d| d.id == "claude").unwrap();
        // First candidate is missing, second is a binary every machine has.
        claude.launches[1] = catalog::Launch {
            command: "sh".into(),
            args: vec![],
            downloads_on_first_run: false,
        };
        let spec = resolved_runtime_spec(&descriptors, "claude").unwrap();
        assert_eq!(spec.command, "sh");
        assert!(resolved_runtime_spec(&descriptors, "nope").is_none());
    }

    #[test]
    fn a_catalog_with_no_resolvable_launch_still_yields_the_primary() {
        let descriptors = catalog::compiled_in();
        let spec = resolved_runtime_spec(&descriptors, "codex").unwrap();
        assert_eq!(spec.command, "codex-acp");
    }
}
