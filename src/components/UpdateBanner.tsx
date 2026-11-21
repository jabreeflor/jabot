import { useEffect, useState } from "react";

import {
  bannerFor,
  restartToUpdate,
  subscribeUpdates,
  type UpdateStatus,
} from "../updates";
import { CrossIcon } from "./Icon";

/**
 * "JaBot 0.1.8 is ready. [Restart]" — bottom-right, over everything.
 *
 * The update is already downloaded and verified when this appears, so the
 * one action is the restart. Dismissing only hides the banner: quitting
 * JaBot still installs the update on the way out (updates.rs).
 */
export function UpdateBanner() {
  const [status, setStatus] = useState<UpdateStatus>({ state: "idle" });
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [restarting, setRestarting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => subscribeUpdates(setStatus), []);

  const banner = bannerFor(status);
  // Dismissal is per message: a later "ready" after a dismissed "you're on
  // the latest" still shows.
  const key = banner ? `${status.state}:${banner.text}` : null;
  if (!banner || key === dismissed) return null;

  const restart = () => {
    setRestarting(true);
    setError(null);
    restartToUpdate().catch((e: unknown) => {
      setRestarting(false);
      setError(e instanceof Error ? e.message : String(e));
    });
  };

  return (
    <div
      className={`update-banner ${banner.tone}`}
      role={banner.tone === "error" ? "alert" : "status"}
    >
      <span className="update-text">
        {banner.text}
        {banner.tone === "ready" && (
          <span className="update-sub">
            {error ?? "Restart to finish updating."}
          </span>
        )}
      </span>
      {banner.tone === "ready" && (
        <button
          type="button"
          className="btn primary"
          onClick={restart}
          disabled={restarting}
        >
          {restarting ? "Restarting…" : "Restart"}
        </button>
      )}
      <button
        type="button"
        className="update-dismiss"
        onClick={() => setDismissed(key)}
        aria-label="Dismiss"
      >
        <CrossIcon />
      </button>
    </div>
  );
}
