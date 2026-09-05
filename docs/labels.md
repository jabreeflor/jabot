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

## 2026-09 triage results

26 open issues; 23 are decision records with nothing left, 2 records still
owe one item each, and 1 is a feature. Evidence for every row is in the
`## Triage` comment on the issue.

| issue | disposition | pickup | size | area | what is left |
| --- | --- | --- | --- | --- | --- |
| #38 Google OAuth client | implement | needs-human | L | crew, harness | Owner: Google Cloud project, consent screen, CASA verification. Agent-ready slice once a client id exists: default-id wiring in `flow.rs`, narrowing `missing_client_hint`, unverified-app UX, docs. |
| #66 D-013 permission broker | follow-up | needs-human | M | harness, ui | Remembered "always allow" grants: `permission_decisions` table exists (migration 0001) but nothing writes or consults it. Needs a product call on the settings surface and scope before it is agent-ready. |
| #73 D-019 notifications | follow-up | needs-human | S | lifecycle | The record's four-item runtime checklist (permission prompt, banner delivery, click routing, replace-not-stack) has never been run on a real signed Mac. Manual QA, not code. |
| #54 D-001 test harness | decommission | | | app-shell, harness | `jabot-hostd` + Vitest e2e are in `verify.sh` and CI. |
| #55 D-002 #10 merged not rebuilt | decommission | | | harness | Merge and zombie-process fix on `main`. |
| #56 D-003 two host defects | decommission | | | app-shell, harness | Both fixes present with regression tests. |
| #57 D-004 UI port departures | decommission | | | ui, app-shell | All departures still match settled decisions; permission UI shipped in PR #37. |
| #58 D-005 bundle targets | decommission | | | app-shell | `["app","dmg"]` intact. The updater pubkey placeholder is a documented pre-release gate, not record scope. |
| #59 D-006 lifecycle core | decommission | | | lifecycle | Deferred items shipped in #21, #27, #83. |
| #60 D-007 harness catalog | decommission | | | harness, ui | Pooling (#84), picker wiring (#81, #82) shipped. |
| #61 D-008 OAuth without client id | decommission | | | crew, harness | Only open thread is #38. |
| #62 D-009 crew store | decommission | | | crew, data | Standing thread (#24), unread (#86), memory dir shipped. |
| #63 D-010 transcript overlay | decommission | | | harness, ui | #87, #88, #89, #90, #91, #97 closed every deferred item. |
| #64 D-011 supervisor | decommission | | | lifecycle | Resume/drift UI shipped (#90, #91). App Nap fallback was never promised. |
| #65 D-012 worktree policy | decommission | | | git | New Chat controls (#92), location UI (#93) shipped. |
| #67 D-014 Chief's host tools | decommission | | | crew, harness | Provenance surface shipped in #98. |
| #68 D-015 pairing handshake | decommission | | | remote | Mutual host auth shipped in #99. |
| #69 D-014 toolchain | decommission | | | app-shell | `verify.sh` stage 0 now detects drift automatically. |
| #70 D-016 Mobile Inbox | decommission | | | remote | #101, #102, #103 closed every client gap. |
| #71 D-017 cron | decommission | | | lifecycle | Notifications (#27) and real Inbox (#22) shipped. |
| #72 D-018 fold path | decommission | | | lifecycle | #97, #106, #107 closed every deferred item. |
| #74 D-020 PR linkage | decommission | | | git, ui | #110, #111, #112 shipped. |
| #75 D-021 Inbox on real data | decommission | | | lifecycle | Live `useInbox` wiring confirmed; revoke fix landed. |
| #76 D-020 audit | decommission | | | git, data | All three fixes present with tests. |
| #77 D-024 list_crew_status | decommission | | | crew, lifecycle | #114 added `busy` without redefining `idle`. |
| #78 D-025 GitHub sign-in | decommission | | | git, ui | `github/login`, `pr/mine`, `HarnessIcon.tsx` all on `main`. |

Cross-cutting notes from the triage:

- The decision records were a to-do list in disguise: nearly every "declared
  but not built" item became one of #81–#114 and shipped. Closing the 23
  `disposition:decommission` records leaves the tracker showing only real work.
- #66's "always allow" gap is the one deferred item that was never filed as
  its own issue. It should become one when the settings surface is decided.
- #38 is the only feature and the only `size:L`. Its owner-side steps
  (Cloud project, verification) are weeks long and gate the in-repo slice.
