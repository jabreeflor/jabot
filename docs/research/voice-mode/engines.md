# Voice engines

Question 4, 6, and 7 from [`brief.md`](brief.md). Locked by
[`findings.md`](findings.md): dictation first, host-owned, no cloud
default, no per-harness engine.

The job is narrow. A user holds (or taps) the existing mic, speaks a
sentence they would otherwise type, and sees words appear in the
composer. Accuracy has to be "good enough to edit", not "good enough
to send unseen." Latency has to feel like a keystroke burst, not a
file upload.

## Candidates

| Engine | Cost | Bundle | Offline | Why it fits or doesn't |
|---|---|---|---|---|
| **`SFSpeechRecognizer` (Apple Speech)** | Free | 0 bytes | On-device on current macOS | Same stack Notes and Messages use. TCC prompt the user already understands. Fast. English + the OS language. **Phase A.** |
| **whisper.cpp `tiny.en-q5_1`** | Free, MIT-ish model weights | 31 MiB | Yes | Light enough to fetch after Yes. Metal on Apple Silicon. Host crate: `whisper-rs`. **Phase B default.** |
| whisper.cpp `tiny.en` | Free | 75 MiB | Yes | Same quality class as q5_1, more than twice the disk. Skip unless a machine can't run quantized. |
| whisper.cpp `base.en` | Free | 142 MiB | Yes | Better WER. Settings upgrade, not the default. |
| whisper.cpp `small.en` and up | Free | 466 MiB–2.9 GiB | Yes | Not "lightweight". Do not bundle. |
| **`AVSpeechSynthesizer` / `say`** | Free | 0 bytes | Yes | Phase C spoken replies. System voices. No Piper/Kokoro until someone hates the system voice enough to say so. |
| Web Speech API in WKWebView | "Free" | 0 bytes | Often not | Safari's recognition is not a thing we can promise, and some paths phone home. The renderer must not own this. |
| OpenAI Whisper API | Paid, network | 0 bytes | No | Cloud. Contradicts "free or local" and the no-account product. |
| ElevenLabs / Happy's toy | Paid, network | 0 bytes | No | Already deferred in mobile research. Do not sneak it onto desktop. |
| `tauri-plugin-stt` as a dep | Free | plugin + its catalogue | After download | Right *engine* (whisper-rs on desktop), wrong *ownership*. It downloads from HuggingFace under its own lifecycle. JaBot already has a host protocol and a settings store. Speak `voice/*`, call `whisper-rs` ourselves. |
| Vosk / sherpa-onnx / Moonshine | Free | varies | Yes | Fine models, extra format and packaging story. whisper.cpp is the one the Tauri/Rust world already builds on Metal. |

Phase A and Phase B share the same host methods. The renderer never
names an engine. `voice/status` reports `{ available, permission,
engine, modelReady }` so the mic can disable itself with a reason
instead of failing silently.

## Phase A — Apple Speech

Ship this with the first composer wiring. Reasons:

- The `.dmg` does not grow. Packaging (#12) is already a notarized
  universal image; adding tens of megabytes for a feature nobody has
  opted into yet is the wrong first move.
- The OS prompt is the real onboarding. A Yes on the Chief pane that
  does not then show the system sheet is a lie.
- On-device recognition on current macOS does not need a model file
  we vendor or update.
- A Linux CI host and `jabot-hostd` compile the unsupported backend
  and answer `available: false`. Same pattern as `#27`.

Limits we accept: quality follows the OS; some locales are better
than others; Apple can still prefer a network recognizer for some
languages. The usage string has to say we stay on-device when the
OS lets us, and the host must request the on-device path
(`requiresOnDeviceRecognition`) so a "local" opt-in does not
quietly become a cloud call.

## Phase B — bundled-capable whisper.cpp {#whisper}

This is the "something lightweight we bundle" the product asked for.
It is **not** day one, and it is **not** a second mic.

Default artifact: **`ggml-tiny.en-q5_1.bin` (~31 MiB)**.

```
Yes on Chief
    → host starts the install (or copies from Resources if we shipped it)
    → progress is a notification, not a blocking setup pane
    → modelReady flips; the mic starts working
Not now
    → nothing is downloaded
    → Settings can still do it later
```

Two ways to get the bytes, in order of preference:

1. **First-use fetch from our release assets.** We vendor the exact
   ggml file next to `latest.json`, checksum it, and put it in
   `~/Library/Application Support/jabot/voice/`. The `.dmg` stays
   small. A user who said no pays nothing.
2. **Copy out of `JaBot.app/Contents/Resources/voice/`** if we later
   decide the extra 31 MiB in every download is cheaper than a
   first-run network fetch. That is a packaging change, not an API
   change.

Do not fetch from HuggingFace at runtime. Release assets we sign
and pin are the same rule the updater already lives under
([packaging.md](../../packaging.md)).

Runtime: `whisper-rs` with Metal on Apple Silicon, CPU fallback on
Intel. Capture via `cpal` or `AVAudioEngine` in the host — not
`getUserMedia` in the webview. Inference is one utterance per
start/stop, not a rolling always-on window.

`base.en` (142 MiB) is a Settings upgrade for people who dictate
long technical sentences and hate editing. It is never the thing
setup downloads.

## Packaging {#packaging}

`entitlements.plist` is an audit. Today it lists camera, microphone,
location, and friends as *deliberately absent* so an unused device
entitlement cannot show up in a Gatekeeper prompt. The first engine
PR must:

1. Add the microphone entitlement (Hardened Runtime / TCC; we are
   not sandboxed, but the usage string and the prompt still exist).
2. Write the justification in the same comment block: "host-owned
   dictation for the composer; audio never leaves the machine on
   Phase A/B."
3. Add `NSMicrophoneUsageDescription` and, for Phase A,
   `NSSpeechRecognitionUsageDescription` to the bundled Info.plist
   (`src-tauri/tauri.conf.json` `bundle.macOS`).

Universal binary math: model files are architecture-independent.
A 31 MiB ggml is 31 MiB, not 62. The whisper-rs / ggml *code* is
what gets fat, and it is small next to the model.

Do not add `com.apple.security.device.audio-input` as cargo cult
if the Developer ID build already reaches the mic through TCC
alone — confirm on a notarized build, then record the observed
fact in the entitlements file, the same way library-validation
was recorded as absent.

## Spoken replies {#tts}

Out of the first slice on purpose.

Dictation is a hole in the UI we already drew. Spoken replies are
a new channel: when does Chief talk, what does it do with a
folded thread, how do we not read a 400-line diff aloud, how does
the user barge in. That is a product, not a checkbox.

When we do it:

- Engine: `AVSpeechSynthesizer` (or `say`) — free, local, zero
  bytes. Piper/Kokoro only if the system voice is the complaint.
- Scope: Chief standing thread, opt-in, short replies. Never
  auto-speak a code thread.
- Same host ownership. A `voice/speak` method, cancellable, quiet
  when the window is not frontmost unless the user asked.

Hermes excluding TTS from its ACP adapter is the hint: spoken
output is not the harness's job either.

## What we are not deciding

- A "better" multilingual default. `tiny.en` is the English-first
  Mac app. A multilingual `tiny` can be a Settings row later.
- Shipping an LLM to rewrite the transcript. If the words are
  wrong, the user edits the box. That is what the box is for.
- Android / iOS speech. Mobile research already said no.
