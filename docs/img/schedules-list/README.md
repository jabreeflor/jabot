# Schedules screen — screenshot evidence

Evidence for the schedules list and its docked prompt (#25), merged as
[#50](https://github.com/jabreeflor/jabot/pull/50). That pull request changed
1,979 lines of rendering and committed no screenshots, which `CLAUDE.md`
requires; these are that evidence, captured after the fact.

Captured by rendering the real `SchedulesView` — not a mock of it — in
Chromium at 1180x860, deviceScaleFactor 2, against fixtures built from the
same `ScheduleView` shape the host serves. The throwaway Vite entry that
mounts the component is not part of the tree; everything visible below is the
shipped component and the shipped `src/styles/schedules.css`.

| file | what it shows |
| --- | --- |
| `list.png` | the list at rest — all five status dots, relative "next run", search, All/Active/Paused counts, suggestions, and the docked prompt |
| `row-open.png` | one row expanded: prompt, last-run and cron chips, Run now / Edit |
| `prompt-parsed.png` | `parseWhen` reading a sentence and moving the WHEN chip |
| `empty.png` | the empty state and its suggestions |

## What each one is proof of

**`list.png`** — the headline claim of #50. `relativeWhen` renders
"Next run in 13 minutes", "in 8 hours", "in 4 days" and "in 17 hours" rather
than the absolute `nextRunAt` timestamp the screen drew before. The five
status dots are visibly distinct: ready (green), running (purple), caught-up
late (orange), failed (red), paused (hollow).

A first capture of this screen showed every row reading "due now". That was
the fixture's fault and not the view's — it pinned `Date.now()` to a date in
the past, so every `nextRunAt` had already elapsed. The fixtures take their
offsets from the real clock, which is why the numbers above are meaningful.

**`prompt-parsed.png`** — typing `summarise overnight mail every weekday at
9am` into the docked box shows `READ FROM YOUR WORDS · Summarise overnight
mail · Weekdays at 09:00, as Chief`, with the WHEN chip moved to "Every
weekday, 9am" and highlighted. That is `parseWhen` and `suggestName` working
on real input, and it is the part of the change a unit test can assert but
cannot show.
