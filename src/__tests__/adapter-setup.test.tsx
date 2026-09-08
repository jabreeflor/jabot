import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AdapterSetup } from "../onboarding/AdapterSetup";
import type { HostClient } from "../host";

const missing = {
  id: "codex",
  ready: false,
  status: "adapter_missing",
  detail: "Codex adapter is missing.",
  remedy: "Install the adapter.",
};
function mount(over: Record<string, unknown> = {}) {
  const client = {
    harnessDoctor: vi.fn().mockResolvedValue({ reports: [missing] }),
    installHarness: vi.fn().mockResolvedValue({ running: false, error: null }),
    ...over,
  };
  const onContinue = vi.fn();
  render(
    <AdapterSetup
      client={client as unknown as HostClient}
      harnessId="codex"
      onContinue={onContinue}
    />,
  );
  return { client, onContinue };
}
describe("onboarding adapter setup", () => {
  it("installs the selected adapter and rechecks readiness before continuing", async () => {
    const { client, onContinue } = mount();
    await screen.findByText("Codex adapter is missing.");
    client.harnessDoctor.mockResolvedValue({
      reports: [{ ...missing, ready: true, status: "ready", detail: "Ready" }],
    });
    await userEvent.click(
      screen.getByRole("button", { name: "Install adapter" }),
    );
    expect(client.installHarness).toHaveBeenCalledWith("codex", true);
    await screen.findByText("Your engine and adapter are ready.");
    expect(client.installHarness).toHaveBeenCalledWith("codex");
    expect(client.harnessDoctor).toHaveBeenLastCalledWith({
      harnessId: "codex",
    });
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(onContinue).toHaveBeenCalledOnce();
  });
  it("keeps install failure visible and allows retry or skip", async () => {
    const installHarness = vi
      .fn()
      .mockResolvedValueOnce({ running: true, error: null })
      .mockResolvedValueOnce({
        running: false,
        error: "Network unavailable. Retry or skip.",
      });
    const { onContinue } = mount({ installHarness });
    await screen.findByText("Codex adapter is missing.");
    await userEvent.click(
      screen.getByRole("button", { name: "Install adapter" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Network unavailable",
    );
    expect(
      screen.getByRole("button", { name: "Install adapter" }),
    ).toBeEnabled();
    await userEvent.click(screen.getByRole("button", { name: "Skip for now" }));
    expect(onContinue).toHaveBeenCalledOnce();
  });
  it("recovers when polling an ongoing install loses the connection", async () => {
    const installHarness = vi
      .fn()
      .mockResolvedValueOnce({ running: true, error: null })
      .mockResolvedValueOnce({ running: true, error: null })
      .mockRejectedValueOnce(new Error("Connection lost"))
      .mockResolvedValue({ running: false, error: null });
    mount({ installHarness });
    await screen.findByText("Codex adapter is missing.");
    await userEvent.click(
      screen.getByRole("button", { name: "Install adapter" }),
    );
    expect(
      await screen.findByRole("alert", {}, { timeout: 3000 }),
    ).toHaveTextContent("Connection lost");
    expect(screen.getByRole("button", { name: "Retry check" })).toBeEnabled();
    await userEvent.click(
      screen.getByRole("button", { name: "Install adapter" }),
    );
    await screen.findByText("Codex adapter is missing.");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
  it("does not offer an npm install for Aider", async () => {
    const client = {
      harnessDoctor: vi.fn().mockResolvedValue({
        reports: [
          {
            id: "aider",
            ready: false,
            status: "cli_missing",
            detail: "Aider is not installed — no `aider` on PATH.",
            remedy: "Install Aider (`python -m pip install aider-chat`).",
          },
        ],
      }),
      installHarness: vi.fn(),
    };
    render(
      <AdapterSetup
        client={client as unknown as HostClient}
        harnessId="aider"
        onContinue={vi.fn()}
      />,
    );
    expect(
      await screen.findByText(/Aider is not installed/),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Install adapter" }),
    ).not.toBeInTheDocument();
    expect(client.installHarness).not.toHaveBeenCalled();
  });

  it("shows a recoverable doctor error instead of claiming success", async () => {
    const { client } = mount({
      harnessDoctor: vi
        .fn()
        .mockRejectedValueOnce(new Error("Host offline"))
        .mockResolvedValue({ reports: [missing] }),
    });
    expect(await screen.findByRole("alert")).toHaveTextContent("Host offline");
    await userEvent.click(screen.getByRole("button", { name: "Retry check" }));
    await waitFor(() => expect(client.harnessDoctor).toHaveBeenCalledTimes(2));
    await screen.findByText("Codex adapter is missing.");
  });
  it("offers Gemini CLI install with a separate sign-in note", async () => {
    const client = {
      harnessDoctor: vi.fn().mockResolvedValue({
        reports: [
          {
            id: "gemini",
            ready: false,
            status: "cli_missing",
            detail: "Gemini CLI is not installed.",
            remedy: "Install Gemini CLI.",
          },
        ],
      }),
      installHarness: vi
        .fn()
        .mockResolvedValue({ running: false, error: null }),
    };
    render(
      <AdapterSetup
        client={client as unknown as HostClient}
        harnessId="gemini"
        onContinue={() => {}}
      />,
    );
    expect(
      await screen.findByText(/Install Gemini CLI into ~\/.local/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Install adapter" }),
    ).toBeEnabled();
  });
});
