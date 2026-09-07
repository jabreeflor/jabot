# Voice mode — Findings

Researched September 2026 against the current tree (`src/onboarding/`,
`src/components/Composer.tsx`, `src-tauri/entitlements.plist`, host
`settings/*`) and the local-STT options that actually fit a macOS Tauri
2 `.dmg`. This file answers the seven questions in [`brief.md`](brief.md).
Deep dives: [`engines.md`](engines.md),
[`onboarding-and-composer.md`](onboarding-and-composer.md).

**Recommendation in one sentence:** Voice is **composer dictation**,
owned by the **Rust host**, filling the same text box every bot already
uses — so it merges into Claude, Codex, and Pi without touching a
harness. Ask during the existing three-pane setup (parked on Chief, not
a fourth card). Ship Apple on-device Speech first (zero extra bytes);
keep a **whisper.cpp `tiny.en-q5_1` (~31 MiB)** slot for the lightweight
bundled model, fetched or copied only after the user says yes.

| Question | Short answer | Detail |
|---|---|---|
| 1. What is it? | Push-to-talk dictation into the composer. User still sends. No wake word. No duplex "voice mode". | [onboarding-and-composer.md](onboarding-and-composer.md#product) |
| 2. Where? | Every composer (Chief and every code thread). Never per-harness. | [onboarding-and-composer.md](onboarding-and-composer.md#seam) |
| 3. Onboarding | Fold the Yes / Not now ask onto pane 3 (Chief). Do not re-onboard existing installs. | [onboarding-and-composer.md](onboarding-and-composer.md#onboarding) |
| 4. Engine | Phase A: `SFSpeechRecognizer` on-device. Phase B: whisper.cpp `tiny.en-q5_1`. Never ElevenLabs / OpenAI Whisper API as the default. | [engines.md](engines.md) |
| 5. Host vs UI | Host captures and transcribes. Renderer only starts/stops and inserts text. No Web Speech API. | [onboarding-and-composer.md](onboarding-and-composer.md#protocol) |
| 6. Packaging | Document the microphone entitlement when the first engine lands. Do not ship 75–150 MiB in every `.dmg` on day one. | [engines.md](engines.md#packaging) |
| 7. Spoken replies | Later. `AVSpeechSynthesizer` / `say`, same host slot. Not in the first slice. | [engines.md](engines.md#tts) |

## What this unblocks

The brief's "What this blocks" list can become issues, in this order:

1. **Host `voice/*` API** — `voice/status`, `voice/start`, `voice/stop`
   plus `voice/partial` / `voice/final` / `voice/error` notifications.
   macOS-only backend, no-op elsewhere, same split as `#27` notifications
   (`notify/mac.rs` vs `notify/unsupported.rs`). See
   [onboarding-and-composer.md](onboarding-and-composer.md#protocol).
2. **Composer mic wiring** — the existing `aria-label="Voice"` button
   in `src/components/Composer.tsx` becomes start/stop. Partial text
   lands in the input; the user still hits Return. Stop-while-busy
   keeps the mic's slot, exactly as today.
3. **Onboarding opt-in + Settings toggle** — Chief pane grows a Yes /
   Not now row. `OnboardingProfile` gains `voiceEnabled` without bumping
   `jabot.onboarding.v1` or re-running setup. Settings gets the same
   knob the day the host can honour it (Settings still refuses controls
   that decide nothing). See
   [onboarding-and-composer.md](onboarding-and-composer.md#onboarding).
4. **Phase B local model** — whisper.cpp via `whisper-rs` + Metal,
   default `tiny.en-q5_1` (~31 MiB). Same `voice/*` methods. Install
   on Yes or on first mic tap, not in the cold `.dmg`. See
   [engines.md](engines.md#whisper).
5. **Spoken replies (later)** — host-owned TTS, opt-in, Chief first.
   Still not a duplex conversation and still not ElevenLabs.
6. **Entitlements audit** — `src-tauri/entitlements.plist` currently
   records microphone as *deliberately absent*. The first engine PR
   has to add the key and the justification in the same file, plus
   `NSMicrophoneUsageDescription` / `NSSpeechRecognitionUsageDescription`.

Do **not** open a per-harness issue. Do **not** open a mobile-voice
issue — [remote-and-mobile](../remote-and-mobile/findings.md) already
deferred it.

## Locked constraints (from earlier research)

Do not relitigate these here:

- The UI never owns ACP stdio. Voice is the same rule: the UI never
  owns the microphone stream south of the host API.
  ([#4](../../decisions/issues-4-6.md))
- Every bot is an ACP harness session. Voice is **not** a fourth
  runtime and not a host-owned LLM loop.
  ([#6](../../decisions/issues-4-6.md))
- Settings only draws knobs the host already honours
  (`src/views/SettingsView.tsx`, `src-tauri/src/host/settings.rs`).
- Entitlements without a justification are a bug
  (`src-tauri/entitlements.plist`).
- Mobile voice stays later
  ([pairing-security-mobile.md](../remote-and-mobile/pairing-security-mobile.md)).

## What we explicitly defer

- Always-on listening, wake words, or a floating "voice mode" that
  replaces the transcript.
- Cloud STT/TTS as the default path (OpenAI Whisper API, ElevenLabs,
  Happy's toy).
- Per-harness or per-bot voice implementations.
- Re-onboarding existing installs to ask the new question.
- Shipping `tiny.en` / `base.en` inside every `.dmg` before anyone
  has said yes.
- LLM "cleanup" of the transcript (OpenWhisper-style Gemma). The
  composer is a text box; the user edits.
- Spoken replies, duplex barge-in, or reading Inbox cards aloud.
- Linux CI actually capturing a microphone — the unsupported backend
  is a typed no-op, like notifications.

## Prototype note

The clickable contract is
[`prototypes/jabot-voice.html`](../../../prototypes/jabot-voice.html).
It is the first-run ask, the composer mic, the off/denied hint, and
the Settings row — same tokens as the shell, scripted speech rather
than a real microphone (the renderer must not own one). Deep links
are listed at the top of that file; screenshots live in
[`docs/img/voice-mode/`](../../img/voice-mode/).

`src/components/Composer.tsx` already encodes the affordance: a
round mic button, `aria-label="Voice"`, swapped for Stop while a
turn is running. `Icon.tsx` calls it "the composer's decorative mic."
Treat that as the product claim, not a working control — the same
way the header's monitor icon claimed multi-host before pairing
existed. The first implementation should light *that* button, not
invent a second one.

The onboarding takeover already has the grammar the opt-in has to
speak: one card, an eyebrow (`STEP n OF 3`), Skip / Escape persist
whatever is drafted, extra profile fields must survive a downgrade
(`loadOnboarding` in `src/onboarding/state.ts`).

## Sources

- This repo: `src/onboarding/Onboarding.tsx`, `src/onboarding/state.ts`,
  `src/components/Composer.tsx`, `src-tauri/entitlements.plist`,
  `src-tauri/src/host/settings.rs`, `src/views/SettingsView.tsx`
- whisper.cpp model table:
  [ggml-org/whisper.cpp models](https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md)
  (`tiny.en` 75 MiB, `tiny.en-q5_1` 31 MiB, `base.en` 142 MiB)
- Apple Speech: `SFSpeechRecognizer` on-device on current macOS;
  requires Speech + microphone TCC usage strings
- `whisper-rs` + Metal for in-process whisper.cpp (same stack
  `tauri-plugin-stt` uses on desktop — we own the RPC, we do not
  take that plugin's HuggingFace downloader as a dependency)
- Prior art already in-tree: Hermes ACP excludes TTS;
  Happy/11Labs called out as later in mobile research
