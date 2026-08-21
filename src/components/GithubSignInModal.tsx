//! Sign in to GitHub, so the board can show every pull request you have open.
//!
//! The flow is a token paste rather than an OAuth round trip, and that is the
//! decision `folders-and-auth.md` made: JaBot ships no GitHub App and no OAuth
//! client id, because a public native binary cannot keep a client secret, and
//! because `gh` is already the login every coding agent on this Mac uses. What
//! this dialog collects, the host hands to `gh auth login --with-token`.
//!
//! So the two steps are as short as they can honestly be: open the page that
//! mints the token with the scopes already ticked, and paste what it prints.
//! The field is a password field — a token is a password, and this window gets
//! screen-shared.
//!
//! A machine with no `gh` at all gets the install line instead of the form.
//! Offering a paste box to somebody with nowhere to put the token would fail
//! at the last step for a reason they could have been told first.

import { useId, useState } from "react";

import { FieldLabel, Modal } from "./Modal";
import { tokenUrl, TOKEN_SCOPES } from "../views/github";

export function GithubSignInModal({
  host,
  installed,
  installHint,
  onSignIn,
  onCancel,
  onOpenUrl,
}: {
  host: string;
  /** Whether `gh` is on this machine at all. */
  installed: boolean;
  /** The host's own remedy for a missing `gh`, when it gave one. */
  installHint?: string;
  /** Resolves once the host has the token; rejects with GitHub's refusal. */
  onSignIn: (token: string) => Promise<void>;
  onCancel: () => void;
  onOpenUrl: (url: string) => void;
}) {
  const tokenId = useId();
  const [token, setToken] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit() {
    if (!token.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSignIn(token.trim());
      // Cleared before the dialog goes, so a re-open never re-shows a secret
      // and nothing holds it once the host has it.
      setToken("");
      onCancel();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  }

  if (!installed) {
    return (
      <Modal title="Sign in to GitHub" onClose={onCancel}>
        <p className="modal-note">
          JaBot signs in through the GitHub CLI, the same login your coding
          agents already use — and it is not installed on this Mac yet.
        </p>
        <p className="modal-note">
          <code>{installHint ?? "brew install gh"}</code>
        </p>
        <div className="macts">
          <button type="button" className="btn" onClick={onCancel}>
            Close
          </button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal title="Sign in to GitHub" onClose={onCancel}>
      <p className="modal-note">
        JaBot signs in through the GitHub CLI, so this is the same login your
        coding agents use. The token goes straight to <code>gh</code>, which
        keeps it in your Keychain — JaBot never stores it.
      </p>

      <FieldLabel>STEP 1 — MAKE A TOKEN</FieldLabel>
      <div className="macts start">
        <button
          type="button"
          className="btn"
          onClick={() => onOpenUrl(tokenUrl(host))}
        >
          Open {host}
        </button>
      </div>
      <p className="modal-note">
        The page opens with <code>{TOKEN_SCOPES}</code> already ticked — enough
        to read your pull requests, including private and SSO-protected ones.
      </p>

      <FieldLabel htmlFor={tokenId}>STEP 2 — PASTE IT HERE</FieldLabel>
      <input
        id={tokenId}
        type="password"
        autoComplete="off"
        spellCheck={false}
        value={token}
        placeholder="ghp_… or github_pat_…"
        onChange={(event) => setToken(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") void submit();
        }}
      />

      {error && (
        <p className="modal-error" role="alert">
          {error}
        </p>
      )}

      <div className="macts">
        <button type="button" className="btn" onClick={onCancel}>
          Cancel
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={!token.trim() || busy}
          onClick={submit}
        >
          {busy ? "Signing in…" : "Sign in"}
        </button>
      </div>
    </Modal>
  );
}
