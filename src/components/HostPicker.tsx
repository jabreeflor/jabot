//! The monitor icon in the chat header. MVP1 runs everything on this Mac, but
//! the header keeps the affordance so a second host is a longer menu rather
//! than a new piece of chrome (#7 decision: the renderer only ever talks to a
//! host API).

import { MonitorIcon } from "./Icon";
import type { HostTarget } from "./types";

export function HostPicker({
  host,
  onPick,
}: {
  host: HostTarget;
  onPick?: (hostId: string) => void;
}) {
  return (
    <button
      type="button"
      className="host-picker"
      aria-label={`Host: ${host.name}`}
      title={host.reachable ? host.name : `${host.name} — unreachable`}
      onClick={() => onPick?.(host.hostId)}
    >
      <MonitorIcon />
      {!host.reachable && <span className="host-error">offline</span>}
    </button>
  );
}
