//! Structured questions and plan decisions (#298): the broker's second face.
//!
//! Cursor's ACP bridge sends two blocking requests that are not permissions —
//! `cursor/ask_question` and `cursor/create_plan` (`acp/extensions.rs`). They
//! have exactly the lifecycle #20 gave a permission: on disk before they are
//! announced, delivered separately from resolved, answered once however many
//! times the button is pressed. So they live in the same `pending_permissions`
//! map and the same `permission_requests` ledger, tagged by [`AskKind`], and
//! this module is the part of the broker that speaks for them.
//!
//! What is deliberately *not* shared is the wire and the policy. A question is
//! listed by `interaction/pending` and settled by `interaction/reply`; it never
//! appears in `permission/pending`, `permission/reply` refuses its id, the fold
//! policy never answers one, and accepting a plan writes nothing a permission
//! check reads. Approving a plan is a decision about what the agent will do,
//! not a grant of what it may do.
//!
//! And two states a permission never enters: `expired` (the turn ended with
//! the ask still open) and `unavailable` (the process that asked is gone). A
//! permission decision is worth recording after the fact — #20 keeps it
//! `pending` across a quit for that reason — but a question's answer is not:
//! Cursor will not ask again, and nothing here replays a dead RPC.

use serde_json::{json, Map, Value};
use uuid::Uuid;

use super::acp::extensions::{self, ExtensionRequest};
use super::backend::InteractionId;
use super::permission::{PendingPermission, HOST_DECIDED};
use super::protocol::error::RpcError;
use super::protocol::methods::{
    AskKind, InteractionOutcome, InteractionPendingParams, InteractionPendingResult,
    InteractionReplyParams, InteractionReplyResult, InteractionView, QuestionAnswer,
    INTERACTION_RESOLVED,
};
use super::store::{
    PermissionRequestRow, ASK_ANSWERED, ASK_CANCELLED, ASK_EXPIRED, ASK_PENDING, ASK_UNAVAILABLE,
};
use super::HostSession;

/// JSON-RPC's code for a method the client does not implement — and the
/// signal Cursor reads before falling back to what it can do without us.
const METHOD_NOT_FOUND: i64 = -32601;

impl HostSession {
    // ---- from the adapter ---------------------------------------------

    /// An agent sent one of the extension requests the host renders. Put it
    /// in front of the human, or refuse it if it cannot be drawn.
    pub(crate) fn open_extension_request(
        &mut self,
        thread_id: &str,
        interaction: InteractionId,
        method: &str,
        params: &Value,
    ) {
        let request = match extensions::parse(method, params) {
            Ok(request) => request,
            Err(err) => {
                // Refused, not cancelled: no human declined anything. The
                // error is the same one an unknown method gets, and it is
                // what lets Cursor fall back to its own permission prompts.
                eprintln!("{thread_id}: {method} refused: {err}");
                self.refuse_extension(thread_id, interaction, method);
                return;
            }
        };
        let request_id = Uuid::new_v4().to_string();
        let ask = request.kind();
        let title = request.title();
        let typed = request.to_json();
        // The transcript row keeps the raw payload beside the typed one: the
        // record and the card are built from the schema, the log keeps what
        // was actually said. Labelled by the ACP method, so a replay knows
        // what it is looking at.
        self.persist_transcript_event(
            thread_id,
            method,
            &json!({
                "requestId": request_id,
                "ask": ask,
                "method": method,
                "toolCallId": request.tool_call_id(),
                "title": title,
                "request": typed,
                "raw": params,
            }),
        );
        let pending = PendingPermission {
            thread_id: thread_id.to_string(),
            interaction,
            title: title.clone(),
            kind: None,
            subject: typed.clone(),
            options: json!([]),
            created_at: super::store::now_utc(),
            ask,
            method: method.to_string(),
        };
        // On disk before it is announced, like every ask (#20).
        self.record_permission_request(&request_id, &pending);
        self.pending_permissions.insert(request_id.clone(), pending);
        self.notify_interaction_ask(thread_id, &request_id, ask, method, &title, typed);
        // The same lifecycle as a permission — the run pauses as Needs you and
        // a folded thread resurfaces — and pointedly not the same policy:
        // nothing on this path ever answers on the user's behalf.
        self.lifecycle_on_permission_pending(thread_id, &json!({ "title": title }));
    }

    fn refuse_extension(&self, thread_id: &str, interaction: InteractionId, method: &str) {
        let Some(conn) = self.conn(thread_id) else {
            return;
        };
        if let Err(err) = conn.respond_error(
            interaction,
            METHOD_NOT_FOUND,
            &format!("Method not found: {method}"),
        ) {
            eprintln!("could not refuse {method} on {thread_id}: {err}");
        }
    }

    // ---- client methods -----------------------------------------------

    /// Settle a question or plan. Idempotent, honest about delivery, and strict
    /// about the answer: one that does not fit what was asked is refused
    /// before anything is taken off the wire, so the card stays answerable.
    pub fn interaction_reply(
        &mut self,
        params: InteractionReplyParams,
    ) -> Result<InteractionReplyResult, RpcError> {
        let request_id = params.request_id.clone();
        let device_id = self.answering_device(&params.device_id)?;
        let record = self.permission_record(&request_id);
        // Already settled: a second click, a cancelled turn, an adapter that
        // died first. A read, and it reports what stands.
        if let Some(row) = record.as_ref().filter(|row| row.state != ASK_PENDING) {
            return Ok(resolved_result(row));
        }
        // Which ask this is, and what it asked: the live half when there is
        // one, the row when the host that took it is gone.
        let (ask, request, thread_id) = match self.pending_permissions.get(&request_id) {
            Some(pending) => (
                pending.ask,
                pending.subject.clone(),
                pending.thread_id.clone(),
            ),
            None => match record.as_ref() {
                Some(row) => (
                    AskKind::parse(&row.ask).ok_or_else(|| {
                        RpcError::Internal(format!(
                            "request {request_id} records an unknown ask kind {}",
                            row.ask
                        ))
                    })?,
                    serde_json::from_str(&row.subject_json).unwrap_or(Value::Null),
                    row.thread_id.clone(),
                ),
                None => {
                    return Err(RpcError::InvalidParams(format!(
                        "unknown interaction requestId {request_id}"
                    )))
                }
            },
        };
        if ask == AskKind::Permission {
            return Err(RpcError::InvalidParams(format!(
                "{request_id} is a permission request; answer it with permission/reply"
            )));
        }
        // Checked against the record before the live entry is claimed.
        let Decision {
            outcome,
            wire,
            answers,
            reason,
        } = decide(ask, &request, &params)?;
        // Removing the live entry is the claim on delivery: exactly one caller
        // finds it there, so the agent is told exactly once (#20).
        let live = self.pending_permissions.remove(&request_id);
        let delivered = match live {
            Some(pending) => self.answer_agent(&thread_id, pending.interaction, wire),
            // The asker is gone. Recorded, and said to be undelivered.
            None => false,
        };
        let claimed = self.settle_interaction(
            &thread_id,
            &request_id,
            ask,
            outcome,
            answers,
            reason,
            &device_id,
            delivered,
        );
        if !claimed {
            if let Some(row) = self
                .permission_record(&request_id)
                .filter(|row| row.state != ASK_PENDING)
            {
                return Ok(resolved_result(&row));
            }
        }
        // The question is answered, so the Needs-you card that carried it
        // stops asking — same narrow clear `permission/reply` does.
        if let Some(store) = self.store.as_ref() {
            if let Err(err) = store.mark_inbox_kind_read(&thread_id, "needs_you") {
                eprintln!("could not clear the needs_you card for {thread_id}: {err}");
            }
        }
        // Only when the agent heard it does the run go back to work.
        if delivered {
            self.lifecycle_on_permission_answered(
                &thread_id,
                outcome == InteractionOutcome::Cancelled,
            );
        }
        Ok(InteractionReplyResult {
            request_id,
            delivered,
            already_answered: false,
            outcome,
            state: state_for(outcome).to_string(),
        })
    }

    /// Every question and plan still waiting on a human, oldest first — the
    /// live ones, and the rows a previous host left `pending` behind.
    pub fn interaction_pending(
        &self,
        params: InteractionPendingParams,
    ) -> Result<InteractionPendingResult, RpcError> {
        let wanted = params.thread_id.as_deref();
        let mut requests: Vec<InteractionView> = self
            .pending_permissions
            .iter()
            .filter(|(_, pending)| pending.ask != AskKind::Permission)
            .filter(|(_, pending)| wanted.is_none_or(|id| pending.thread_id == id))
            .map(|(request_id, pending)| InteractionView {
                request_id: request_id.clone(),
                thread_id: pending.thread_id.clone(),
                ask: pending.ask,
                method: pending.method.clone(),
                title: pending.title.clone(),
                request: pending.subject.clone(),
                created_at: pending.created_at.clone(),
                stale: false,
            })
            .collect();
        if let Some(store) = self.store.as_ref() {
            let rows = store
                .list_open_permission_requests(wanted)
                .map_err(|err| RpcError::Internal(err.to_string()))?;
            for row in rows {
                if self.pending_permissions.contains_key(&row.id) {
                    continue;
                }
                let Some(ask) = AskKind::parse(&row.ask) else {
                    continue;
                };
                if ask == AskKind::Permission {
                    continue;
                }
                requests.push(InteractionView {
                    request_id: row.id,
                    thread_id: row.thread_id,
                    ask,
                    method: row.method.unwrap_or_default(),
                    title: row.title,
                    request: serde_json::from_str(&row.subject_json).unwrap_or(Value::Null),
                    created_at: row.created_at,
                    stale: true,
                });
            }
        }
        requests.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.request_id.cmp(&b.request_id))
        });
        Ok(InteractionPendingResult { requests })
    }

    // ---- the record ---------------------------------------------------

    /// Boot: close every question or plan a previous host left `pending`.
    ///
    /// A clean quit closes them itself (`Withdrawal::Abandoned`). A crash, a
    /// kill, or a daemon stopped mid-ask does not — and the ACP call each of
    /// those rows belonged to died with that process, so they are
    /// `unavailable` by definition. Saying so here, before any client can
    /// ask, is what keeps a reopened thread from being handed buttons that
    /// reach nobody. Permissions are deliberately left alone: #20 brings
    /// those back answerable, because a permission decision is still worth
    /// recording after the fact.
    pub(crate) fn reconcile_dead_interactions(&mut self) {
        let rows = match self
            .store
            .as_ref()
            .map(|store| store.list_open_permission_requests(None))
        {
            Some(Ok(rows)) => rows,
            Some(Err(err)) => {
                eprintln!("could not read the open asks at boot: {err}");
                return;
            }
            None => return,
        };
        for row in rows {
            let Some(ask) = AskKind::parse(&row.ask) else {
                continue;
            };
            if ask == AskKind::Permission || self.pending_permissions.contains_key(&row.id) {
                continue;
            }
            self.settle_interaction(
                &row.thread_id,
                &row.id,
                ask,
                InteractionOutcome::Unavailable,
                None,
                Some("JaBot restarted while the agent was waiting".to_string()),
                HOST_DECIDED,
                false,
            );
        }
    }

    /// Resolve a question or plan, whoever settled it, and say so: the row,
    /// the transcript, and every listening client. `false` when another
    /// resolution landed first, in which case nothing else is written.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn settle_interaction(
        &mut self,
        thread_id: &str,
        request_id: &str,
        ask: AskKind,
        outcome: InteractionOutcome,
        answers: Option<Vec<QuestionAnswer>>,
        reason: Option<String>,
        device_id: &str,
        delivered: bool,
    ) -> bool {
        let answer = answer_record(outcome, &answers, &reason);
        let claimed = self.resolve_permission_record(
            request_id,
            state_for(outcome),
            device_id,
            None,
            delivered,
            Some(&answer.to_string()),
        );
        if !claimed {
            return false;
        }
        let mut event = answer;
        event["requestId"] = json!(request_id);
        event["ask"] = json!(ask);
        event["deviceId"] = json!(device_id);
        event["delivered"] = json!(delivered);
        // The decision is part of the conversation: a reopened thread shows
        // the question and what was decided, not a card with buttons on it.
        self.persist_transcript_event(thread_id, INTERACTION_RESOLVED, &event);
        self.notify_interaction_resolved(
            thread_id, request_id, device_id, outcome, answers, reason, delivered,
        );
        true
    }
}

/// `{ outcome, answers?, reason? }` — the row's `answer_json`, and the seed
/// of the transcript event.
fn answer_record(
    outcome: InteractionOutcome,
    answers: &Option<Vec<QuestionAnswer>>,
    reason: &Option<String>,
) -> Value {
    let mut record = Map::new();
    record.insert("outcome".into(), json!(outcome));
    if let Some(answers) = answers {
        record.insert("answers".into(), json!(answers));
    }
    if let Some(reason) = reason {
        record.insert("reason".into(), json!(reason));
    }
    Value::Object(record)
}

fn state_for(outcome: InteractionOutcome) -> &'static str {
    match outcome {
        InteractionOutcome::Cancelled => ASK_CANCELLED,
        InteractionOutcome::Expired => ASK_EXPIRED,
        InteractionOutcome::Unavailable => ASK_UNAVAILABLE,
        InteractionOutcome::Answered
        | InteractionOutcome::Skipped
        | InteractionOutcome::Accepted
        | InteractionOutcome::Rejected => ASK_ANSWERED,
    }
}

struct Decision {
    outcome: InteractionOutcome,
    /// Exactly what goes back on the wire, in the agent's own shape.
    wire: Value,
    answers: Option<Vec<QuestionAnswer>>,
    reason: Option<String>,
}

/// What a reply means for this ask — or why it is refused.
///
/// The outcome has to fit the kind, and answers have to fit the questions.
/// Cursor reads anything it does not expect as "User cancelled", so the only
/// way to keep a human's choice from being silently turned into a cancel is
/// to refuse the ones that could be.
fn decide(
    ask: AskKind,
    request: &Value,
    params: &InteractionReplyParams,
) -> Result<Decision, RpcError> {
    let reason = params
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .map(str::to_string);
    match (ask, params.outcome) {
        (_, InteractionOutcome::Cancelled) => Ok(Decision {
            outcome: InteractionOutcome::Cancelled,
            wire: extensions::cancelled(),
            answers: None,
            reason,
        }),
        (AskKind::Question, InteractionOutcome::Answered) => {
            let ExtensionRequest::Question(question) = extensions::parse_recorded(ask, request)
                .map_err(|err| {
                    RpcError::Internal(format!("recorded question does not read back: {err}"))
                })?
            else {
                return Err(RpcError::Internal(
                    "recorded question is not a question".into(),
                ));
            };
            let answers = params.answers.clone().unwrap_or_default();
            extensions::check_answers(&question, &answers).map_err(RpcError::InvalidParams)?;
            Ok(Decision {
                outcome: InteractionOutcome::Answered,
                wire: extensions::question_answered(&answers),
                answers: Some(answers),
                reason: None,
            })
        }
        (AskKind::Question, InteractionOutcome::Skipped) => Ok(Decision {
            outcome: InteractionOutcome::Skipped,
            wire: extensions::question_skipped(reason.as_deref()),
            answers: None,
            reason,
        }),
        (AskKind::Plan, InteractionOutcome::Accepted) => Ok(Decision {
            outcome: InteractionOutcome::Accepted,
            wire: extensions::plan_accepted(),
            answers: None,
            reason: None,
        }),
        (AskKind::Plan, InteractionOutcome::Rejected) => Ok(Decision {
            outcome: InteractionOutcome::Rejected,
            wire: extensions::plan_rejected(reason.as_deref()),
            answers: None,
            reason,
        }),
        (kind, outcome) => Err(RpcError::InvalidParams(format!(
            "outcome {} does not apply to a {}",
            outcome.as_str(),
            kind.as_str()
        ))),
    }
}

fn resolved_result(row: &PermissionRequestRow) -> InteractionReplyResult {
    let recorded = row
        .answer_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Value>(json).ok())
        .and_then(|answer| {
            answer
                .get("outcome")
                .and_then(Value::as_str)
                .and_then(InteractionOutcome::parse)
        });
    let outcome = recorded.unwrap_or(match row.state.as_str() {
        ASK_CANCELLED => InteractionOutcome::Cancelled,
        ASK_EXPIRED => InteractionOutcome::Expired,
        ASK_UNAVAILABLE => InteractionOutcome::Unavailable,
        _ => InteractionOutcome::Answered,
    });
    InteractionReplyResult {
        request_id: row.id.clone(),
        delivered: row.delivered,
        already_answered: true,
        outcome,
        state: row.state.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question() -> Value {
        json!({
            "toolCallId": "c1",
            "questions": [
                { "id": "q1", "prompt": "Which?", "options": [
                    { "id": "a", "label": "A" }, { "id": "b", "label": "B" }
                ] }
            ]
        })
    }

    fn reply(outcome: InteractionOutcome) -> InteractionReplyParams {
        InteractionReplyParams {
            request_id: "r".into(),
            device_id: "d".into(),
            outcome,
            answers: None,
            reason: None,
        }
    }

    #[test]
    fn a_question_answer_goes_out_in_cursors_shape_and_nothing_else_fits() {
        let mut answered = reply(InteractionOutcome::Answered);
        answered.answers = Some(vec![QuestionAnswer {
            question_id: "q1".into(),
            selected_option_ids: vec!["b".into()],
        }]);
        let decision = decide(AskKind::Question, &question(), &answered).expect("fits");
        assert_eq!(decision.outcome, InteractionOutcome::Answered);
        assert_eq!(
            decision.wire,
            json!({ "outcome": { "outcome": "answered", "answers": [
                { "questionId": "q1", "selectedOptionIds": ["b"] }
            ] } })
        );

        // An option the agent never offered is refused, not sent.
        answered.answers = Some(vec![QuestionAnswer {
            question_id: "q1".into(),
            selected_option_ids: vec!["zzz".into()],
        }]);
        assert!(decide(AskKind::Question, &question(), &answered).is_err());
        // A plan's verbs do not apply to a question, and vice versa.
        assert!(decide(
            AskKind::Question,
            &question(),
            &reply(InteractionOutcome::Accepted)
        )
        .is_err());
        assert!(decide(
            AskKind::Plan,
            &json!({ "toolCallId": "p" }),
            &reply(InteractionOutcome::Answered)
        )
        .is_err());
        // Cancel fits anything.
        assert_eq!(
            decide(
                AskKind::Plan,
                &json!({ "toolCallId": "p" }),
                &reply(InteractionOutcome::Cancelled)
            )
            .expect("fits")
            .wire,
            json!({ "outcome": { "outcome": "cancelled" } })
        );
    }

    #[test]
    fn a_rejection_carries_its_reason_and_a_skip_can_have_none() {
        let mut rejected = reply(InteractionOutcome::Rejected);
        rejected.reason = Some("  too broad ".into());
        let decision =
            decide(AskKind::Plan, &json!({ "toolCallId": "p" }), &rejected).expect("fits");
        assert_eq!(decision.reason.as_deref(), Some("too broad"));
        assert_eq!(
            decision.wire,
            json!({ "outcome": { "outcome": "rejected", "reason": "too broad" } })
        );
        let skipped = decide(
            AskKind::Question,
            &question(),
            &reply(InteractionOutcome::Skipped),
        )
        .expect("fits");
        assert_eq!(skipped.wire, json!({ "outcome": { "outcome": "skipped" } }));
        assert_eq!(state_for(InteractionOutcome::Skipped), ASK_ANSWERED);
        assert_eq!(state_for(InteractionOutcome::Expired), ASK_EXPIRED);
        assert_eq!(state_for(InteractionOutcome::Unavailable), ASK_UNAVAILABLE);
    }

    #[test]
    fn the_answer_record_omits_what_was_not_given() {
        assert_eq!(
            answer_record(InteractionOutcome::Accepted, &None, &None),
            json!({ "outcome": "accepted" })
        );
        assert_eq!(
            answer_record(InteractionOutcome::Rejected, &None, &Some("no".into())),
            json!({ "outcome": "rejected", "reason": "no" })
        );
    }
}
