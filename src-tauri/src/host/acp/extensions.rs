//! Cursor's blocking ACP extensions, typed at the wire (#298).
//!
//! ACP lets an agent send requests the spec does not name
//! (<https://agentclientprotocol.com/protocol/v1/extensibility>). Cursor's
//! CLI uses two that block its turn on a human: `cursor/ask_question` — pick
//! among the options the agent offers — and `cursor/create_plan` — accept or
//! reject a plan before it is carried out. Neither is advertised in
//! `initialize`, and the CLI does not consult any client capability before
//! sending them, so "supported" is a fact about *this* host, declared here.
//!
//! This module is the whole of the extension boundary. It says which methods
//! are rendered, reads a payload into the shape the rest of the host works
//! on, and writes the one answer Cursor accepts for each outcome. The broker
//! (`host/interaction`) holds the typed request; the renderer draws it; no
//! code downstream matches on a method name.
//!
//! Two refusals, both deliberate. A method not in [`SUPPORTED`] is answered
//! JSON-RPC `-32601` by the reader thread — the honest signal, and the one
//! Cursor reads as "this client cannot do that": it then falls back to plain
//! `session/request_permission` prompts for a question and writes the plan
//! file itself for a plan, and the turn never hangs. A supported method whose
//! payload is not the documented shape gets the same answer for the same
//! reason: a card built from fields nobody understood would be a fabricated
//! question, and `cancelled` would tell the agent a human declined something
//! no human ever saw.
//!
//! Shapes verified against `cursor-agent 2026.08.04` — the bundled ACP
//! bridge's `ask-question-handler` and `create-plan-handler`, which match
//! the interfaces published at <https://cursor.com/docs/cli/acp>. The JSON
//! under `fixtures/cursor/` is what it sends and what it accepts back.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::super::protocol::methods::{AskKind, QuestionAnswer};

pub const CURSOR_ASK_QUESTION: &str = "cursor/ask_question";
pub const CURSOR_CREATE_PLAN: &str = "cursor/create_plan";

/// The extension requests this host renders. Every other request an agent
/// sends is method-not-found — see the module docs for why that is the answer
/// rather than a synthetic `cancelled`.
pub const SUPPORTED: &[&str] = &[CURSOR_ASK_QUESTION, CURSOR_CREATE_PLAN];

pub fn is_supported(method: &str) -> bool {
    SUPPORTED.contains(&method)
}

/// One choice the agent offered. `id` is what goes back on the wire; `label`
/// is what the human reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub id: String,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<QuestionOption>,
    /// Several options may be chosen. Absent means exactly one.
    #[serde(default)]
    pub allow_multiple: bool,
}

/// `cursor/ask_question`. Free text is not part of this: the ACP bridge only
/// reads `selectedOptionIds`, so a typed answer would never reach the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskQuestion {
    pub tool_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub questions: Vec<Question>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanTodo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub content: String,
    /// `pending`, `in_progress`, `completed` or `cancelled` from Cursor.
    /// Kept as text: a status this build has not seen is still a status.
    #[serde(default = "pending_status")]
    pub status: String,
}

fn pending_status() -> String {
    "pending".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanPhase {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub todos: Vec<PlanTodo>,
}

/// `cursor/create_plan`. `plan` is the markdown body; the rest is structure
/// the CLI derives from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlan {
    pub tool_call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub todos: Vec<PlanTodo>,
    #[serde(default)]
    pub is_project: bool,
    #[serde(default)]
    pub phases: Vec<PlanPhase>,
}

/// A request the host will put in front of a human.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionRequest {
    Question(AskQuestion),
    Plan(CreatePlan),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionError {
    /// Not a method this host renders.
    Unsupported(String),
    /// A supported method whose payload is not the shape Cursor documents.
    Malformed(String),
}

impl std::fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(method) => write!(f, "unsupported extension {method}"),
            Self::Malformed(why) => write!(f, "malformed extension request: {why}"),
        }
    }
}

/// Read a request off the wire.
pub fn parse(method: &str, params: &Value) -> Result<ExtensionRequest, ExtensionError> {
    match method {
        CURSOR_ASK_QUESTION => parse_question(params),
        CURSOR_CREATE_PLAN => parse_plan(params),
        other => Err(ExtensionError::Unsupported(other.to_string())),
    }
}

/// Re-read a request the broker recorded, by the kind the row says it was.
pub fn parse_recorded(ask: AskKind, request: &Value) -> Result<ExtensionRequest, ExtensionError> {
    match ask {
        AskKind::Question => parse_question(request),
        AskKind::Plan => parse_plan(request),
        AskKind::Permission => Err(ExtensionError::Unsupported(
            "session/request_permission".to_string(),
        )),
    }
}

fn parse_question(params: &Value) -> Result<ExtensionRequest, ExtensionError> {
    let ask: AskQuestion = serde_json::from_value(params.clone())
        .map_err(|err| ExtensionError::Malformed(err.to_string()))?;
    check_question(&ask).map_err(ExtensionError::Malformed)?;
    Ok(ExtensionRequest::Question(ask))
}

/// What a question has to have before a card can be built from it: ids to
/// answer with, prompts to read, and at least one option per question. Ids
/// are the agent's own and are checked for uniqueness, because an answer is
/// addressed by id and a duplicate would make one of them unanswerable.
fn check_question(ask: &AskQuestion) -> Result<(), String> {
    if ask.tool_call_id.trim().is_empty() {
        return Err("toolCallId is empty".into());
    }
    if ask.questions.is_empty() {
        return Err("no questions".into());
    }
    let mut question_ids = HashSet::new();
    for question in &ask.questions {
        if question.id.trim().is_empty() {
            return Err("a question has no id".into());
        }
        if !question_ids.insert(question.id.as_str()) {
            return Err(format!("question {} appears twice", question.id));
        }
        if question.prompt.trim().is_empty() {
            return Err(format!("question {} has no prompt", question.id));
        }
        if question.options.is_empty() {
            return Err(format!("question {} offers no options", question.id));
        }
        let mut option_ids = HashSet::new();
        for option in &question.options {
            if option.id.trim().is_empty() {
                return Err(format!("question {} has an option with no id", question.id));
            }
            if !option_ids.insert(option.id.as_str()) {
                return Err(format!(
                    "question {} offers option {} twice",
                    question.id, option.id
                ));
            }
        }
    }
    Ok(())
}

fn parse_plan(params: &Value) -> Result<ExtensionRequest, ExtensionError> {
    let plan: CreatePlan = serde_json::from_value(params.clone())
        .map_err(|err| ExtensionError::Malformed(err.to_string()))?;
    if plan.tool_call_id.trim().is_empty() {
        return Err(ExtensionError::Malformed("toolCallId is empty".into()));
    }
    Ok(ExtensionRequest::Plan(plan))
}

impl ExtensionRequest {
    pub fn kind(&self) -> AskKind {
        match self {
            Self::Question(_) => AskKind::Question,
            Self::Plan(_) => AskKind::Plan,
        }
    }

    pub fn tool_call_id(&self) -> &str {
        match self {
            Self::Question(ask) => &ask.tool_call_id,
            Self::Plan(plan) => &plan.tool_call_id,
        }
    }

    /// The card's heading, and the Inbox's summary of what needs you.
    ///
    /// The agent's own title or plan name when it gave one; otherwise the
    /// first prompt, or a plain description of the decision. Never a sentence
    /// composed from the body — that would be the host paraphrasing the agent.
    pub fn title(&self) -> String {
        match self {
            Self::Question(ask) => given(ask.title.as_deref())
                .or_else(|| {
                    ask.questions
                        .first()
                        .map(|question| question.prompt.clone())
                })
                .unwrap_or_else(|| "The agent has a question".to_string()),
            Self::Plan(plan) => {
                given(plan.name.as_deref()).unwrap_or_else(|| "Review the plan".to_string())
            }
        }
    }

    /// The request as the record keeps it and the renderer draws it.
    ///
    /// Fields the agent sent that the schema does not name are gone here, on
    /// purpose: the card is built from the schema, and an unknown field is
    /// never turned into UI. The raw payload is what the transcript event
    /// keeps for diagnostics.
    pub fn to_json(&self) -> Value {
        match self {
            Self::Question(ask) => serde_json::to_value(ask).unwrap_or(Value::Null),
            Self::Plan(plan) => serde_json::to_value(plan).unwrap_or(Value::Null),
        }
    }
}

fn given(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// A human's answers, checked against the questions that were asked.
///
/// Every question gets exactly one answer, every answer names options the
/// question offered, and a single-choice question gets one of them. Refusing
/// here — before anything is sent — is what makes an invalid submission leave
/// the card answerable rather than sending Cursor an answer it cannot map.
pub fn check_answers(ask: &AskQuestion, answers: &[QuestionAnswer]) -> Result<(), String> {
    let mut answered = HashSet::new();
    for answer in answers {
        let Some(question) = ask
            .questions
            .iter()
            .find(|question| question.id == answer.question_id)
        else {
            return Err(format!("no question {}", answer.question_id));
        };
        if !answered.insert(question.id.as_str()) {
            return Err(format!("question {} answered twice", question.id));
        }
        if answer.selected_option_ids.is_empty() {
            return Err(format!("question {} has no selection", question.id));
        }
        if !question.allow_multiple && answer.selected_option_ids.len() > 1 {
            return Err(format!("question {} takes one answer", question.id));
        }
        let mut picked = HashSet::new();
        for option_id in &answer.selected_option_ids {
            if !question
                .options
                .iter()
                .any(|option| &option.id == option_id)
            {
                return Err(format!(
                    "question {} has no option {option_id}",
                    question.id
                ));
            }
            if !picked.insert(option_id.as_str()) {
                return Err(format!("option {option_id} selected twice"));
            }
        }
    }
    if let Some(missed) = ask
        .questions
        .iter()
        .find(|question| !answered.contains(question.id.as_str()))
    {
        return Err(format!("question {} was not answered", missed.id));
    }
    Ok(())
}

// ---- what goes back on the wire -------------------------------------------
//
// Each is the exact result Cursor's bridge reads. Anything else — a missing
// `answers`, an outcome it does not know — is read as "User cancelled", so
// there is no room for a shape of our own here.

pub fn question_answered(answers: &[QuestionAnswer]) -> Value {
    json!({ "outcome": { "outcome": "answered", "answers": answers } })
}

pub fn question_skipped(reason: Option<&str>) -> Value {
    json!({ "outcome": with_reason(json!({ "outcome": "skipped" }), reason) })
}

pub fn plan_accepted() -> Value {
    // No `planUri`: Cursor writes its own plan file when the client offers
    // none, and streams where it put it. The host takes no side effect of
    // its own on an accept.
    json!({ "outcome": { "outcome": "accepted" } })
}

pub fn plan_rejected(reason: Option<&str>) -> Value {
    json!({ "outcome": with_reason(json!({ "outcome": "rejected" }), reason) })
}

pub fn cancelled() -> Value {
    json!({ "outcome": { "outcome": "cancelled" } })
}

fn with_reason(mut outcome: Value, reason: Option<&str>) -> Value {
    if let Some(reason) = reason.map(str::trim).filter(|reason| !reason.is_empty()) {
        outcome["reason"] = json!(reason);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let text = match name {
            "ask_question.request" => include_str!("fixtures/cursor/ask_question.request.json"),
            "ask_question.answered" => {
                include_str!("fixtures/cursor/ask_question.answered.json")
            }
            "ask_question.skipped" => include_str!("fixtures/cursor/ask_question.skipped.json"),
            "create_plan.request" => include_str!("fixtures/cursor/create_plan.request.json"),
            "create_plan.accepted" => include_str!("fixtures/cursor/create_plan.accepted.json"),
            "create_plan.rejected" => include_str!("fixtures/cursor/create_plan.rejected.json"),
            "cancelled" => include_str!("fixtures/cursor/cancelled.json"),
            other => panic!("no fixture {other}"),
        };
        serde_json::from_str(text).expect("fixture parses")
    }

    fn question() -> AskQuestion {
        match parse(CURSOR_ASK_QUESTION, &fixture("ask_question.request")).expect("parses") {
            ExtensionRequest::Question(ask) => ask,
            other => panic!("not a question: {other:?}"),
        }
    }

    fn answer(question_id: &str, options: &[&str]) -> QuestionAnswer {
        QuestionAnswer {
            question_id: question_id.into(),
            selected_option_ids: options.iter().map(|id| (*id).to_string()).collect(),
        }
    }

    #[test]
    fn only_the_two_cursor_methods_are_supported() {
        assert!(is_supported(CURSOR_ASK_QUESTION));
        assert!(is_supported(CURSOR_CREATE_PLAN));
        // Cursor also sends this one; it is not blocking and not rendered.
        assert!(!is_supported("cursor/update_todos"));
        assert!(!is_supported("session/request_permission"));
        assert!(matches!(
            parse("cursor/update_todos", &json!({})),
            Err(ExtensionError::Unsupported(_))
        ));
    }

    #[test]
    fn a_question_keeps_the_agents_ids_and_reads_allow_multiple() {
        let ask = question();
        assert_eq!(ask.tool_call_id, "call-ask-1");
        assert_eq!(ask.title.as_deref(), Some("Need input"));
        assert_eq!(ask.questions.len(), 2);
        assert_eq!(ask.questions[0].id, "q-mode");
        assert!(!ask.questions[0].allow_multiple);
        assert_eq!(ask.questions[0].options[1].id, "plan");
        assert_eq!(ask.questions[0].options[1].label, "Plan");
        assert!(ask.questions[1].allow_multiple);
        assert_eq!(
            ExtensionRequest::Question(ask.clone()).title(),
            "Need input"
        );
        assert_eq!(ExtensionRequest::Question(ask).kind(), AskKind::Question);
    }

    #[test]
    fn a_question_with_no_title_is_headed_by_its_first_prompt() {
        let mut raw = fixture("ask_question.request");
        raw.as_object_mut().unwrap().remove("title");
        let request = parse(CURSOR_ASK_QUESTION, &raw).expect("parses");
        assert_eq!(request.title(), "Which mode should the migration run in?");
    }

    #[test]
    fn unknown_fields_are_dropped_from_the_record_not_turned_into_ui() {
        let mut raw = fixture("ask_question.request");
        raw["runAsync"] = json!(true);
        raw["questions"][0]["hint"] = json!("<script>");
        let request = parse(CURSOR_ASK_QUESTION, &raw).expect("parses");
        let recorded = request.to_json();
        assert!(recorded.get("runAsync").is_none());
        assert!(recorded["questions"][0].get("hint").is_none());
        assert_eq!(recorded["questions"][0]["id"], "q-mode");
        // And it reads back as the same request.
        assert_eq!(
            parse_recorded(AskKind::Question, &recorded).expect("re-reads"),
            request
        );
    }

    #[test]
    fn a_malformed_question_is_refused_not_rendered() {
        let cases: Vec<(&str, Value)> = vec![
            (
                "no questions",
                json!({ "toolCallId": "c", "questions": [] }),
            ),
            (
                "no tool call",
                json!({ "toolCallId": "", "questions": [{ "id": "q", "prompt": "?", "options": [{ "id": "a", "label": "A" }] }] }),
            ),
            (
                "no options",
                json!({ "toolCallId": "c", "questions": [{ "id": "q", "prompt": "?", "options": [] }] }),
            ),
            (
                "blank prompt",
                json!({ "toolCallId": "c", "questions": [{ "id": "q", "prompt": " ", "options": [{ "id": "a", "label": "A" }] }] }),
            ),
            (
                "duplicate option",
                json!({ "toolCallId": "c", "questions": [{ "id": "q", "prompt": "?", "options": [{ "id": "a", "label": "A" }, { "id": "a", "label": "B" }] }] }),
            ),
            (
                "duplicate question",
                json!({ "toolCallId": "c", "questions": [
                { "id": "q", "prompt": "?", "options": [{ "id": "a", "label": "A" }] },
                { "id": "q", "prompt": "??", "options": [{ "id": "b", "label": "B" }] }
            ] }),
            ),
            ("not an object", json!("questions?")),
        ];
        for (why, raw) in cases {
            assert!(
                matches!(
                    parse(CURSOR_ASK_QUESTION, &raw),
                    Err(ExtensionError::Malformed(_))
                ),
                "{why} should be malformed"
            );
        }
    }

    #[test]
    fn a_plan_keeps_its_body_todos_and_phases() {
        let request = parse(CURSOR_CREATE_PLAN, &fixture("create_plan.request")).expect("parses");
        let ExtensionRequest::Plan(plan) = &request else {
            panic!("not a plan");
        };
        assert_eq!(plan.tool_call_id, "call-plan-1");
        assert_eq!(request.title(), "Auth migration");
        assert!(plan.plan.starts_with("## Steps"));
        assert_eq!(plan.todos.len(), 3);
        assert_eq!(plan.todos[1].status, "in_progress");
        assert_eq!(plan.phases.len(), 2);
        assert_eq!(plan.phases[1].todos.len(), 2);
        assert_eq!(request.kind(), AskKind::Plan);
    }

    #[test]
    fn a_bare_plan_still_parses_and_an_unnamed_one_is_called_a_plan() {
        let request = parse(CURSOR_CREATE_PLAN, &json!({ "toolCallId": "c" })).expect("parses");
        let ExtensionRequest::Plan(plan) = &request else {
            panic!("not a plan");
        };
        assert_eq!(plan.plan, "");
        assert!(plan.todos.is_empty());
        assert_eq!(request.title(), "Review the plan");
        assert!(matches!(
            parse(CURSOR_CREATE_PLAN, &json!({ "plan": "x" })),
            Err(ExtensionError::Malformed(_))
        ));
    }

    #[test]
    fn answers_are_checked_against_what_was_asked() {
        let ask = question();
        assert_eq!(
            check_answers(
                &ask,
                &[
                    answer("q-mode", &["plan"]),
                    answer("q-scope", &["api", "web"])
                ]
            ),
            Ok(())
        );
        let refused: Vec<(&str, Vec<QuestionAnswer>)> = vec![
            ("missing question", vec![answer("q-mode", &["plan"])]),
            (
                "unknown question",
                vec![
                    answer("q-mode", &["plan"]),
                    answer("q-scope", &["api"]),
                    answer("q-nope", &["x"]),
                ],
            ),
            (
                "unknown option",
                vec![answer("q-mode", &["yolo"]), answer("q-scope", &["api"])],
            ),
            (
                "two for a single choice",
                vec![
                    answer("q-mode", &["plan", "agent"]),
                    answer("q-scope", &["api"]),
                ],
            ),
            (
                "empty selection",
                vec![answer("q-mode", &[]), answer("q-scope", &["api"])],
            ),
            (
                "same option twice",
                vec![
                    answer("q-mode", &["plan"]),
                    answer("q-scope", &["api", "api"]),
                ],
            ),
            (
                "same question twice",
                vec![
                    answer("q-mode", &["plan"]),
                    answer("q-mode", &["agent"]),
                    answer("q-scope", &["api"]),
                ],
            ),
        ];
        for (why, answers) in refused {
            assert!(
                check_answers(&ask, &answers).is_err(),
                "{why} should be refused"
            );
        }
    }

    #[test]
    fn every_outcome_is_the_exact_shape_cursor_reads() {
        assert_eq!(
            question_answered(&[
                answer("q-mode", &["plan"]),
                answer("q-scope", &["api", "web"])
            ]),
            fixture("ask_question.answered")
        );
        assert_eq!(
            question_skipped(Some("User skipped questions")),
            fixture("ask_question.skipped")
        );
        assert_eq!(
            question_skipped(Some("  ")),
            json!({ "outcome": { "outcome": "skipped" } })
        );
        assert_eq!(plan_accepted(), fixture("create_plan.accepted"));
        assert_eq!(
            plan_rejected(Some("User rejected plan")),
            fixture("create_plan.rejected")
        );
        assert_eq!(
            plan_rejected(None),
            json!({ "outcome": { "outcome": "rejected" } })
        );
        assert_eq!(cancelled(), fixture("cancelled"));
    }
}
