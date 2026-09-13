//! Question and plan-review cards (#298): an agent's ACP extension asks,
//! drawn in the thread they block.
//!
//! Both cards are built from the typed request the host read — the agent's
//! own question ids, option ids and labels, its own plan body and todos. The
//! host never invents an option and neither does this file: a card can only
//! send back the verbs Cursor offers for its kind, with the ids it sent.
//!
//! Two rules the shapes here keep.
//!
//! **Native controls, native keyboard.** A question is a form of fieldsets;
//! a single choice is radios and a multiple choice is checkboxes, so Tab,
//! Space and the arrow keys do what they do everywhere else. Nothing here
//! reimplements focus. What it does add is recovery: when a card settles
//! while focus is inside it, focus moves to the status line that says what
//! happened, instead of falling off a button that just went disabled.
//!
//! **A settled card stays, and says what was decided.** A permission card
//! fades out (#20); a question or plan is part of the conversation, so once
//! answered — by this window, another device, or the host when nobody could —
//! it locks and reports the outcome. The buttons are the only thing that could
//! send a second answer, and they go with the first one.

import {
  useEffect,
  useId,
  useRef,
  useState,
  type FocusEvent,
  type FormEvent,
  type RefObject,
} from "react";

import { CheckIcon, RingIcon, SparkIcon } from "./Icon";
import { renderMarkdown } from "./markdown";
import type {
  InteractionDecision,
  InteractionReply,
  PlanTodo,
  QuestionPrompt,
  TranscriptItem,
} from "./types";

export type QuestionItem = Extract<TranscriptItem, { kind: "question" }>;
export type PlanItem = Extract<TranscriptItem, { kind: "plan" }>;

export type OnInteract = (requestId: string, reply: InteractionReply) => void;

/** Which option ids are picked per question, before the answer is sent. */
type Picks = Readonly<Record<string, readonly string[]>>;

export function QuestionCard({
  item,
  onReply,
}: {
  item: QuestionItem;
  onReply?: OnInteract;
}) {
  const [picks, setPicks] = useState<Picks>({});
  const titleId = useId();
  const rootRef = useRef<HTMLFormElement>(null);
  const statusRef = useRef<HTMLParagraphElement>(null);
  const settled = item.decision !== undefined;
  // A stale ask has no adapter behind it; the host closes it as soon as it
  // knows, and until then the only honest control is to dismiss it.
  const answerable = !settled && !item.stale && Boolean(onReply);
  const complete = item.questions.every(
    (question) => (picks[question.id] ?? []).length > 0,
  );

  const focus = useFocusRecovery(rootRef, statusRef, settled);

  function pick(question: QuestionPrompt, optionId: string, on: boolean) {
    setPicks((current) => {
      const chosen = current[question.id] ?? [];
      let next: readonly string[];
      if (!question.allowMultiple) next = [optionId];
      else if (on)
        next = chosen.includes(optionId) ? chosen : [...chosen, optionId];
      else next = chosen.filter((id) => id !== optionId);
      return { ...current, [question.id]: next };
    });
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!answerable || !complete) return;
    onReply?.(item.requestId, {
      outcome: "answered",
      answers: item.questions.map((question) => ({
        questionId: question.id,
        selectedOptionIds: [...(picks[question.id] ?? [])],
      })),
    });
  }

  return (
    <form
      ref={rootRef}
      className={cardClass("question", item)}
      aria-labelledby={titleId}
      data-ask="question"
      data-request-id={item.requestId}
      onSubmit={submit}
      onFocus={focus.onFocus}
      onBlur={focus.onBlur}
    >
      <div className="r1">
        <b id={titleId}>{item.title}</b>
        <span className="pill">
          <SparkIcon />
          question
        </span>
      </div>
      {item.questions.map((question) => {
        const chosen = picks[question.id] ?? [];
        return (
          <fieldset key={question.id} disabled={!answerable}>
            <legend>{question.prompt}</legend>
            {question.options.map((option) => (
              <label key={option.id} className="ichoice">
                <input
                  type={question.allowMultiple ? "checkbox" : "radio"}
                  name={`${item.id}-${question.id}`}
                  value={option.id}
                  checked={chosen.includes(option.id)}
                  onChange={(event) =>
                    pick(question, option.id, event.target.checked)
                  }
                />
                <span>{option.label}</span>
              </label>
            ))}
          </fieldset>
        );
      })}
      <p className="istatus" role="status" tabIndex={-1} ref={statusRef}>
        {questionStatus(item)}
      </p>
      {answerable && (
        <div className="acts">
          <button type="submit" className="btn primary" disabled={!complete}>
            Send answer
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => onReply?.(item.requestId, { outcome: "skipped" })}
          >
            Skip
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => onReply?.(item.requestId, { outcome: "cancelled" })}
          >
            Cancel
          </button>
        </div>
      )}
      {!settled && item.stale && onReply && (
        <div className="acts">
          <button
            type="button"
            className="btn"
            onClick={() => onReply(item.requestId, { outcome: "cancelled" })}
          >
            Dismiss
          </button>
        </div>
      )}
    </form>
  );
}

export function PlanCard({
  item,
  onReply,
}: {
  item: PlanItem;
  onReply?: OnInteract;
}) {
  const [rejecting, setRejecting] = useState(false);
  const [reason, setReason] = useState("");
  const titleId = useId();
  const reasonId = useId();
  const rootRef = useRef<HTMLElement>(null);
  const statusRef = useRef<HTMLParagraphElement>(null);
  const settled = item.decision !== undefined;
  const answerable = !settled && !item.stale && Boolean(onReply);

  const focus = useFocusRecovery(rootRef, statusRef, settled);

  function reject(event: FormEvent) {
    event.preventDefault();
    if (!answerable) return;
    const why = reason.trim();
    onReply?.(item.requestId, {
      outcome: "rejected",
      ...(why ? { reason: why } : {}),
    });
  }

  const grouped = item.phases.length > 0;

  return (
    <section
      ref={rootRef}
      className={cardClass("plan", item)}
      aria-labelledby={titleId}
      data-ask="plan"
      data-request-id={item.requestId}
      onFocus={focus.onFocus}
      onBlur={focus.onBlur}
    >
      <div className="r1">
        <b id={titleId}>{item.title}</b>
        <span className="pill">
          <SparkIcon />
          plan
        </span>
      </div>
      {item.overview && <p className="ioverview">{item.overview}</p>}
      {item.plan.trim() && (
        <div className="iplan">
          {renderMarkdown(withHeadingsAsBold(item.plan))}
        </div>
      )}
      {grouped
        ? item.phases.map((phase, index) => (
            <div key={`${phase.name}-${index}`} className="iphase">
              <h4>{phase.name || `Phase ${index + 1}`}</h4>
              <Todos todos={phase.todos} label={phase.name || "Plan steps"} />
            </div>
          ))
        : item.todos.length > 0 && (
            <Todos todos={item.todos} label="Plan steps" />
          )}
      <p className="istatus" role="status" tabIndex={-1} ref={statusRef}>
        {planStatus(item)}
      </p>
      {answerable && rejecting && (
        <form className="ireason" onSubmit={reject}>
          <label htmlFor={reasonId}>Why? (optional)</label>
          <input
            id={reasonId}
            type="text"
            value={reason}
            autoFocus
            onChange={(event) => setReason(event.target.value)}
            placeholder="What should change"
          />
          <div className="acts">
            <button type="submit" className="btn danger">
              Reject plan
            </button>
            <button
              type="button"
              className="btn"
              onClick={() => setRejecting(false)}
            >
              Back
            </button>
          </div>
        </form>
      )}
      {answerable && !rejecting && (
        <div className="acts">
          <button
            type="button"
            className="btn primary"
            onClick={() => onReply?.(item.requestId, { outcome: "accepted" })}
          >
            Accept plan
          </button>
          <button
            type="button"
            className="btn"
            aria-expanded={false}
            onClick={() => setRejecting(true)}
          >
            Reject…
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => onReply?.(item.requestId, { outcome: "cancelled" })}
          >
            Not now
          </button>
        </div>
      )}
      {!settled && item.stale && onReply && (
        <div className="acts">
          <button
            type="button"
            className="btn"
            onClick={() => onReply(item.requestId, { outcome: "cancelled" })}
          >
            Dismiss
          </button>
        </div>
      )}
    </section>
  );
}

function Todos({
  todos,
  label,
}: {
  todos: readonly PlanTodo[];
  label: string;
}) {
  if (todos.length === 0) return null;
  return (
    <ul className="itodos" aria-label={label}>
      {todos.map((todo, index) => (
        <li key={todo.id || index} data-status={todo.status}>
          <span className="itodo-mark" aria-hidden="true">
            {todo.status === "completed" ? <CheckIcon /> : <RingIcon />}
          </span>
          <span>{todo.content}</span>
          <span className="itodo-status">{todo.status.replace(/_/g, " ")}</span>
        </li>
      ))}
    </ul>
  );
}

/**
 * When a card settles while focus is inside it, move focus to the line that
 * says what happened. Without this the focused button is disabled or gone,
 * and a keyboard user is dropped at the top of the document.
 *
 * Focus-within is tracked from the events rather than read off
 * `document.activeElement` when the card settles: by then React has already
 * removed the button that was pressed, the browser has already moved focus
 * to the body, and no `blur` was fired for the removal. The events are the
 * only record of where focus was.
 */
function useFocusRecovery(
  root: RefObject<HTMLElement | null>,
  status: RefObject<HTMLParagraphElement | null>,
  settled: boolean,
) {
  const within = useRef(false);
  useEffect(() => {
    if (!settled || !within.current) return;
    within.current = false;
    status.current?.focus();
  }, [settled, status]);
  return {
    onFocus: () => {
      within.current = true;
    },
    onBlur: (event: FocusEvent<HTMLElement>) => {
      const next = event.relatedTarget as Node | null;
      if (!next || !root.current?.contains(next)) within.current = false;
    },
  };
}

/**
 * Cursor writes its plans with markdown headings, and the transcript's
 * renderer deliberately has none (`markdown.tsx`: a heading rule can misfire
 * on prose). A plan is not prose an agent typed at a human — its headings are
 * structure — so they become bold lines, which the renderer does have.
 */
function withHeadingsAsBold(plan: string): string {
  return plan
    .split("\n")
    .map((line) => {
      const heading = /^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$/.exec(line);
      return heading ? `**${heading[1]}**` : line;
    })
    .join("\n");
}

function cardClass(kind: "question" | "plan", item: QuestionItem | PlanItem) {
  const classes = ["notice", "icard", kind];
  if (item.decision !== undefined) classes.push("settled");
  return classes.join(" ");
}

function questionStatus(item: QuestionItem): string {
  const decision = item.decision;
  if (decision) {
    switch (decision.outcome) {
      case "answered":
        return withDelivery(
          `Answered: ${chosenLabels(item, decision) || "recorded"}.`,
          decision,
        );
      case "skipped":
        return withDelivery(
          decision.reason ? `Skipped — ${decision.reason}` : "Skipped.",
          decision,
        );
      default:
        return sharedStatus(decision);
    }
  }
  if (item.stale) return STALE;
  return item.questions.length > 1
    ? "Choose an answer for each question."
    : "Choose an answer.";
}

function planStatus(item: PlanItem): string {
  const decision = item.decision;
  if (decision) {
    switch (decision.outcome) {
      case "accepted":
        return withDelivery("Plan accepted.", decision);
      case "rejected":
        return withDelivery(
          decision.reason
            ? `Plan rejected — ${decision.reason}`
            : "Plan rejected.",
          decision,
        );
      default:
        return sharedStatus(decision);
    }
  }
  if (item.stale) return STALE;
  return "Review the plan before the agent goes ahead. Accepting it does not change what the agent is allowed to do.";
}

const STALE =
  "JaBot restarted while the agent was waiting on this, so it can no longer be answered.";

function sharedStatus(decision: InteractionDecision): string {
  switch (decision.outcome) {
    case "cancelled":
      return "Cancelled.";
    case "expired":
      return "The turn ended before this was answered.";
    case "unavailable":
      return "The session that asked this is gone, so it can no longer be answered.";
    default:
      return "Settled.";
  }
}

/** The honest half: recorded is not the same as heard. */
function withDelivery(text: string, decision: InteractionDecision): string {
  return decision.delivered === false
    ? `${text} Recorded, but the agent that asked is gone.`
    : text;
}

/** "Plan; api, web" — the labels behind the ids that were sent. */
function chosenLabels(item: QuestionItem, decision: InteractionDecision) {
  const answers = decision.answers ?? [];
  return item.questions
    .map((question) => {
      const answer = answers.find((a) => a.questionId === question.id);
      if (!answer) return "";
      return answer.selectedOptionIds
        .map(
          (id) =>
            question.options.find((option) => option.id === id)?.label ?? id,
        )
        .join(", ");
    })
    .filter(Boolean)
    .join("; ");
}
