//! Session receipts and the compatibility fingerprint.
//!
//! When #21 resumes a session it did not spawn, it has to answer one question
//! first: is this still the same job? A session resumed against a different
//! harness, model, cwd, tool allowlist, or permission mode is not the thread
//! the user folded, and silently continuing it is worse than saying so.
//!
//! The receipt is a SQLite row, not a map in the supervisor. Buzz keeps its
//! session table in memory and loses every resume across a restart; that is the
//! bug this module exists to not have.

use crate::host::store::SessionReceiptRow;

/// The five inputs that decide whether a stored session is still usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFingerprint {
    pub harness_id: String,
    pub model: Option<String>,
    pub cwd: String,
    /// MCP catalog ids the bot is allowed to use. Order is not meaningful, so
    /// it is normalised before hashing — a reordered allowlist is not drift.
    pub tools: Vec<String>,
    pub permission_mode: String,
}

/// A field that no longer matches what the session was spawned with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftField {
    Harness,
    Model,
    Cwd,
    Tools,
    PermissionMode,
}

impl DriftField {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Harness => "harnessId",
            Self::Model => "model",
            Self::Cwd => "cwd",
            Self::Tools => "tools",
            Self::PermissionMode => "permissionMode",
        }
    }
}

impl SessionFingerprint {
    pub fn new(
        harness_id: impl Into<String>,
        model: Option<String>,
        cwd: impl Into<String>,
        mut tools: Vec<String>,
        permission_mode: impl Into<String>,
    ) -> Self {
        tools.sort();
        tools.dedup();
        Self {
            harness_id: harness_id.into(),
            model,
            cwd: cwd.into(),
            tools,
            permission_mode: permission_mode.into(),
        }
    }

    /// Tool ids as stored (`tools_json`), so a receipt round-trips exactly.
    pub fn tools_json(&self) -> String {
        serde_json::to_string(&self.tools).unwrap_or_else(|_| "[]".into())
    }

    /// Human-readable, field-tagged, one line per field. Tagging means a cwd of
    /// `a` with model `b` cannot collide with a cwd of `a\nb`.
    pub fn canonical(&self) -> String {
        format!(
            "harness={}\nmodel={}\ncwd={}\ntools={}\npermission={}",
            self.harness_id,
            self.model.as_deref().unwrap_or(""),
            self.cwd,
            self.tools.join(","),
            self.permission_mode
        )
    }

    /// 64-bit FNV-1a of [`Self::canonical`], hex.
    ///
    /// Not a cryptographic digest and does not need to be: nothing here is
    /// adversarial, the inputs are short, and the columns are stored alongside
    /// so a mismatch can name the field rather than shrug at a hash. It buys a
    /// cheap equality check that survives a restart.
    pub fn digest(&self) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in self.canonical().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{hash:016x}")
    }
}

/// What changed between the session on disk and the one we would spawn now.
/// Empty means the receipt is still good.
pub fn drift(stored: &SessionReceiptRow, current: &SessionFingerprint) -> Vec<DriftField> {
    let mut drifted = Vec::new();
    if stored.fingerprint == current.digest() {
        return drifted;
    }
    if stored.harness_id != current.harness_id {
        drifted.push(DriftField::Harness);
    }
    if stored.model.as_deref() != current.model.as_deref() {
        drifted.push(DriftField::Model);
    }
    if stored.cwd != current.cwd {
        drifted.push(DriftField::Cwd);
    }
    if stored.tools_json != current.tools_json() {
        drifted.push(DriftField::Tools);
    }
    if stored.permission_mode != current.permission_mode {
        drifted.push(DriftField::PermissionMode);
    }
    drifted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint() -> SessionFingerprint {
        SessionFingerprint::new(
            "claude",
            Some("sonnet".into()),
            "/repos/app",
            vec!["github".into(), "gmail".into()],
            "default",
        )
    }

    fn receipt_of(fp: &SessionFingerprint) -> SessionReceiptRow {
        SessionReceiptRow {
            thread_id: "t1".into(),
            acp_session_id: "sess-1".into(),
            native_session_ref: None,
            harness_id: fp.harness_id.clone(),
            model: fp.model.clone(),
            cwd: fp.cwd.clone(),
            tools_json: fp.tools_json(),
            permission_mode: fp.permission_mode.clone(),
            fingerprint: fp.digest(),
            created_at: "t".into(),
            updated_at: "t".into(),
        }
    }

    #[test]
    fn tool_order_is_not_drift() {
        let a = fingerprint();
        let b = SessionFingerprint::new(
            "claude",
            Some("sonnet".into()),
            "/repos/app",
            vec!["gmail".into(), "github".into(), "gmail".into()],
            "default",
        );
        assert_eq!(a.digest(), b.digest());
        assert!(drift(&receipt_of(&a), &b).is_empty());
    }

    #[test]
    fn each_field_is_named_when_it_moves() {
        let stored = receipt_of(&fingerprint());

        let moved_cwd = SessionFingerprint {
            cwd: "/repos/other".into(),
            ..fingerprint()
        };
        assert_eq!(drift(&stored, &moved_cwd), vec![DriftField::Cwd]);

        let swapped_harness = SessionFingerprint {
            harness_id: "codex".into(),
            ..fingerprint()
        };
        assert_eq!(drift(&stored, &swapped_harness), vec![DriftField::Harness]);

        let new_tool = SessionFingerprint::new(
            "claude",
            Some("sonnet".into()),
            "/repos/app",
            vec!["github".into(), "gmail".into(), "calendar".into()],
            "default",
        );
        assert_eq!(drift(&stored, &new_tool), vec![DriftField::Tools]);

        let dropped_model = SessionFingerprint {
            model: None,
            ..fingerprint()
        };
        assert_eq!(drift(&stored, &dropped_model), vec![DriftField::Model]);
    }

    #[test]
    fn field_tagging_stops_a_shifted_collision() {
        // Without per-field tags, moving text across the cwd/model boundary
        // would hash the same and drift would go unnoticed.
        let a = SessionFingerprint::new("claude", Some("x".into()), "y", vec![], "default");
        let b = SessionFingerprint::new("claude", None, "x\ncwd=y", vec![], "default");
        assert_ne!(a.digest(), b.digest());
    }
}
