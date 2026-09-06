# Voice mode

JaBot already draws a mic on every composer and labels it "Voice". The
button does nothing. First-run setup is three quiet panes that ask two
things (name, default engine) and teach one (Chief). The question is how
to make that mic real — dictation into the same text box, optionally a
spoken reply later — without a cloud STT bill, without a second product,
and without a fourth mandatory setup card.

This is **not** a harness feature. Claude Code, Codex, and Pi already
consume text on `session/prompt`. Voice has to land *above* that seam so
it merges into all three engines on day one.

Prior art we already recorded: Hermes's ACP adapter **excludes** TTS
([setup-porting/hermes.md](../setup-porting/hermes.md)); mobile research
explicitly deferred "voice (Happy's 11Labs toy)"
([remote-and-mobile/pairing-security-mobile.md](../remote-and-mobile/pairing-security-mobile.md)).
Those are the things not to copy.

Constrained by: [app-shell](../app-shell/brief.md) (Tauri host owns
devices; WKWebView owns none), [host API](../../requirements/host-api-protocol.md)
(socket-shaped JSON-RPC), [onboarding](../../requirements/ui-shell.md)
(`src/onboarding/`), [packaging](../../packaging.md) (universal `.dmg`,
empty entitlements audit).

## Questions to answer

1. **What is "voice mode"** — dictation into the composer, a spoken
   duplex conversation, wake-word always-on, or something else? What is
   the first shippable slice, and what is later?
2. **Where does it attach** — composer only, Chief only, every bot, or
   per-harness? How does that merge into Claude / Codex / Pi without
   three implementations?
3. **Onboarding** — do we add a pane, fold the ask into one of the
   existing three, or leave it for Settings and the first mic tap? What
   does Skip / Escape / "Run setup again" do? How does the stored
   profile grow without re-onboarding everyone?
4. **Engine** — Apple Speech, a bundled local model (which one, how
   big), a download-on-yes sidecar, or a cloud API? What is free, what
   is local, what is light enough to live inside `JaBot.app`?
5. **Host vs renderer** — who captures the microphone, who transcribes,
   what RPCs and notifications exist, and why not `webkitSpeechRecognition`?
6. **Permissions and packaging** — entitlements, usage strings, TCC
   timing, bundle-size budget, Intel vs Apple Silicon.
7. **Spoken replies** — when, which engine, and why they are not in
   the first slice.

## What this blocks (future issues)

- Host `voice/*` API + notifications
- Composer mic wiring (the decorative button in `Composer.tsx`)
- Onboarding opt-in + Settings toggle
- A bundled or first-use local STT model
- Optional TTS for Chief replies
- Entitlements audit update for the microphone
