import { useEffect, useState } from "react";
import type { HostClient } from "../host";
import type { HarnessReport } from "../host/protocol";

/** Readiness and explicit installation happen before the first conversation. */
export function AdapterSetup({
  client,
  harnessId,
  onContinue,
}: {
  client: HostClient | null;
  harnessId: string;
  onContinue: () => void;
}) {
  const [report, setReport] = useState<HarnessReport | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [installing, setInstalling] = useState(false);
  const supported = ["claude", "codex", "pi", "gemini"].includes(harnessId);
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    setBusy(true);
    setError(null);
    async function check() {
      try {
        if (!client || typeof client.harnessDoctor !== "function")
          throw new Error("Connect to the host, then retry the adapter check.");
        if (installing) {
          const status = await client.installHarness(harnessId);
          if (!active) return;
          if (status.running) {
            timer = setTimeout(() => void check(), 1000);
            return;
          }
          setInstalling(false);
          if (status.error) throw new Error(status.error);
        }
        const result = await client.harnessDoctor({ harnessId });
        if (!active) return;
        const next = result.reports.find((row) => row.id === harnessId);
        if (!next)
          throw new Error(
            "The host did not return this harness. Retry or choose another engine.",
          );
        setReport(next);
      } catch (cause) {
        if (active) {
          setInstalling(false);
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      } finally {
        if (active) setBusy(false);
      }
    }
    void check();
    return () => {
      active = false;
      if (timer) clearTimeout(timer);
    };
    // `installing` is a mode for the run that `attempt` already restarts.
    // Listing it would re-enter when a failed install clears the flag and
    // wipe the error that run just showed.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- see above
  }, [client, harnessId, attempt]);
  async function install() {
    if (!client) return;
    setBusy(true);
    setError(null);
    try {
      await client.installHarness(harnessId, true);
      setInstalling(true);
      setAttempt((n) => n + 1);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setBusy(false);
    }
  }
  const ready =
    report?.ready &&
    !report.command?.endsWith("/npx") &&
    report.command !== "npx";
  return (
    <section className="setup-adapter" aria-label="Adapter readiness">
      <h2>Prepare your engine</h2>
      <p role="status">
        {installing
          ? "Installing adapter… You can skip while installation continues."
          : busy
            ? "Checking this engine and its adapter…"
            : ready
              ? "Your engine and adapter are ready."
              : report?.detail}
      </p>
      {error && <p role="alert">{error}</p>}
      {!ready && report?.remedy && <p>{report.remedy}</p>}
      {!ready && supported && (
        <p>
          {harnessId === "gemini"
            ? "Install Gemini CLI into ~/.local. Sign-in is separate — run gemini once, or export GEMINI_API_KEY."
            : "Install the ACP adapter into ~/.local. Your engine CLI and account sign-in are separate."}
        </p>
      )}
      <div className="setup-foot">
        {!ready && supported && (
          <button
            className="btn primary"
            disabled={busy || installing || !client}
            onClick={() => void install()}
          >
            Install adapter
          </button>
        )}
        <button
          className="btn"
          disabled={busy || installing}
          onClick={() => {
            setReport(null);
            setAttempt((n) => n + 1);
          }}
        >
          Retry check
        </button>
        <button className="btn" onClick={onContinue}>
          {ready ? "Continue" : "Skip for now"}
        </button>
      </div>
    </section>
  );
}
