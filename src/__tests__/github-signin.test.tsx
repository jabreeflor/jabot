/**
 * Signing in to GitHub from the Pull Requests tab (#28).
 *
 * Three claims, and they are the ones a user would notice if they broke.
 *
 * The board is never *gated* on a login: the rows are linkage, they needed no
 * credential to collect, and a signed-out user still sees everything a session
 * on this Mac opened. What the strip adds is an offer.
 *
 * The dialog is honest about where the token goes — and it goes nowhere but
 * the host. Nothing here keeps it after the call, and the field is a password
 * field because a token is a password.
 *
 * A refusal from GitHub stays on screen with the form intact. A user whose
 * paste was wrong needs to try again, not to start over.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { GithubSignInModal } from "../components/GithubSignInModal";
import { PullRequestsView } from "../views/PullRequestsView";
import { tokenUrl } from "../views/github";
import type { GithubStatusResult } from "../host";
import type { PullRequest } from "../components/types";

const ROWS: PullRequest[] = [
  {
    id: "pr-23",
    threadId: "auth",
    provider: "github",
    repo: "jabreeflor/jabot",
    number: 23,
    url: "https://example.invalid/23",
    title: "Migrate auth to sessions",
    status: "open",
    checkState: "passing",
    updatedAt: "2026-08-21T09:14:02Z",
    additions: 214,
    deletions: 96,
  },
];

function status(over: Partial<GithubStatusResult> = {}): GithubStatusResult {
  return {
    installed: true,
    authenticated: false,
    host: "github.com",
    detail: "Not logged in to github.com.",
    remedy: "gh auth login --hostname github.com",
    ...over,
  };
}

describe("the sign-in strip above the board", () => {
  it("offers a sign-in without hiding the pull requests behind it", async () => {
    const onSignIn = vi.fn();
    render(
      <PullRequestsView
        pullRequests={ROWS}
        githubStatus={status()}
        onSignIn={onSignIn}
        onOpenThread={vi.fn()}
      />,
    );

    // The offer is there…
    const button = screen.getByRole("button", { name: "Sign in with GitHub" });
    expect(screen.getByRole("status").textContent).toContain(
      "every pull request you have open",
    );
    // …and so is the board underneath it.
    expect(screen.getByText("Migrate auth to sessions")).toBeInTheDocument();

    await userEvent.click(button);
    expect(onSignIn).toHaveBeenCalledOnce();
  });

  it("says who the board belongs to once there is somebody", () => {
    render(
      <PullRequestsView
        pullRequests={ROWS}
        githubStatus={status({ authenticated: true, account: "octocat" })}
        account="octocat"
        onSignIn={vi.fn()}
        onOpenThread={vi.fn()}
      />,
    );

    expect(
      screen.getByText(/Showing every pull request you have open as @octocat/),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Sign in with GitHub" }),
    ).not.toBeInTheDocument();
  });

  it("draws no strip at all until the host has answered", () => {
    render(
      <PullRequestsView
        pullRequests={ROWS}
        githubStatus={null}
        onSignIn={vi.fn()}
        onOpenThread={vi.fn()}
      />,
    );

    // An offer that cannot be honoured — no host to sign in to — is worse than
    // no offer, and a preview build is exactly that case.
    expect(
      screen.queryByRole("button", { name: "Sign in with GitHub" }),
    ).not.toBeInTheDocument();
  });
});

describe("the sign-in dialog", () => {
  it("opens the token page with the scopes already ticked, then takes the paste", async () => {
    const onOpenUrl = vi.fn();
    const onSignIn = vi.fn().mockResolvedValue(undefined);
    const onCancel = vi.fn();
    render(
      <GithubSignInModal
        host="github.com"
        installed
        onSignIn={onSignIn}
        onCancel={onCancel}
        onOpenUrl={onOpenUrl}
      />,
    );

    await userEvent.click(screen.getByRole("button", { name: /Open github/ }));
    expect(onOpenUrl).toHaveBeenCalledWith(tokenUrl("github.com"));
    expect(tokenUrl("github.com")).toContain("scopes=repo%2Cread%3Aorg");

    // A token is a password, and this window gets screen-shared.
    const field = document.querySelector<HTMLInputElement>("input");
    expect(field?.type).toBe("password");

    await userEvent.type(field!, "ghp_pretendtoken");
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));

    expect(onSignIn).toHaveBeenCalledWith("ghp_pretendtoken");
    // The dialog closes itself on success, and nothing keeps the token.
    expect(onCancel).toHaveBeenCalledOnce();
    expect(field!.value).toBe("");
  });

  it("keeps the form and GitHub's own words when the token is refused", async () => {
    const onSignIn = vi
      .fn()
      .mockRejectedValue(new Error("HTTP 401: Bad credentials"));
    const onCancel = vi.fn();
    render(
      <GithubSignInModal
        host="github.com"
        installed
        onSignIn={onSignIn}
        onCancel={onCancel}
        onOpenUrl={vi.fn()}
      />,
    );

    await userEvent.type(
      document.querySelector<HTMLInputElement>("input")!,
      "ghp_wrongtoken",
    );
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "HTTP 401: Bad credentials",
    );
    // Still open, still holding what was typed: the fix is one edit away.
    expect(onCancel).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Sign in" })).toBeEnabled();
  });

  it("sends a machine with no gh to the install line, not to a paste box", () => {
    render(
      <GithubSignInModal
        host="github.com"
        installed={false}
        installHint="brew install gh"
        onSignIn={vi.fn()}
        onCancel={vi.fn()}
        onOpenUrl={vi.fn()}
      />,
    );

    expect(screen.getByText("brew install gh")).toBeInTheDocument();
    expect(document.querySelector("input")).toBeNull();
  });
});
