//! `settings/get` and `settings/set` (#26).
//!
//! Three records parked a knob on a settings surface that did not exist —
//! D-006 the stuck backstop's threshold, D-013 a remembered permission scope,
//! D-017 the cron interval — and D-018 said plainly that naming #26 for it had
//! been optimistic, because nothing in that issue created a place to put one.
//!
//! Scoped to the two knobs the code already has. A remembered permission scope
//! has no host support at all, and a pane offering a control that decides
//! nothing would be worse than no pane.

use super::protocol::error::RpcError;
use super::protocol::methods::{SettingsSetParams, SettingsView};
use super::store::{KEY_DEFAULT_FOLD_POLICY, KEY_IDLE_TIMEOUT_MS};
use super::HostSession;

/// Set by the e2e suite on a spawned host, and by a developer. It wins over
/// the stored preference — see `LifecycleState::apply_stored`.
const IDLE_TIMEOUT_ENV: &str = "JABOT_IDLE_TIMEOUT_MS";

impl HostSession {
    /// Every preference, as it is actually in force.
    ///
    /// The idle timeout is read from the live `LifecycleState` rather than
    /// from the row, so a host running under the env var reports the number it
    /// is using. A pane that showed the stored ten minutes while the backstop
    /// fired at two hundred milliseconds would be describing a different
    /// program.
    pub fn settings_get(&mut self) -> Result<SettingsView, RpcError> {
        let store = self.store.as_ref().ok_or(RpcError::StoreUnavailable)?;
        let default_fold_policy = store
            .default_fold_policy()
            .map_err(|err| RpcError::Internal(err.to_string()))?;
        Ok(SettingsView {
            idle_timeout_ms: self.idle_timeout().as_millis() as u64,
            default_fold_policy,
            idle_timeout_from_env: std::env::var(IDLE_TIMEOUT_ENV).is_ok(),
        })
    }

    /// Write what changed, apply it, and answer with the whole view.
    ///
    /// Applying is the half that would be easy to forget: a stored timeout the
    /// running host has not picked up is a preference that takes effect on the
    /// next launch, which is not what pressing Save looks like it does.
    pub fn settings_set(&mut self, params: SettingsSetParams) -> Result<SettingsView, RpcError> {
        params.validate()?;
        {
            let store = self.store.as_ref().ok_or(RpcError::StoreUnavailable)?;
            if let Some(ms) = params.idle_timeout_ms {
                store
                    .set_setting(KEY_IDLE_TIMEOUT_MS, &ms.to_string())
                    .map_err(|err| RpcError::Internal(err.to_string()))?;
            }
            if let Some(policy) = params.default_fold_policy.as_deref() {
                store
                    .set_setting(KEY_DEFAULT_FOLD_POLICY, policy)
                    .map_err(|err| RpcError::Internal(err.to_string()))?;
            }
        }
        // Not while the env var is in force: it wins, and a Save that silently
        // overrode a test's own threshold would be the surprise.
        if let Some(ms) = params.idle_timeout_ms {
            if std::env::var(IDLE_TIMEOUT_ENV).is_err() {
                self.set_idle_timeout(std::time::Duration::from_millis(ms));
            }
        }
        self.settings_get()
    }
}
