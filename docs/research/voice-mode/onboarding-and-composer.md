# Onboarding, composer, and the host seam

Questions 1–3 and 5 from [`brief.md`](brief.md). Engines live in
[`engines.md`](engines.md).

The clickable UI for everything below is
[`prototypes/jabot-voice.html`](../../../prototypes/jabot-voice.html).
Build against that page, not against a re-reading of this file.

## Product {#product}

Voice mode, for JaBot, is **dictation into the composer**.

The user taps (or holds) the mic that is already on every chat. The
host transcribes. Words appear in the same `<input>` Return already
submits. The user edits and sends, or sends as-is. While a turn is
running the button is still Stop — a recording never fights a cancel.

That is the whole first slice. It is not:

- a duplex conversation that talks back
- a wake word or background listener
- a Chief-only toy
- a per-engine feature Claude / Codex / Pi have to implement
- a cloud accessory that needs an API key

The reason the slice is this small is the reason it can merge
everywhere: the rest of the app already knows what to do with text.

## Seam {#seam}

```
composer mic
    → voice/start | voice/stop          (host RPC)
    → host captures + transcribes
    → voice/partial | voice/final       (host notifications)
    → composer input value
    → onSend(text)
    → session/prompt                    (existing, any harness)
```

Claude Code, Codex, and Pi sit *below* `session/prompt`. They never
see audio, a model file, or a permission prompt. One implementation
covers all three engines, Chief, and every template bot. That is
what "merge into the three" means on the runtime side: do not grow
a voice adapter per harness.

The renderer does not capture. WKWebView `getUserMedia` would push
PCM through JavaScript and teach the UI to own a device the host
is supposed to own — the same mistake as letting the UI own ACP
stdio ([#4](../../decisions/issues-4-6.md)). `webkitSpeechRecognition`
is worse: it is not a contract we can keep inside a Tauri WKWebView,
and some implementations are network services.

Linux CI and `jabot-hostd` compile a typed no-op (`available: false`),
matching `#27` notifications.

## Onboarding {#onboarding}

Today the takeover is three cards and a comment that is still true:

> The flow asks two things and teaches one: your name, your default
> engine, then hands you to Chief.

```
STEP 1 OF 3   What should the crew call you?
STEP 2 OF 3   Pick your default engine
STEP 3 OF 3   Chief  →  Enter JaBot
```

Voice is a third *ask*. The wrong reaction is a fourth mandatory
card that makes the first launch feel like a preferences dump.
The wrong reaction the other way is hiding the ask in Settings
until after someone has already ignored a decorative mic for a
week.

**Park the ask on pane 3.** Chief stays the teaching card
(fold / Inbox / the standing thread). Under the existing notes,
one new row:

- Eyebrow stays `STEP 3 OF 3` — the count does not change.
- Heading stays "Chief".
- New copy, one sentence: "Want dictation? The mic in the composer
  can type for you. Nothing is downloaded unless you say yes."
- Two actions, not a checkbox pretending to be one: **Enable
  dictation** and **Not now**. Neither is required to press
  **Enter JaBot**.
- **Enable** sets `voiceEnabled: true` and, once the host is up,
  triggers the OS permission sheet and (Phase B) the model fetch
  *after* the user is in the shell — setup itself does not block
  on a download.
- **Not now** / ignored / Skip / Escape leave `voiceEnabled: false`.
- Skip and Escape keep today's contract: persist whatever was
  drafted (name, engine) and do not surprise-enable a microphone.

If the Chief card gets too tall, split then — and only then — to
`STEP 3 OF 4` *after* the engine pane, keeping panes 1 and 2
untouched. Do not reorder name or engine to make room.

### Profile, versions, existing installs

`jabot.onboarding.v1` is a data-migration tripwire
(`src/__tests__/onboarding-state.test.ts`). Changing the key
re-onboards every install. Do not change it.

`OnboardingProfile` grows one field:

```ts
voiceEnabled: boolean; // missing-on-read ⇒ false
```

`makeProfile` / `saveOnboarding` must write it, or a later "Run
setup again" will strip it — `saveOnboarding` rebuilds the record
from known fields and drops anything else.

`loadOnboarding` already treats `version >= 1` as complete,
including versions this build has never heard of, so a newer
client's extra fields survive a downgrade. **Do not bump `version`
to force the ask.** Existing users keep their shell. They meet
the feature as:

1. a mic that now does something, and
2. a Settings row that says what it does.

"Run setup again" (Crew) can flip the choice, the same way it
can rename the user.

Long term the display name (and this flag) belong on the host —
`state.ts` already says localStorage is the wrong home for a
profile a second device should share. When `user/profile` or
`settings/set` grows the field, the onboarding module stays the
fallback. Do not invent a second store.

### Settings

`SettingsView` is two knobs on purpose. A third knob is allowed
the day `settings/get` returns `voiceEnabled` / `voiceEngine` /
`voiceModelReady` and `settings/set` can change them. Until the
host methods exist, do not draw the row.

Suggested copy, when it does:

- **Dictation** — on / off. Off is the default for anyone who
  never said yes.
- **Engine** — only once Phase B exists. "On-device (macOS)" vs
  "Local Whisper (tiny.en)". Not a model zoo.

Denied TCC is not a silent off. The row says the OS refused, and
how to open the system pane. Same honesty as
`idleTimeoutFromEnv`.

## Protocol {#protocol}

Socket-shaped, like everything else. Suggested methods — names
can move, the shape should not:

| Method / notification | Direction | Job |
|---|---|---|
| `voice/status` | req | `{ available, permission, enabled, engine, modelReady }` |
| `voice/start` | req | Begin capture on this device. Refuse if not enabled, not permitted, or a turn's Stop is showing. |
| `voice/stop` | req | End capture; host still owes a final. |
| `voice/partial` | notif | `{ text }` — replace the live draft suffix, do not send. |
| `voice/final` | notif | `{ text }` — commit into the input. Still do not send. |
| `voice/error` | notif | `{ message }` — permission, engine, or capture. |

`enabled` is the user preference. `available` is "this build can
do it" (macOS, engine compiled). `permission` is TCC.
`modelReady` is always true for Phase A and a real flag for
Phase B.

Start/stop are explicit. No implicit listening when a thread
opens. No start on app launch. Hide-to-Dock (`#4`) must stop an
open capture — a hidden window that keeps the mic is the
opposite of hide-to-Dock.

The composer owns assembly: partials update the in-progress
value; a final replaces that span; Return still calls the same
`onSend`. Auto-send-on-final is a later setting, off by default.
A coding supervisor that fires `rm` because the recognizer heard
"go ahead" is not a cute default.

## Composer wiring

`Composer.tsx` today:

- decorative mic, `aria-label="Voice"`, `type="button"`
- swapped for Stop when `busy && onCancel`
- input never disabled mid-turn (queue, not lock)

The first implementation changes three things and no more:

1. Mic is enabled only when `voice/status` says it can work, or
   it stays visible and explains why on tap (permission, model,
   "turn this on in Settings").
2. Tap toggles start/stop. A later hold-to-talk can reuse the
   same methods. Visual: the existing `.round-btn` in a listening
   state, not a new widget.
3. Incoming `voice/partial` / `voice/final` write the input. They
   never call `onSend`.

Do not add a second mic in the title bar, the sidebar, or the
onboarding card. One affordance, already drawn.

## Tests that have to move

The onboarding suite walks three Continues and Enter
(`src/__tests__/onboarding.test.tsx` `walkToShell`). Adding a
row on pane 3 must not add a click to that walk unless Enable
is the only way out — it must not be.

`onboarding-state.test.ts` pins `ONBOARDING_KEY` and the
blank-name / corrupt-record directions. New cases: missing
`voiceEnabled` reads as false; `makeProfile` cannot drop a true
on save; a v1 record without the field still counts as complete.

Composer tests (when they exist for the mic) need a fake
`voice/*` client, not a live recognizer. Host tests follow the
`#27` split: mac backend behind `cfg`, unsupported backend
answers `available: false` on Linux CI.

## Why this can wait — and what must not drift

The decorative mic can stay decorative until the host methods
exist. What must not happen in the meantime:

- A different pane order on the takeover that this would then
  have to chase.
- A Settings row for dictation before `settings/get` can return
  it.
- A cloud STT "just for now" that teaches the renderer to POST
  audio.
- Per-bot "voice enabled" flags. The preference is the user's,
  on this Mac, for every composer.
