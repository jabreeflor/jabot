import { useCallback, useEffect, useRef, useState } from "react";
import type { HostClient } from "../host";
import type { PrTarget, PrWorkspace } from "../host/prWorkspace";

export const PR_DETAIL_REFRESH_MS = 30_000;

export function prWorkspaceError(error: unknown): string {
  if (error && typeof error === "object" && "data" in error) {
    const data = error.data as { detail?: string } | undefined;
    if (data?.detail) return data.detail;
  }
  return error instanceof Error ? error.message : String(error);
}

/** Keep the visible PR live without replacing a commit the user is reviewing. */
export function usePrDetails(
  client: HostClient | null,
  { repo, number, host }: PrTarget,
  writing: { readonly current: boolean },
) {
  const [data, setData] = useState<PrWorkspace | null>(null);
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshError, setRefreshError] = useState<string | null>(null);
  const [pendingHead, setPendingHead] = useState<string | null>(null);
  const runner = useRef<(() => Promise<void>) | null>(null);
  const refresh = useCallback(async () => {
    await runner.current?.();
  }, []);

  useEffect(() => {
    setData(null);
    setPendingHead(null);
    setRefreshError(null);
    setLoading(false);
    setRefreshing(false);
    if (!client) return;
    let active = true;
    let current: PrWorkspace | null = null;
    let running: Promise<void> | null = null;

    async function read(background = false): Promise<void> {
      if (!active || (background && (document.hidden || writing.current)))
        return;
      // Focus, visibility and timer events share one request. An explicit reload
      // waits for that request, then reads again (also after a completed write).
      if (running) {
        if (background) return;
        await running;
        if (active) await read();
        return;
      }
      if (!background) setLoading(true);
      setRefreshing(true);
      running = (async () => {
        try {
          const next = await Promise.resolve().then(() =>
            client!.pullRequestDetail({ repo, number, host }),
          );
          if (!active || (background && writing.current)) return;
          setRefreshError(null);
          if (
            background &&
            current &&
            next.pr.head.sha !== current.pr.head.sha
          ) {
            // Never silently retarget an approval, merge confirmation, or line
            // comment to a different commit while the user is composing it.
            setPendingHead(next.pr.head.sha);
            return;
          }
          current = next;
          setData(next);
          setPendingHead(null);
        } catch (error) {
          if (active && !(background && writing.current)) {
            setRefreshError(prWorkspaceError(error));
          }
        } finally {
          running = null;
          if (active) {
            setLoading(false);
            setRefreshing(false);
          }
        }
      })();
      await running;
    }

    const tick = () => {
      void read(true);
    };
    runner.current = () => read();
    void read();
    const timer = window.setInterval(tick, PR_DETAIL_REFRESH_MS);
    window.addEventListener("focus", tick);
    document.addEventListener("visibilitychange", tick);
    return () => {
      active = false;
      runner.current = null;
      window.clearInterval(timer);
      window.removeEventListener("focus", tick);
      document.removeEventListener("visibilitychange", tick);
    };
  }, [client, repo, number, host, writing]);

  return { data, loading, refreshing, refreshError, pendingHead, refresh };
}
