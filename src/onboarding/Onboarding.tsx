//! First-run setup: three quiet card panes in the grammar of a macOS Setup
//! Assistant, rendered *instead of* the shell — during setup no sidebar and no
//! MainView exist. The host handshake is the one thing that does: `App` hoists
//! `useHost()` above the gate and hands this component the status line, so the
//! connection opens while the user reads pane 1 instead of after they finish.
//!
//! The flow asks two things and teaches one: your name (which replaces the
//! old hardcoded USER_NAME), your default engine (which fills NewChatModal's
//! `defaultHarnessId`), then hands you to Chief — the seat the shell opens on.
//!
//! Escape and "Skip setup" are the same handler, and both persist whatever the
//! draft currently holds: a user who typed their name and then bailed is
//! stored under their name, not under "You". This component never touches
//! localStorage — it builds the finished record with `makeProfile` and hands
//! it up, so `App` owns the one write and this stays testable in isolation.

import { useEffect, useId, useRef, useState } from "react";

import { Avatar, CrewAvatar } from "../components/avatar";
import { FieldLabel } from "../components/Modal";
import { HarnessPicker } from "../components/HarnessPicker";
import { initials } from "../components/format";
import type { HarnessCard } from "../components/types";
import {
  DEFAULT_USER_NAME,
  makeProfile,
  type OnboardingProfile,
} from "./state";

export function Onboarding({
  harnesses,
  profile,
  hostLine,
  hostOffline,
  onFinish,
}: {
  harnesses: readonly HarnessCard[];
  /** The record a re-run is editing, absent on a genuine first run. Seeds the
      draft so "Run setup again" can *change* a name rather than only replace
      it — and so Escape or Skip re-persists what was already there. */
  profile?: OnboardingProfile;
  /** The hoisted handshake, made visible: which host, or why there isn't one. */
  hostLine: string;
  hostOffline: boolean;
  onFinish: (profile: OnboardingProfile) => void;
}) {
  const [step, setStep] = useState(0);
  // The fallback name is not a name anyone typed, so it seeds as an empty
  // field (the placeholder says what blank means) rather than as literal text.
  const [name, setName] = useState(
    profile && profile.userName !== DEFAULT_USER_NAME ? profile.userName : "",
  );
  // Initialised to the record being edited, else the first card — so Continue
  // is never blocked and Skip never produces a nonsense default.
  const [harnessId, setHarnessId] = useState<string | null>(
    profile?.harnessId ?? harnesses[0]?.id ?? null,
  );
  const nameId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);

  function finish(skipped: boolean) {
    onFinish(
      makeProfile({
        userName: name,
        harnessId,
        skipped,
        version: profile?.version,
      }),
    );
  }

  // Escape leaves setup the way Skip does — swallowing a platform key in a
  // full-window takeover on a Mac is not defensible. Capture phase, exactly
  // as Modal.tsx binds it.
  const finishRef = useRef(finish);
  finishRef.current = finish;
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      finishRef.current(true);
    }
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, []);

  // Pane 1 puts the caret in the one field; each later pane announces itself
  // by moving focus to its heading — the same intent as Modal's on-open move.
  useEffect(() => {
    if (step === 0) inputRef.current?.focus();
    else headingRef.current?.focus();
  }, [step]);

  return (
    <div className="setup">
      <div className="titlebar-drag" data-tauri-drag-region />

      {/* The one landmark on the first screen a user ever sees. */}
      <main className="setup-stage">
        {step === 0 && (
          // A form, so Enter continues. The card is keyed by step so the
          // entry rise replays on advance.
          <form
            key="name"
            className="setup-card"
            onSubmit={(event) => {
              event.preventDefault();
              setStep(1);
            }}
          >
            <div className="setup-eyebrow">STEP 1 OF 3</div>
            <CrewAvatar className="setup-cluster" />
            <h1 tabIndex={-1} ref={headingRef}>
              What should the crew call you?
            </h1>
            <p className="setup-lede">
              JaBot runs a crew of bots on this Mac. Start with your name — it
              sits at the bottom of the sidebar, next to the host you are
              connected to.
            </p>
            <FieldLabel htmlFor={nameId}>YOUR NAME</FieldLabel>
            <input
              ref={inputRef}
              id={nameId}
              className="mfield"
              type="text"
              value={name}
              placeholder='Defaults to "You"'
              onChange={(event) => setName(event.target.value)}
            />
            <div className="setup-preview">
              <span className="me-face" aria-hidden="true">
                {initials(name.trim() === "" ? DEFAULT_USER_NAME : name)}
              </span>
              <span className="cap">This is your badge in the sidebar.</span>
            </div>
            <div className="setup-foot">
              <button
                type="button"
                className="setup-skip"
                onClick={() => finish(true)}
              >
                Skip setup
              </button>
              <button type="submit" className="btn primary">
                Continue
              </button>
            </div>
          </form>
        )}

        {step === 1 && (
          <div key="engine" className="setup-card">
            <div className="setup-eyebrow">STEP 2 OF 3</div>
            <h1 tabIndex={-1} ref={headingRef}>
              Pick your default engine
            </h1>
            <p className="setup-lede">
              Every new code thread starts here. You can switch engines per
              thread.
            </p>
            <FieldLabel>HARNESS — BRING YOUR OWN</FieldLabel>
            <HarnessPicker
              harnesses={harnesses}
              value={harnessId ?? ""}
              onChange={setHarnessId}
              label="Default harness"
            />
            <div className="setup-foot">
              <button
                type="button"
                className="setup-skip"
                onClick={() => finish(true)}
              >
                Skip setup
              </button>
              <button type="button" className="btn" onClick={() => setStep(0)}>
                Back
              </button>
              <button
                type="button"
                className="btn primary"
                onClick={() => setStep(2)}
              >
                Continue
              </button>
            </div>
          </div>
        )}

        {step === 2 && (
          <div key="chief" className="setup-card">
            <div className="setup-eyebrow">STEP 3 OF 3</div>
            <div className="setup-meet">
              {/* Chief before there is a crew to read one from: the seeded
                  name and colour, so the icon a person meets in setup is the
                  icon waiting in the sidebar afterwards. */}
              <Avatar name="Chief" color="b-teal" />
              <div>
                <h1 tabIndex={-1} ref={headingRef}>
                  Chief
                </h1>
                <p className="setup-lede">Runs the crew</p>
              </div>
              <span className="chief-badge">CHIEF</span>
            </div>
            <blockquote className="setup-quote">
              Route work across the crew. Fold long tasks away, surface only
              what matters.
            </blockquote>
            <p className="setup-note">
              Long jobs fold out of the sidebar and keep running. They come back
              through the Inbox when they are done — or when they need you.
            </p>
            <p className="setup-note">
              Chief is already offering to fold one away in the chat you are
              about to land in — take it, and watch the Inbox bring the work
              back.
            </p>
            <div className="setup-foot">
              <button type="button" className="btn" onClick={() => setStep(1)}>
                Back
              </button>
              <button
                type="button"
                className="btn primary"
                onClick={() => finish(false)}
              >
                Enter JaBot
              </button>
            </div>
          </div>
        )}
      </main>

      <div className={hostOffline ? "setup-host bad" : "setup-host"}>
        {hostLine}
      </div>
    </div>
  );
}
