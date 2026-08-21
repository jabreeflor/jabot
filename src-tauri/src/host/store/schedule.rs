//! Schedules and their fires (#25).
//!
//! Two rules live in this file rather than in the caller, because they are the
//! two a second caller would get wrong.
//!
//! **A fire is unique per occurrence.** `INSERT … ON CONFLICT DO NOTHING` on
//! `(schedule_id, due_at)` is what makes catch-up idempotent: the tick that
//! runs twice, the boot pass that overlaps a live tick, and a user pressing
//! Run now on a schedule that is already due all converge on one row and one
//! run. [`claim_fire`] therefore returns `None` for an occurrence somebody
//! already took, and the caller dispatches nothing.
//!
//! **`next_due_at` only ever moves forward, in the same statement that claims
//! the fire.** Advancing it separately would leave a window in which the host
//! could crash having dispatched work it has no record of owing.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::error::StoreError;
use super::models::{NewSchedule, NewScheduleFire, ScheduleFireRow, SchedulePatch, ScheduleRow};
use super::{map_schedule, map_schedule_fire, now_utc};

const COLUMNS: &str = "id, bot_id, title, cron, prompt, enabled, catch_up, \
     last_run_at, next_run_at, last_thread_id, created_at, updated_at";

const FIRE_COLUMNS: &str = "id, schedule_id, thread_id, run_id, due_at, fired_at, \
     state, caught_up, skipped_count, detail, delivered_at";

/// `schedules.catch_up`: run the most recent missed occurrence, once.
pub const CATCH_UP_ONCE: &str = "once";
/// `schedules.catch_up`: run none of them.
pub const CATCH_UP_SKIP: &str = "skip";

/// `schedule_fires.state` — the prompt reached an agent, no result yet.
pub const FIRE_DISPATCHED: &str = "dispatched";
/// The occurrence was deliberately not run.
pub const FIRE_SKIPPED: &str = "skipped";
/// Nothing could be started: no bot, no harness, no workspace.
pub const FIRE_FAILED: &str = "failed";
/// The run ended and its card is in the Inbox.
pub const FIRE_DELIVERED: &str = "delivered";

pub fn insert_schedule(conn: &Connection, new: &NewSchedule) -> Result<ScheduleRow, StoreError> {
    if new.title.trim().is_empty() {
        return Err(StoreError::invalid("a schedule needs a name"));
    }
    if new.prompt.trim().is_empty() {
        return Err(StoreError::invalid("a schedule needs something to say"));
    }
    let id = Uuid::new_v4().to_string();
    let at = now_utc();
    conn.execute(
        "INSERT INTO schedules (
            id, bot_id, title, cron, prompt, enabled, catch_up,
            last_run_at, next_run_at, last_thread_id, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, NULL, ?9, ?9)",
        params![
            id,
            new.bot_id,
            new.title.trim(),
            new.cron.trim(),
            new.prompt.trim(),
            i64::from(new.enabled),
            new.catch_up,
            new.next_run_at,
            at,
        ],
    )?;
    get_schedule(conn, &id)?.ok_or_else(|| StoreError::NotFound(id))
}

pub fn get_schedule(conn: &Connection, id: &str) -> Result<Option<ScheduleRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM schedules WHERE id = ?1"),
        [id],
        map_schedule,
    )
    .optional()
    .map_err(Into::into)
}

/// Newest last: the list is a settings screen, not a feed, so it reads in the
/// order the user created things.
pub fn list_schedules(conn: &Connection) -> Result<Vec<ScheduleRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM schedules ORDER BY created_at ASC, rowid ASC"
    ))?;
    let rows = stmt
        .query_map([], map_schedule)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Enabled schedules with a due time, soonest first. What the tick walks.
pub fn list_due_schedules(conn: &Connection, now: &str) -> Result<Vec<ScheduleRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM schedules
         WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ?1
         ORDER BY next_run_at ASC, rowid ASC"
    ))?;
    let rows = stmt
        .query_map([now], map_schedule)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn update_schedule(
    conn: &Connection,
    id: &str,
    patch: &SchedulePatch,
) -> Result<ScheduleRow, StoreError> {
    let current = get_schedule(conn, id)?.ok_or_else(|| StoreError::NotFound(id.to_string()))?;
    let title = patch.title.clone().unwrap_or(current.title);
    if title.trim().is_empty() {
        return Err(StoreError::invalid("a schedule needs a name"));
    }
    let prompt = patch.prompt.clone().unwrap_or(current.prompt);
    if prompt.trim().is_empty() {
        return Err(StoreError::invalid("a schedule needs something to say"));
    }
    conn.execute(
        "UPDATE schedules SET title = ?2, cron = ?3, prompt = ?4, enabled = ?5,
            catch_up = ?6, updated_at = ?7 WHERE id = ?1",
        params![
            id,
            title.trim(),
            patch.cron.clone().unwrap_or(current.cron).trim(),
            prompt.trim(),
            i64::from(patch.enabled.unwrap_or(current.enabled)),
            patch.catch_up.clone().unwrap_or(current.catch_up),
            now_utc(),
        ],
    )?;
    get_schedule(conn, id)?.ok_or_else(|| StoreError::NotFound(id.to_string()))
}

/// Move a schedule's claim on the clock. `None` parks it — a disabled schedule
/// owes nothing, and leaving a stale due time behind would make re-enabling it
/// look like an outage.
pub fn set_schedule_due(
    conn: &Connection,
    id: &str,
    next_run_at: Option<&str>,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE schedules SET next_run_at = ?2 WHERE id = ?1",
        params![id, next_run_at],
    )?;
    Ok(())
}

/// Which thread the schedule's last fire landed on. Display only; the durable
/// link between a fire and its work is `schedule_fires.thread_id`.
pub fn set_schedule_thread(conn: &Connection, id: &str, thread_id: &str) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE schedules SET last_thread_id = ?2 WHERE id = ?1",
        params![id, thread_id],
    )?;
    Ok(())
}

pub fn delete_schedule(conn: &Connection, id: &str) -> Result<usize, StoreError> {
    Ok(conn.execute("DELETE FROM schedules WHERE id = ?1", [id])?)
}

/// Take one occurrence, exactly once, and move the schedule's clock past it.
///
/// `None` means another pass already claimed this `due_at` — which is not an
/// error and not something to retry. Both statements run in one transaction so
/// a host that dies between them cannot leave a claimed occurrence still owed.
pub fn claim_fire(
    conn: &Connection,
    new: &NewScheduleFire,
    next_run_at: Option<&str>,
) -> Result<Option<ScheduleFireRow>, StoreError> {
    let id = Uuid::new_v4().to_string();
    let at = now_utc();
    // `unchecked_transaction` for the same reason the overlay uses it: `Store`
    // holds one connection behind `&self`, and the host is the only writer.
    let tx = conn.unchecked_transaction()?;
    let inserted = tx.execute(
        "INSERT INTO schedule_fires (
            id, schedule_id, thread_id, run_id, due_at, fired_at,
            state, caught_up, skipped_count, detail, delivered_at
         ) VALUES (?1, ?2, NULL, NULL, ?3, ?4, ?5, ?6, ?7, ?8, NULL)
         ON CONFLICT (schedule_id, due_at) DO NOTHING",
        params![
            id,
            new.schedule_id,
            new.due_at,
            at,
            new.state,
            i64::from(new.caught_up),
            new.skipped_count,
            new.detail,
        ],
    )?;
    if inserted == 0 {
        tx.rollback()?;
        return Ok(None);
    }
    tx.execute(
        "UPDATE schedules SET next_run_at = ?2, last_run_at = ?3 WHERE id = ?1",
        params![new.schedule_id, next_run_at, at],
    )?;
    tx.commit()?;
    get_fire(conn, &id)
}

pub fn get_fire(conn: &Connection, id: &str) -> Result<Option<ScheduleFireRow>, StoreError> {
    conn.query_row(
        &format!("SELECT {FIRE_COLUMNS} FROM schedule_fires WHERE id = ?1"),
        [id],
        map_schedule_fire,
    )
    .optional()
    .map_err(Into::into)
}

/// Where the work landed. Written after the dispatch attempt, because the row
/// has to exist before there is anything to attach a thread or a run to.
pub fn set_fire_target(
    conn: &Connection,
    id: &str,
    thread_id: Option<&str>,
    run_id: Option<&str>,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE schedule_fires SET thread_id = ?2, run_id = ?3 WHERE id = ?1",
        params![id, thread_id, run_id],
    )?;
    Ok(())
}

pub fn set_fire_state(
    conn: &Connection,
    id: &str,
    state: &str,
    detail: Option<&str>,
    delivered: bool,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE schedule_fires SET state = ?2, detail = ?3,
            delivered_at = CASE WHEN ?4 = 1 THEN ?5 ELSE delivered_at END
         WHERE id = ?1",
        params![id, state, detail, i64::from(delivered), now_utc()],
    )?;
    Ok(())
}

/// Fires whose run has not been reported on yet — what the delivery pass walks.
///
/// Deliberately not filtered by host or by process: a fire dispatched by the
/// host that quit is exactly the one whose card is missing, and the next
/// launch is the only thing that can write it.
pub fn list_undelivered_fires(conn: &Connection) -> Result<Vec<ScheduleFireRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {FIRE_COLUMNS} FROM schedule_fires
         WHERE state = '{FIRE_DISPATCHED}' AND delivered_at IS NULL
         ORDER BY fired_at ASC"
    ))?;
    let rows = stmt
        .query_map([], map_schedule_fire)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The most recent fire of one schedule, for the list view.
pub fn latest_fire(
    conn: &Connection,
    schedule_id: &str,
) -> Result<Option<ScheduleFireRow>, StoreError> {
    conn.query_row(
        &format!(
            "SELECT {FIRE_COLUMNS} FROM schedule_fires WHERE schedule_id = ?1
             ORDER BY fired_at DESC, rowid DESC LIMIT 1"
        ),
        [schedule_id],
        map_schedule_fire,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list_fires(
    conn: &Connection,
    schedule_id: &str,
    limit: i64,
) -> Result<Vec<ScheduleFireRow>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {FIRE_COLUMNS} FROM schedule_fires WHERE schedule_id = ?1
         ORDER BY fired_at DESC, rowid DESC LIMIT ?2"
    ))?;
    let rows = stmt
        .query_map(params![schedule_id, limit], map_schedule_fire)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Does this run already have a card in the Inbox?
///
/// A schedule fire on a *folded* thread resurfaces through #15, which writes
/// the card itself. Asking first is what keeps one finished job from producing
/// two rows in the Inbox.
pub fn run_has_inbox_event(conn: &Connection, run_id: &str) -> Result<bool, StoreError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM inbox_events WHERE run_id = ?1",
        [run_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::store::Store;

    fn open() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("jabot.sqlite")).unwrap();
        (store, dir)
    }

    fn a_schedule(store: &Store, due: &str) -> ScheduleRow {
        store
            .insert_schedule(&NewSchedule {
                bot_id: "writer".into(),
                title: "  Morning triage  ".into(),
                cron: "0 9 * * *".into(),
                prompt: "  Summarise overnight mail.  ".into(),
                enabled: true,
                catch_up: CATCH_UP_ONCE.into(),
                next_run_at: Some(due.into()),
            })
            .expect("insert")
    }

    #[test]
    fn a_schedule_round_trips_and_is_trimmed() {
        let (store, _dir) = open();
        let row = a_schedule(&store, "2026-03-04T09:00:00.000Z");
        assert_eq!(row.title, "Morning triage");
        assert_eq!(row.prompt, "Summarise overnight mail.");
        assert!(row.enabled);
        assert_eq!(row.catch_up, "once");
        assert_eq!(store.get_schedule(&row.id).unwrap().unwrap(), row);
        assert_eq!(store.list_schedules().unwrap(), vec![row]);
    }

    #[test]
    fn a_schedule_without_a_name_or_a_prompt_is_refused() {
        let (store, _dir) = open();
        for (title, prompt) in [("", "do it"), ("Nightly", "   ")] {
            let err = store.insert_schedule(&NewSchedule {
                bot_id: "writer".into(),
                title: title.into(),
                cron: "0 9 * * *".into(),
                prompt: prompt.into(),
                enabled: true,
                catch_up: CATCH_UP_ONCE.into(),
                next_run_at: None,
            });
            assert!(err.is_err(), "{title:?}/{prompt:?} should be refused");
        }
    }

    #[test]
    fn only_enabled_schedules_that_are_actually_due_come_back() {
        let (store, _dir) = open();
        let due = a_schedule(&store, "2026-03-04T09:00:00.000Z");
        let later = a_schedule(&store, "2099-01-01T00:00:00.000Z");
        let off = a_schedule(&store, "2026-01-01T00:00:00.000Z");
        store
            .update_schedule(
                &off.id,
                &SchedulePatch {
                    enabled: Some(false),
                    ..SchedulePatch::default()
                },
            )
            .unwrap();

        let ids: Vec<String> = store
            .list_due_schedules("2026-03-04T09:00:01.000Z")
            .unwrap()
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(ids, vec![due.id]);
        assert!(!ids.contains(&later.id));
    }

    /// The property the whole catch-up story rests on.
    #[test]
    fn one_occurrence_can_only_ever_be_claimed_once() {
        let (store, _dir) = open();
        let row = a_schedule(&store, "2026-03-04T09:00:00.000Z");
        let new = NewScheduleFire {
            schedule_id: row.id.clone(),
            due_at: "2026-03-04T09:00:00.000Z".into(),
            state: FIRE_DISPATCHED.into(),
            caught_up: false,
            skipped_count: 0,
            detail: None,
        };
        let first = store
            .claim_fire(&new, Some("2026-03-05T09:00:00.000Z"))
            .unwrap();
        assert!(first.is_some(), "the first claim takes the occurrence");
        // A second tick, a boot pass overlapping a live tick, a Run now that
        // lands on the same millisecond: all of them find it taken.
        let second = store
            .claim_fire(&new, Some("2026-03-06T09:00:00.000Z"))
            .unwrap();
        assert!(second.is_none(), "the occurrence cannot be claimed twice");

        // …and the losing claim must not have moved the clock either, or two
        // ticks would between them skip a day.
        let after = store.get_schedule(&row.id).unwrap().unwrap();
        assert_eq!(
            after.next_run_at.as_deref(),
            Some("2026-03-05T09:00:00.000Z")
        );
        assert!(after.last_run_at.is_some());
    }

    #[test]
    fn a_fire_records_where_its_work_went_and_how_it_ended() {
        let (store, _dir) = open();
        let row = a_schedule(&store, "2026-03-04T09:00:00.000Z");
        let fire = store
            .claim_fire(
                &NewScheduleFire {
                    schedule_id: row.id.clone(),
                    due_at: "2026-03-04T09:00:00.000Z".into(),
                    state: FIRE_DISPATCHED.into(),
                    caught_up: true,
                    skipped_count: 3,
                    detail: Some("caught up".into()),
                },
                None,
            )
            .unwrap()
            .expect("claimed");
        assert!(fire.caught_up);
        assert_eq!(fire.skipped_count, 3);

        // Undelivered until something says how the run ended.
        assert_eq!(store.list_undelivered_fires().unwrap().len(), 1);
        store
            .set_fire_state(&fire.id, FIRE_DELIVERED, Some("finished"), true)
            .unwrap();
        assert!(store.list_undelivered_fires().unwrap().is_empty());

        let settled = store.get_fire(&fire.id).unwrap().unwrap();
        assert_eq!(settled.state, FIRE_DELIVERED);
        assert!(settled.delivered_at.is_some());
        assert_eq!(store.latest_fire(&row.id).unwrap().unwrap().id, fire.id);
    }

    /// A schedule belongs to a bot: removing the crew member removes the job,
    /// rather than leaving one that fires forever into an error.
    #[test]
    fn removing_the_bot_removes_its_schedules() {
        let (store, _dir) = open();
        let row = a_schedule(&store, "2026-03-04T09:00:00.000Z");
        store.delete_bot("writer").unwrap();
        assert!(store.get_schedule(&row.id).unwrap().is_none());
    }

    #[test]
    fn a_disabled_schedule_parks_its_claim_on_the_clock() {
        let (store, _dir) = open();
        let row = a_schedule(&store, "2026-03-04T09:00:00.000Z");
        store.set_schedule_due(&row.id, None).unwrap();
        let parked = store.get_schedule(&row.id).unwrap().unwrap();
        assert_eq!(parked.next_run_at, None);
        // …and is therefore never due, whatever the wall clock says.
        assert!(store
            .list_due_schedules("2099-01-01T00:00:00.000Z")
            .unwrap()
            .is_empty());
    }
}
