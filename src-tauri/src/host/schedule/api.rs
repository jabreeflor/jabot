//! `schedule/*`: the five methods the settings surface calls.
//!
//! Every one of them goes through the same two guards, and both are here
//! rather than in the UI because the UI is not the only client:
//!
//! - **the cron has to parse before the row exists.** A schedule whose spec is
//!   nonsense is a job that silently never runs, so it is refused at the door
//!   with the field that is wrong named in the message.
//! - **the bot has to exist.** A schedule runs *as* somebody (decision #6);
//!   there is no host-owned persona to fall back on, and a job pointed at a
//!   deleted crew member would fire forever into an error.
//!
//! Anything that changes when a schedule next comes due — the cron, the enabled
//! flag — recomputes `next_due_at` in the same call. That column is not a
//! cache: while JaBot is closed it is the *only* record of what was owed, so a
//! stale one would read as an outage that never happened.

use super::super::protocol::error::RpcError;
use super::super::protocol::methods::{
    ScheduleCreateParams, ScheduleFireView, ScheduleListResult, ScheduleRefParams,
    ScheduleRemoveResult, ScheduleRunResult, ScheduleUpdateParams, ScheduleView,
};
use super::super::store::{
    NewSchedule, ScheduleFireRow, SchedulePatch, ScheduleRow, CATCH_UP_ONCE, CATCH_UP_SKIP,
};
use super::super::HostSession;
use super::{to_stamp, CronSpec};

/// How many past occurrences travel with a schedule. Enough to show "it has
/// been running fine all week"; not a log.
const RECENT_FIRES: i64 = 10;

impl HostSession {
    pub fn schedule_list(&mut self) -> Result<ScheduleListResult, RpcError> {
        let store = self.store_or_err()?;
        let rows = store
            .list_schedules()
            .map_err(|err| RpcError::Internal(err.to_string()))?;
        let mut schedules = Vec::with_capacity(rows.len());
        for row in rows {
            schedules.push(self.schedule_view(&row)?);
        }
        Ok(ScheduleListResult { schedules })
    }

    pub fn schedule_create(
        &mut self,
        params: ScheduleCreateParams,
    ) -> Result<ScheduleView, RpcError> {
        let spec = parse_cron(&params.cron)?;
        let catch_up = parse_catch_up(params.catch_up.as_deref())?;
        // Refuses an unknown bot with the same message `crew/thread` uses.
        let bot = self.bot_row(&params.bot_id)?;
        let enabled = params.enabled.unwrap_or(true);
        // From now, never from the past: a schedule created at 10am does not
        // owe this morning's 9am. Catch-up is about the host being *absent*,
        // not about a job that did not exist yet.
        let next_run_at = enabled
            .then(|| spec.next_after(chrono::Local::now()))
            .flatten()
            .map(to_stamp);
        let row = self
            .store_or_err()?
            .insert_schedule(&NewSchedule {
                bot_id: bot.id,
                title: params.name,
                cron: params.cron,
                prompt: params.prompt,
                enabled,
                catch_up: catch_up.to_string(),
                next_run_at,
            })
            .map_err(|err| RpcError::Internal(err.to_string()))?;
        self.schedule_view(&row)
    }

    pub fn schedule_update(
        &mut self,
        params: ScheduleUpdateParams,
    ) -> Result<ScheduleView, RpcError> {
        let current = self.schedule_row(&params.schedule_id)?;
        let cron = params.cron.clone().unwrap_or_else(|| current.cron.clone());
        let spec = parse_cron(&cron)?;
        let catch_up = match params.catch_up.as_deref() {
            Some(raw) => Some(parse_catch_up(Some(raw))?.to_string()),
            None => None,
        };
        let row = self
            .store_or_err()?
            .update_schedule(
                &params.schedule_id,
                &SchedulePatch {
                    title: params.name,
                    cron: params.cron.clone(),
                    prompt: params.prompt,
                    enabled: params.enabled,
                    catch_up,
                },
            )
            .map_err(|err| RpcError::Internal(err.to_string()))?;

        // Re-arm only when the answer could have changed. Editing the prompt
        // must not move a job that is due in ten minutes, and re-enabling a
        // schedule must not hand it the backlog it accrued while it was off —
        // switching it off was the user saying they did not want those.
        let rearm = params.cron.is_some() || params.enabled.is_some();
        if rearm {
            let next = row
                .enabled
                .then(|| spec.next_after(chrono::Local::now()))
                .flatten()
                .map(to_stamp);
            self.store_or_err()?
                .set_schedule_due(&row.id, next.as_deref())
                .map_err(|err| RpcError::Internal(err.to_string()))?;
        }
        let row = self.schedule_row(&params.schedule_id)?;
        self.schedule_view(&row)
    }

    pub fn schedule_remove(
        &mut self,
        params: ScheduleRefParams,
    ) -> Result<ScheduleRemoveResult, RpcError> {
        // Existence first, so removing something that is already gone is an
        // error the user can understand rather than a silent success.
        let row = self.schedule_row(&params.schedule_id)?;
        let removed = self
            .store_or_err()?
            .delete_schedule(&row.id)
            .map_err(|err| RpcError::Internal(err.to_string()))?;
        Ok(ScheduleRemoveResult {
            schedule_id: row.id,
            removed: removed > 0,
        })
    }

    /// Run now. A manual occurrence, stamped with the moment the user asked;
    /// it does not consume or move the schedule's next due time.
    pub fn schedule_run(
        &mut self,
        params: ScheduleRefParams,
    ) -> Result<ScheduleRunResult, RpcError> {
        let row = self.schedule_row(&params.schedule_id)?;
        let fire = self.fire_now(&row)?;
        Ok(ScheduleRunResult {
            schedule_id: row.id,
            fire: fire_view(&fire),
        })
    }

    fn schedule_row(&self, schedule_id: &str) -> Result<ScheduleRow, RpcError> {
        let id = schedule_id.trim();
        if id.is_empty() {
            return Err(RpcError::InvalidParams("scheduleId is required".into()));
        }
        self.store_or_err()?
            .get_schedule(id)
            .map_err(|err| RpcError::Internal(err.to_string()))?
            .ok_or_else(|| RpcError::InvalidParams(format!("no such schedule: {id}")))
    }

    fn schedule_view(&self, row: &ScheduleRow) -> Result<ScheduleView, RpcError> {
        let store = self.store_or_err()?;
        let bot_name = store
            .get_bot(&row.bot_id)
            .ok()
            .flatten()
            .map(|bot| bot.name)
            .unwrap_or_else(|| row.bot_id.clone());
        let recent = store
            .list_fires(&row.id, RECENT_FIRES)
            .map_err(|err| RpcError::Internal(err.to_string()))?;
        Ok(ScheduleView {
            schedule_id: row.id.clone(),
            bot_id: row.bot_id.clone(),
            bot_name,
            name: row.title.clone(),
            cron: row.cron.clone(),
            prompt: row.prompt.clone(),
            enabled: row.enabled,
            catch_up: row.catch_up.clone(),
            next_run_at: row.next_run_at.clone(),
            last_run_at: row.last_run_at.clone(),
            thread_id: row.last_thread_id.clone(),
            last_fire: recent.first().map(fire_view),
            recent_fires: recent.iter().map(fire_view).collect(),
            created_at: row.created_at.clone(),
            updated_at: row.updated_at.clone(),
        })
    }
}

pub(crate) fn fire_view(row: &ScheduleFireRow) -> ScheduleFireView {
    ScheduleFireView {
        fire_id: row.id.clone(),
        schedule_id: row.schedule_id.clone(),
        thread_id: row.thread_id.clone(),
        run_id: row.run_id.clone(),
        due_at: row.due_at.clone(),
        fired_at: row.fired_at.clone(),
        state: row.state.clone(),
        caught_up: row.caught_up,
        skipped_count: row.skipped_count,
        detail: row.detail.clone(),
        delivered_at: row.delivered_at.clone(),
    }
}

/// A cron that does not parse is refused with the reason, not with "invalid".
fn parse_cron(raw: &str) -> Result<CronSpec, RpcError> {
    CronSpec::parse(raw).map_err(|err| RpcError::InvalidParams(err.to_string()))
}

/// The two policies, spelled the way the SQL check constraint spells them.
fn parse_catch_up(raw: Option<&str>) -> Result<&'static str, RpcError> {
    match raw.map(str::trim) {
        None | Some("") | Some("once") => Ok(CATCH_UP_ONCE),
        Some("skip") => Ok(CATCH_UP_SKIP),
        Some(other) => Err(RpcError::InvalidParams(format!(
            "catchUp is once or skip, not {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catch_up_policy_is_one_of_two_words() {
        assert_eq!(parse_catch_up(None).unwrap(), "once");
        assert_eq!(parse_catch_up(Some(" skip ")).unwrap(), "skip");
        // Not defaulted: a client that meant `skip` and typed `never` must
        // find out, rather than get a schedule that catches up.
        assert!(parse_catch_up(Some("never")).is_err());
    }

    #[test]
    fn a_cron_that_does_not_parse_says_which_field_is_wrong() {
        let err = parse_cron("0 99 * * *").unwrap_err();
        assert!(err.to_string().contains("hour"), "{err}");
    }
}
