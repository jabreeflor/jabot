/**
 * Renderer smoke test. The app must render its shell and surface a host error
 * rather than a blank window when the host cannot be reached — the jsdom test
 * environment has no Tauri bridge, which is exactly that failure mode.
 */
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import App from "../App";

describe("App", () => {
  it("renders the shell while the host connection is pending", () => {
    render(<App />);

    expect(screen.getByText("JaBot")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1 })).toBeInTheDocument();
  });

  it("does not crash without a Tauri bridge", async () => {
    render(<App />);

    // Either a loading state or an error — never an empty document.
    expect(document.querySelector(".app-shell")).not.toBeNull();
  });
});
