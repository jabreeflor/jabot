//! Signing in to GitHub, from the renderer's side (#28).
//!
//! There is no JaBot GitHub App and no OAuth client id, and that is a decision
//! rather than an omission (`folders-and-auth.md`): a native binary cannot keep
//! a client secret, and every coding agent on this Mac already runs `gh`. So
//! "Sign in with GitHub" means *give `gh` a token to hold* — the host hands it
//! straight to `gh auth login --with-token`, which stores it in the Keychain
//! where `gh` keeps its own. One login, one identity, one place to revoke.
//!
//! Two consequences shape this file.
//!
//! **The status is three facts, not a boolean.** `installed` and
//! `authenticated` have different remedies — a user who has never installed
//! `gh` must not be sent to a login form — and the host has said so since #16.
//! This hook passes them through unflattened.
//!
//! **The token never comes back.** [`GithubAuth.signIn`] takes one and returns
//! the status; nothing here keeps it in React state a moment longer than the
//! call, and no host method can read it back out afterwards.

import { useCallback, useEffect, useState } from "react";

import type { GithubStatusResult, HostClient } from "../host";

/** The forge the desktop signs in to unless a folder says otherwise. */
export const DEFAULT_HOST = "github.com";

/**
 * What a token needs to be able to do for the board to work.
 *
 * `repo` because a private repository's pull requests are invisible without
 * it, and `read:org` because SSO-protected organisations answer nothing at all
 * to a token that cannot see them. Neither grants anything JaBot writes with:
 * every mutation on a pull request still happens on GitHub, in the browser.
 */
export const TOKEN_SCOPES = "repo,read:org";

export interface GithubAuth {
  /** `null` until the host answers — a preview build, or a unit test. */
  status: GithubStatusResult | null;
  /** An RPC that failed outright, as opposed to a report of being logged out. */
  error: string | null;
  /** True once GitHub can be asked as somebody. */
  signedIn: boolean;
  reload: () => void;
  /** Hand the host a token. Rejects with the host's own sentence when GitHub
      refused it — a person is waiting at the dialog to be told. */
  signIn: (token: string) => Promise<void>;
}

export function useGithubAuth(client: HostClient | null): GithubAuth {
  const [status, setStatus] = useState<GithubStatusResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [generation, setGeneration] = useState(0);

  useEffect(() => {
    if (!client) return;
    let cancelled = false;
    // Guarded as a whole, method lookup included: a transport that predates
    // `github/status` should leave the strip unsaid rather than take the
    // render down.
    (async () => client.githubStatus())()
      .then((answer) => {
        if (cancelled) return;
        setStatus(answer);
        setError(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [client, generation]);

  const reload = useCallback(() => setGeneration((n) => n + 1), []);

  const signIn = useCallback(
    async (token: string) => {
      if (!client) throw new Error("No host to sign in to.");
      // Deliberately not caught: the dialog shows the refusal, and swallowing
      // it here would leave a spinner claiming success.
      const answer = await client.githubLogin({ token });
      setStatus(answer);
      setError(null);
    },
    [client],
  );

  return {
    status,
    error,
    signedIn: status?.authenticated === true,
    reload,
    signIn,
  };
}

/**
 * Where a user makes the token this asks for, with the scopes pre-ticked.
 *
 * GitHub's own new-token page reads `scopes` and `description` out of the
 * query string, so the browser opens on a form that is already filled in and
 * the only step left is Generate. GHES serves the same page under its own
 * host.
 */
export function tokenUrl(host: string = DEFAULT_HOST): string {
  const at = host.trim() || DEFAULT_HOST;
  const params = new URLSearchParams({
    scopes: TOKEN_SCOPES,
    description: "JaBot",
  });
  return `https://${at}/settings/tokens/new?${params.toString()}`;
}
