/**
 * The header host affordance (#7): one button, clickable so a second host
 * is a longer menu rather than new chrome.
 */
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { HostPicker } from "../components/HostPicker";

describe("HostPicker", () => {
  it("names a reachable host and reports the pick", async () => {
    const onPick = vi.fn();
    render(
      <HostPicker
        host={{ hostId: "h1", name: "This Mac", reachable: true }}
        onPick={onPick}
      />,
    );
    const button = screen.getByRole("button", { name: "Host: This Mac" });
    expect(button).toHaveAttribute("title", "This Mac");
    await userEvent.click(button);
    expect(onPick).toHaveBeenCalledWith("h1");
  });

  it("marks an unreachable host offline", () => {
    render(
      <HostPicker host={{ hostId: "h2", name: "Office", reachable: false }} />,
    );
    expect(
      screen.getByRole("button", { name: "Host: Office" }),
    ).toHaveAttribute("title", "Office — unreachable");
    expect(screen.getByText("offline")).toBeInTheDocument();
  });
});
