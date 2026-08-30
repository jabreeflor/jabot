/**
 * The schedules screen (#25).
 *
 * Three things this view has to get right that a generic list does not. It has
 * to say when the job is *next* owed — the host's answer, never a cron the
 * browser re-derived — it has to make a catch-up visible, because a run that
 * happened hours late while the Mac was shut is the one event on this screen
 * the user did not ask for and cannot otherwise find out about, and it has to
 * turn a sentence into a schedule, because that is what the prompt promises.
 */
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { ScheduleEditorModal } from "../components/ScheduleEditorModal";
import { SchedulesView } from "../views/SchedulesView";
import {
  describeCron,
  describeFire,
  parseWhen,
  relativeWhen,
  suggestName,
} from "../views/schedules";
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
    // Far enough out that the relative label is stable whenever this runs.
    nextRunAt: new Date(Date.now() + 3 * 3600_000).toISOString(),
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
    onCreate: vi.fn().mockResolvedValue(schedule()),
    onEdit: vi.fn(),
    onToggle: vi.fn(),
    onRunNow: vi.fn(),
    onOpenThread: vi.fn(),
    ...over,
  };
  render(<SchedulesView {...props} />);
  return props;
}

function row(name: string): HTMLElement {
  const element = screen.getByText(name).closest(".sched-row");
  if (!element) throw new Error(`no schedule row for ${name}`);
  return element as HTMLElement;
}

/** The row is closed at rest: everything but the headline is one click down. */
async function open(name: string): Promise<HTMLElement> {
  await userEvent.click(within(row(name)).getByRole("button", { expanded: false }));
  return row(name);
}

describe("SchedulesView", () => {
  it("says when, how soon, and as whom without being opened", () => {
    renderView();

    const headline = row("Morning triage");
    expect(within(headline).getByText(/Weekdays at 09:00/)).toBeInTheDocument();
    expect(within(headline).getByText(/Next run in 3 hours/)).toBeInTheDocument();
    expect(within(headline).getByText(/as Writer/)).toBeInTheDocument();
    // The instruction is real content, not a headline: it waits to be asked for.
    expect(
      screen.queryByText("Summarise overnight mail."),
    ).not.toBeInTheDocument();
  });

  it("opens a row to the prompt, the last run, and the buttons", async () => {
    renderView({ schedules: [schedule({ lastFire: fire() })] });

    const opened = await open("Morning triage");
    expect(
      within(opened).getByText("Summarise overnight mail."),
    ).toBeInTheDocument();
    expect(within(opened).getByText(/^Ran /)).toBeInTheDocument();
    expect(
      within(opened).getByRole("button", { name: "Run now" }),
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
        onCreate={vi.fn()}
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

    const headline = row("Morning triage");
    expect(within(headline).getByText("Paused")).toBeInTheDocument();
    expect(within(headline).queryByText(/^Next run/)).not.toBeInTheDocument();
  });

  /** The case the whole catch-up policy exists for. */
  it("says out loud when a run was caught up and how many were dropped", async () => {
    const caught = fire({
      caughtUp: true,
      skippedCount: 6,
      detail: "caught up on the 2h old run; 6 earlier were skipped",
    });
    renderView({ schedules: [schedule({ lastFire: caught })] });

    const opened = await open("Morning triage");
    expect(
      within(opened).getByText(/Caught up .*6 missed runs skipped/),
    ).toBeInTheDocument();
    expect(within(opened).getByText(caught.detail!)).toBeInTheDocument();
  });

  it("runs, edits, toggles and opens the thread by id", async () => {
    const props = renderView({
      schedules: [schedule({ lastFire: fire(), threadId: "bot-writer" })],
    });
    const opened = await open("Morning triage");

    await userEvent.click(
      within(opened).getByRole("button", { name: "Run now" }),
    );
    expect(props.onRunNow).toHaveBeenCalledWith("sched-1");

    await userEvent.click(within(opened).getByRole("button", { name: "Edit" }));
    expect(props.onEdit).toHaveBeenCalledWith("sched-1");

    // The switch is the one control that works without opening the row.
    await userEvent.click(within(opened).getByRole("checkbox"));
    expect(props.onToggle).toHaveBeenCalledWith("sched-1", false);

    await userEvent.click(
      within(opened).getByRole("button", { name: "Open thread" }),
    );
    expect(props.onOpenThread).toHaveBeenCalledWith("bot-writer");
  });

  it("offers no thread to open before the job has ever run", async () => {
    renderView({ schedules: [schedule({ threadId: undefined })] });
    const opened = await open("Morning triage");
    expect(
      within(opened).queryByRole("button", { name: "Open thread" }),
    ).not.toBeInTheDocument();
  });
});

describe("finding one in a list of them", () => {
  const many = [
    schedule(),
    schedule({
      scheduleId: "sched-2",
      name: "Nightly backup",
      prompt: "Back the vault up.",
      cron: "0 2 * * *",
      enabled: false,
      nextRunAt: undefined,
    }),
  ];

  it("searches names, prompts, bots and the cron in words", async () => {
    renderView({ schedules: many });

    await userEvent.type(screen.getByLabelText("Search schedules"), "backup");
    expect(screen.getByText("Nightly backup")).toBeInTheDocument();
    expect(screen.queryByText("Morning triage")).not.toBeInTheDocument();

    await userEvent.clear(screen.getByLabelText("Search schedules"));
    await userEvent.type(screen.getByLabelText("Search schedules"), "weekdays");
    expect(screen.getByText("Morning triage")).toBeInTheDocument();
    expect(screen.queryByText("Nightly backup")).not.toBeInTheDocument();
  });

  it("filters to active and paused, and counts both", async () => {
    renderView({ schedules: many });

    const paused = screen.getByRole("button", { name: /^Paused/ });
    expect(paused).toHaveTextContent("1");

    await userEvent.click(paused);
    expect(screen.getByText("Nightly backup")).toBeInTheDocument();
    expect(screen.queryByText("Morning triage")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /^Active/ }));
    expect(screen.getByText("Morning triage")).toBeInTheDocument();
    expect(screen.queryByText("Nightly backup")).not.toBeInTheDocument();
  });

  /** An empty result from a filter is not an empty screen: the total is what
      tells you the schedules are still there. */
  it("says how many there are when nothing matches", async () => {
    renderView({ schedules: many });
    await userEvent.type(screen.getByLabelText("Search schedules"), "zzz");
    expect(screen.getByText(/Nothing matches. 2 schedules/)).toBeInTheDocument();
  });
});

describe("writing one as a prompt", () => {
  it("swaps the list for the prompt, and back again", async () => {
    renderView();

    await userEvent.click(
      screen.getByRole("button", { name: /New schedule/ }),
    );
    expect(screen.queryByText("Morning triage")).not.toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: /What should run on a timer/ }),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Schedules" }));
    expect(screen.getByText("Morning triage")).toBeInTheDocument();
  });

  it("reads the schedule out of the sentence, and names it from the rest", async () => {
    const props = renderView();
    await userEvent.click(screen.getByRole("button", { name: /New schedule/ }));

    await userEvent.type(
      screen.getByLabelText("What should it do?"),
      "Summarise overnight mail every weekday at 9am",
    );
    // The WHEN chip moved itself, and says so in the words the list will use.
    expect(screen.getByLabelText("When")).toHaveValue("0 9 * * 1-5");
    expect(screen.getByText(/Read from your words/)).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "Create schedule" }),
    );
    expect(props.onCreate).toHaveBeenCalledWith({
      botId: BOTS[0].id,
      name: "Summarise overnight mail",
      cron: "0 9 * * 1-5",
      prompt: "Summarise overnight mail every weekday at 9am",
      catchUp: "once",
    });
  });

  /** Once the chip is the user's answer, the parser stops moving it: a control
      that springs back while you type is worse than one that never helped. */
  it("stops reading the sentence once the chip has been set by hand", async () => {
    renderView();
    await userEvent.click(screen.getByRole("button", { name: /New schedule/ }));

    await userEvent.selectOptions(screen.getByLabelText("When"), "0 * * * *");
    await userEvent.type(
      screen.getByLabelText("What should it do?"),
      "Check the build every weekday at 9am",
    );
    expect(screen.getByLabelText("When")).toHaveValue("0 * * * *");
    expect(screen.queryByText(/Read from your words/)).not.toBeInTheDocument();
  });

  it("keeps the draft on screen when the host refuses it", async () => {
    const onCreate = vi
      .fn()
      .mockRejectedValue(new Error("99 is outside 0-23 in the hour field"));
    renderView({ onCreate });
    await userEvent.click(screen.getByRole("button", { name: /New schedule/ }));

    await userEvent.type(
      screen.getByLabelText("What should it do?"),
      "Do the thing",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Create schedule" }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent("hour field");
    expect(screen.getByLabelText("What should it do?")).toHaveValue(
      "Do the thing",
    );
  });

  /** Escape leaves every other surface in the app that can be left. */
  it("leaves on Escape", async () => {
    renderView();
    await userEvent.click(screen.getByRole("button", { name: /New schedule/ }));

    await userEvent.type(screen.getByLabelText("What should it do?"), "{Escape}");
    expect(screen.getByText("Morning triage")).toBeInTheDocument();
  });

  it("will not send an empty prompt", async () => {
    const props = renderView();
    await userEvent.click(screen.getByRole("button", { name: /New schedule/ }));

    const send = screen.getByRole("button", { name: "Create schedule" });
    expect(send).toBeDisabled();
    await userEvent.click(send);
    expect(props.onCreate).not.toHaveBeenCalled();
  });

  /** A schedules screen cannot demonstrate itself, so it offers three whole
      schedules — and a suggestion is a prompt already written, not a shortcut
      that creates something behind your back. */
  it("opens a suggestion as a prompt you can still edit", async () => {
    const props = renderView({ schedules: [] });

    await userEvent.click(
      screen.getByRole("button", { name: /Morning brief/ }),
    );
    expect(props.onCreate).not.toHaveBeenCalled();
    const written = screen.getByLabelText(
      "What should it do?",
    ) as HTMLTextAreaElement;
    expect(written.value).toContain("Summarise overnight mail");
    expect(screen.getByLabelText("When")).toHaveValue("0 8 * * 1-5");
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

describe("words in cron", () => {
  it("reads the schedules people write in sentences", () => {
    expect(parseWhen("summarise mail every weekday at 9am")).toBe("0 9 * * 1-5");
    expect(parseWhen("every day at 08:30, do the thing")).toBe("30 8 * * *");
    expect(parseWhen("check the build every hour")).toBe("0 * * * *");
    expect(parseWhen("every monday at 9:30am, plan the week")).toBe(
      "30 9 * * 1",
    );
    expect(parseWhen("post the digest every evening")).toBe("0 18 * * *");
    expect(parseWhen("every day at noon, water the plants")).toBe("0 12 * * *");
  });

  /** A time in a sentence is not a schedule. "The 9am standup" is a subject,
      and a chip that filled itself in from it would be lying. */
  it("refuses to invent a recurrence nobody said", () => {
    expect(parseWhen("summarise the 9am standup")).toBeNull();
    expect(parseWhen("look at the 15 open pull requests")).toBeNull();
    expect(parseWhen("")).toBeNull();
  });
});

describe("how soon", () => {
  const now = Date.parse("2026-03-04T09:00:00.000Z");

  it("counts in the units a person would say out loud", () => {
    const at = (ms: number) => new Date(now + ms).toISOString();
    expect(relativeWhen(at(13 * 60_000), now)).toBe("in 13 minutes");
    expect(relativeWhen(at(3 * 3600_000), now)).toBe("in 3 hours");
    expect(relativeWhen(at(5 * 24 * 3600_000), now)).toBe("in 5 days");
    expect(relativeWhen(at(60_000), now)).toBe("in a minute");
  });

  /** Past due is a statement about the queue, not about the clock: the host
      still owes the run, so the row says so rather than counting backwards. */
  it("calls a run the host still owes due now", () => {
    expect(relativeWhen(new Date(now - 6 * 3600_000).toISOString(), now)).toBe(
      "due now",
    );
    expect(relativeWhen(undefined, now)).toBeNull();
    expect(relativeWhen("not a date", now)).toBeNull();
  });
});

describe("naming one nobody named", () => {
  it("keeps what it does and drops when it does it", () => {
    expect(suggestName("Summarise overnight mail every weekday at 9am")).toBe(
      "Summarise overnight mail",
    );
    expect(
      suggestName(
        "Every Friday at 4pm, turn this week’s threads into a status update",
      ),
    ).toBe("Turn this week’s threads");
    expect(suggestName("   ")).toBe("");
  });

  /** "every" is only a schedule word when a time follows it. */
  it("leaves an every that is not a when alone", () => {
    expect(suggestName("check every open pull request for CI failures")).toBe(
      "Check every open pull request",
    );
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
