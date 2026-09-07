import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { HostClient } from "../host";
import type { PrTarget, PrWorkspace } from "../host/prWorkspace";

export const PR_DETAIL_REFRESH_MS = 30_000;
export const PR_DETAIL_CACHE_MAX = 5;

/**
 * Detail reads are expensive GitHub requests, while navigating back to the PR
 * board destroys the workspace component. Keep the last good answer per host
 * client so reopening a PR can paint immediately and revalidate in the
 * background. The WeakMap gives the cache the same lifetime as the connection.
 */
const detailCache = new WeakMap<HostClient, Map<string, PrWorkspace>>();

function cacheKey({ repo, number, host }: PrTarget): string {
  return `${host}:${repo}#${number}`;
}

function cachedDetail(
  client: HostClient | null,
  target: PrTarget,
): PrWorkspace | null {
  if (!client) return null;
  const clientCache = detailCache.get(client);
  const key = cacheKey(target);
  const detail = clientCache?.get(key) ?? null;
  if (detail && clientCache) {
    clientCache.delete(key);
    clientCache.set(key, detail);
  }
  return detail;
}

function rememberDetail(
  client: HostClient,
  target: PrTarget,
  detail: PrWorkspace,
): void {
  let clientCache = detailCache.get(client);
  if (!clientCache) {
    clientCache = new Map();
    detailCache.set(client, clientCache);
  }
  const key = cacheKey(target);
  clientCache.delete(key);
  clientCache.set(key, detail);
  while (clientCache.size > PR_DETAIL_CACHE_MAX) {
    const oldest = clientCache.keys().next().value;
    if (oldest === undefined) break;
    clientCache.delete(oldest);
  }
}

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
  const target = useMemo(() => ({ repo, number, host }), [repo, number, host]);
  const [data, setData] = useState<PrWorkspace | null>(() =>
    cachedDetail(client, target),
  );
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshError, setRefreshError] = useState<string | null>(null);
  const [pendingHead, setPendingHead] = useState<string | null>(null);
  const runner = useRef<(() => Promise<void>) | null>(null);
  const refresh = useCallback(async () => {
    await runner.current?.();
  }, []);

  useEffect(() => {
    const cached = cachedDetail(client, target);
    setData(cached);
    setPendingHead(null);
    setRefreshError(null);
    setLoading(false);
    setRefreshing(false);
    if (!client) return;
    let active = true;
    let current = cached;
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
      // Automatic revalidation is intentionally invisible: current data stays
      // interactive and the explicit Refresh control does not flash or disable
      // itself for timer, focus, or cache-warming reads.
      if (!background) {
        setLoading(true);
        setRefreshing(true);
      }
      running = (async () => {
        try {
          const next = await Promise.resolve().then(() =>
            client!.pullRequestDetail(target),
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
          rememberDetail(client!, target, next);
          setData(next);
          setPendingHead(null);
        } catch (error) {
          if (active && !(background && writing.current)) {
            setRefreshError(prWorkspaceError(error));
          }
        } finally {
          running = null;
          if (active) {
            if (!background) {
              setLoading(false);
              setRefreshing(false);
            }
          }
        }
      })();
      await running;
    }

    const tick = () => {
      void read(true);
    };
    runner.current = () => read();
    void read(cached !== null);
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
  }, [client, target, writing]);

  return { data, loading, refreshing, refreshError, pendingHead, refresh };
}
