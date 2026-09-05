# Issue labels: where an issue can be picked up, and by whom

The label set answers two questions about any open issue without reading it:
**is there work left**, and **who can do it** — a Claude Code session running
unattended, a person, or nobody until something is researched first. Area
labels say *where* in the codebase; the rest say *what to do about it*.

Every open issue should carry exactly one `disposition:*` label. Issues with
work left also carry exactly one pickup label and one `size:*` label.

## Disposition — is there work left?

| label | meaning | what happens next |
| --- | --- | --- |
| `disposition:decommission` | A decision record (or stale issue) with nothing actionable left: every item it deferred has since shipped or is moot. The triage comment says why, with evidence. | Close as **completed**. The record stays searchable; closing it is the decommission. |
| `disposition:follow-up` | A decision record that still leaves concrete unfinished work. The triage comment lists it as a checklist. | Keep open, or spin the checklist out into its own issue(s) and then close the record. |
| `disposition:implement` | A real feature or bug, not a record. | Normal build issue. |

## Pickup — who can do it?

| label | meaning | how to act on it |
| --- | --- | --- |
| `agent-ready` | The spec is clear enough that a Claude Code session can do it unattended. The triage comment includes a starter prompt. | Start a session on the issue (Claude Code on the web, `claude` in a clone, or a routine) with the starter prompt. Expect a PR with a PR artifact and screenshot evidence per `CLAUDE.md`. |
| `needs-human` | Blocked on something only the owner can supply: a product call, an account, a credential, hardware (a Mac), or a design decision. The comment names it. | Supply the named thing, then relabel `agent-ready` if the rest is mechanical. |
| `needs-research` | Unknowns must be resolved before the work can be specified. The comment says what to research. | A research-only session is fine here: its output is a comment or a doc, not a PR. Relabel afterwards. |

## Size — how big is it?

| label | rough budget |
| --- | --- |
| `size:S` | under half a day; typically one PR touching a few files |
| `size:M` | one to two days; one PR, maybe two |
| `size:L` | multi-day or multi-PR; worth its own tracking issue |

## Area — where in the codebase?

These predate the triage and are reused as-is:

| label | surface |
| --- | --- |
| `app-shell` | Tauri workspace, packaging, signing, updater, CI/toolchain |
| `harness` | ACP adapters, harness catalog, doctor, permission broker, transcript |
| `crew` | bots, crew store, Chief of Staff, tool/MCP/OAuth framework |
| `lifecycle` | thread state machine, run ledger, supervisor, schedules, fold, notifications, Inbox |
| `git` | folders/repos, worktrees, pull-request linkage, GitHub auth |
| `ui` | renderer components and styling |
| `data` | SQLite store, migrations, vault |
| `remote` | device pairing, mobile Inbox client, socket transport |

Other pre-existing labels: `decision` (a decision record, D-001…), `tracking`
(a dependency-graph issue), `mvp1` / `mvp2` (milestone of the original plan).

## Automation recipes

- **Unattended backlog:** `is:open label:agent-ready` is the queue a routine
  can drain. Each item's triage comment carries the prompt.
- **Owner's inbox:** `is:open label:needs-human` is the list of decisions and
  assets blocking work; each comment names exactly what is needed.
- **Cleanup:** `is:open label:disposition:decommission` can be closed in one
  pass; nothing in that list has work attached.
- **Planning:** `is:open label:disposition:follow-up` shows which decision
  records still owe work and how much (`size:*`).

## How the labels were assigned

The 2026-09 triage read every open issue, checked each deferred item against
`main` and the follow-up issues that closed it, and left one `## Triage`
comment per issue with the classification, the evidence, and the path forward.
Labels are meant to be re-evaluated whenever an issue's state changes; the
comment is the audit trail for why the current labels are what they are.
