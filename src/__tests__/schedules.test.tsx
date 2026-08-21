/**
 * The schedules screen (#25).
 *
 * Two things this view has to get right that a generic list does not. It has to
 * say when the job is *next* owed — the host's answer, never a cron the browser
 * re-derived — and it has to make a catch-up visible, because a run that
 * happened hours late while the Mac was shut is the one event on this screen
 * the user did not ask for and cannot otherwise find out about.
 */
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ScheduleEditorModal } from "../components/ScheduleEditorModal";
import { SchedulesView } from "../views/SchedulesView";
import { describeCron, describeFire } from "../views/schedules";
import { initialMockState } from "../views/mock-host";
import type { ScheduleFireView, ScheduleView } from "../host";

const BOTS = initialMockState().bots;

function fire(over: Partial<ScheduleFireView> = {}): ScheduleFireView {
  return {
    fireId: "fire-1",
    scheduleId: "sched-1",
    threadId: "bot-writer",
    runId: "run-1",
    dueAt: "2026-03-04T09:00:00.000Z",
    firedAt: "2026-03-04T09:00:01.000Z",
    state: "delivered",
    caughtUp: false,
    skippedCount: 0,
    ...over,
  };
}

function schedule(over: Partial<ScheduleView> = {}): ScheduleView {
  return {
    scheduleId: "sched-1",
    botId: "writer",
    botName: "Writer",
    name: "Morning triage",
    cron: "0 9 * * 1-5",
    prompt: "Summarise overnight mail.",
    enabled: true,
    catchUp: "once",
    nextRunAt: "2026-03-05T09:00:00.000Z",
    recentFires: [],
    createdAt: "2026-03-01T00:00:00.000Z",
    updatedAt: "2026-03-01T00:00:00.000Z",
    ...over,
  };
}

function renderView(over: Partial<Parameters<typeof SchedulesView>[0]> = {}) {
  const props = {
    schedules: [schedule()],
    bots: BOTS,
    error: null,
    onReload: vi.fn(),
    onAdd: vi.fn(),
    onEdit: vi.fn(),
    onToggle: vi.fn(),
    onRunNow: vi.fn(),
    onOpenThread: vi.fn(),
    ...over,
  };
  render(<SchedulesView {...props} />);
  return props;
}

function card(name: string): HTMLElement {
  const element = screen.getByText(name).closest(".crew-card");
  if (!element) throw new Error(`no schedule card for ${name}`);
  return element as HTMLElement;
}

describe("SchedulesView", () => {
  it("says when, as whom, and what it will do", () => {
    renderView();

    const row = card("Morning triage");
    expect(within(row).getByText(/Weekdays at 09:00/)).toBeInTheDocument();
    expect(within(row).getByText(/as Writer/)).toBeInTheDocument();
    expect(
      within(row).getByText("Summarise overnight mail."),
    ).toBeInTheDocument();
  });

  /** A host that has not answered and a host with nothing to say are two
      different pictures, and only one of them is an empty state. */
  it("tells an unanswered host apart from an empty one", () => {
    const { unmount } = render(
      <SchedulesView
        schedules={null}
        bots={BOTS}
        error={null}
        onReload={vi.fn()}
        onAdd={vi.fn()}
        onEdit={vi.fn()}
        onToggle={vi.fn()}
        onRunNow={vi.fn()}
        onOpenThread={vi.fn()}
      />,
    );
    expect(screen.getByText(/Asking the host/)).toBeInTheDocument();
    unmount();

    renderView({ schedules: [] });
    expect(screen.getByText(/No schedules yet/)).toBeInTheDocument();
  });

  it("shows a paused schedule as paused rather than as overdue", () => {
    renderView({
      schedules: [schedule({ enabled: false, nextRunAt: undefined })],
    });

    const row = card("Morning triage");
    expect(within(row).getByText("Paused")).toBeInTheDocument();
    expect(within(row).queryByText(/^Next /)).not.toBeInTheDocument();
  });

  /** The case the whole catch-up policy exists for. */
  it("says out loud when a run was caught up and how many were dropped", () => {
    const caught = fire({
      caughtUp: true,
      skippedCount: 6,
      detail: "caught up on the 2h old run; 6 earlier were skipped",
    });
    renderView({ schedules: [schedule({ lastFire: caught })] });

    const row = card("Morning triage");
    expect(
      within(row).getByText(/Caught up .*6 missed runs skipped/),
    ).toBeInTheDocument();
    expect(within(row).getByText(caught.detail!)).toBeInTheDocument();
  });

  it("runs, edits, toggles and opens the thread by id", async () => {
    const props = renderView({
      schedules: [schedule({ lastFire: fire(), threadId: "bot-writer" })],
    });
    const row = card("Morning triage");

    await userEvent.click(within(row).getByRole("button", { name: "Run now" }));
    expect(props.onRunNow).toHaveBeenCalledWith("sched-1");

    await userEvent.click(within(row).getByRole("button", { name: "Edit" }));
    expect(props.onEdit).toHaveBeenCalledWith("sched-1");

    await userEvent.click(within(row).getByRole("checkbox"));
    expect(props.onToggle).toHaveBeenCalledWith("sched-1", false);

    await userEvent.click(
      within(row).getByRole("button", { name: "Open thread" }),
    );
    expect(props.onOpenThread).toHaveBeenCalledWith("bot-writer");
  });

  it("offers no thread to open before the job has ever run", () => {
    renderView({ schedules: [schedule({ threadId: undefined })] });
    expect(
      within(card("Morning triage")).queryByRole("button", {
        name: "Open thread",
      }),
    ).not.toBeInTheDocument();
  });
});

describe("cron in words", () => {
  it("describes the schedules people actually write", () => {
    expect(describeCron("0 9 * * 1-5")).toBe("Weekdays at 09:00");
    expect(describeCron("30 8 * * *")).toBe("Every day at 08:30");
    expect(describeCron("0 9 * * 1")).toBe("Every Monday at 09:00");
    expect(describeCron("15 * * * *")).toBe("Every hour at :15");
    expect(describeCron("@daily")).toBe("Every day at midnight");
  });

  /** Better the raw cron than a confident wrong sentence: the user can look a
      cron up, and cannot look up a description we invented. */
  it("shows anything it cannot describe verbatim", () => {
    expect(describeCron("*/7 3 1,15 * 2")).toBe("*/7 3 1,15 * 2");
    expect(describeCron("0 0 1 1 *")).toBe("0 0 1 1 *");
  });
});

describe("last run in words", () => {
  it("separates never-run, skipped, failed and still-running", () => {
    expect(describeFire(undefined)).toBe("Has not run yet");
    expect(describeFire(fire({ state: "skipped", skippedCount: 3 }))).toMatch(
      /^Skipped .*3 missed runs skipped$/,
    );
    expect(describeFire(fire({ state: "failed" }))).toMatch(/^Failed /);
    expect(describeFire(fire({ state: "dispatched" }))).toMatch(
      /^Running since /,
    );
    expect(describeFire(fire({ skippedCount: 1 }))).toMatch(
      /1 missed run skipped$/,
    );
  });
});

describe("ScheduleEditorModal", () => {
  it("asks the three questions the host cannot guess", async () => {
    const onSave = vi.fn();
    render(
      <ScheduleEditorModal
        schedule={null}
        bots={BOTS}
        onSave={onSave}
        onCancel={vi.fn()}
      />,
    );

    await userEvent.type(screen.getByLabelText("NAME"), "Morning triage");
    await userEvent.selectOptions(screen.getByLabelText("RUNS AS"), "writer");
    await userEvent.click(
      screen.getByRole("button", { name: "Every weekday, 9am" }),
    );
    await userEvent.type(
      screen.getByLabelText("WHAT SHOULD IT DO?"),
      "Summarise overnight mail.",
    );
    // The question decision #4 forces: the Mac is not always on.
    await userEvent.selectOptions(
      screen.getByLabelText("IF JABOT WAS CLOSED"),
      "skip",
    );
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(onSave).toHaveBeenCalledWith({
      botId: "writer",
      name: "Morning triage",
      cron: "0 9 * * 1-5",
      prompt: "Summarise overnight mail.",
      catchUp: "skip",
    });
  });

  it("keeps the draft on screen when the host refuses it", () => {
    render(
      <ScheduleEditorModal
        schedule={{
          scheduleId: "sched-1",
          botId: "writer",
          name: "Morning triage",
          cron: "0 99 * * *",
          prompt: "go",
          catchUp: "once",
        }}
        bots={BOTS}
        error="99 is outside 0-23 in the hour field (-32602)"
        onSave={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(screen.getByRole("alert")).toHaveTextContent("hour field");
    // The bad cron is still there to be corrected, not thrown away.
    expect(screen.getByLabelText("WHEN")).toHaveValue("0 99 * * *");
  });

  it("removes only an existing schedule", async () => {
    const onRemove = vi.fn();
    const { unmount } = render(
      <ScheduleEditorModal
        schedule={null}
        bots={BOTS}
        onSave={vi.fn()}
        onRemove={onRemove}
        onCancel={vi.fn()}
      />,
    );
    expect(
      screen.queryByRole("button", { name: "Remove" }),
    ).not.toBeInTheDocument();
    unmount();

    render(
      <ScheduleEditorModal
        schedule={{
          scheduleId: "sched-1",
          botId: "writer",
          name: "Morning triage",
          cron: "0 9 * * *",
          prompt: "go",
          catchUp: "once",
        }}
        bots={BOTS}
        onSave={vi.fn()}
        onRemove={onRemove}
        onCancel={vi.fn()}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(onRemove).toHaveBeenCalledWith("sched-1");
  });
});
