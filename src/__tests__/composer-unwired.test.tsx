/**
 * Attach and Voice are not wired yet (#306). They must say so instead of being
 * a working-looking control that silently does nothing.
 */
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Composer } from "../components/Composer";

describe("Composer unwired controls", () => {
  it("marks Attach and Voice unavailable, with a reason", () => {
    render(<Composer placeholder="Message Chief" onSend={vi.fn()} />);
    const attach = screen.getByRole("button", {
      name: "Attach (not available yet)",
    });
    expect(attach).toHaveAttribute("aria-disabled", "true");
    expect(attach).toHaveAttribute("title", "Attachments aren't available yet");
    const voice = screen.getByRole("button", {
      name: "Voice (not available yet)",
    });
    expect(voice).toHaveAttribute("aria-disabled", "true");
  });
});
