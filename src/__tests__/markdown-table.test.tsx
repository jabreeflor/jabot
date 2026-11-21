/**
 * GFM pipe tables in an agent bubble (#305). Bold and `code` already rendered;
 * a table arrived as literal pipes and dashes.
 */
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { renderMarkdown } from "../components/markdown";

function draw(text: string) {
  return render(<div>{renderMarkdown(text)}</div>);
}

describe("renderMarkdown tables", () => {
  it("renders a pipe table as a table, with inline markup in its cells", () => {
    draw(
      [
        "Give me **bold** and a table:",
        "",
        "| a | b |",
        "| --- | ---: |",
        "| 1 | `2` |",
        "| 3 | **4** |",
      ].join("\n"),
    );
    const table = screen.getByRole("table");
    const headers = within(table).getAllByRole("columnheader");
    expect(headers.map((h) => h.textContent)).toEqual(["a", "b"]);
    const cells = within(table).getAllByRole("cell");
    expect(cells.map((c) => c.textContent)).toEqual(["1", "2", "3", "4"]);
    expect(cells[1].querySelector("code")).not.toBeNull();
    expect(cells[3].querySelector("strong")).not.toBeNull();
    expect(cells[1]).toHaveStyle({ textAlign: "right" });
    expect(screen.queryByText(/\| --- \|/)).toBeNull();
  });

  it("accepts tables without the outer pipes and pads short rows", () => {
    draw(["x | y", "--|--", "only |"].join("\n"));
    const cells = within(screen.getByRole("table")).getAllByRole("cell");
    expect(cells.map((c) => c.textContent)).toEqual(["only", ""]);
  });

  it("leaves a stray pipe in prose alone", () => {
    draw("use a | b to pipe\nthen carry on");
    expect(screen.queryByRole("table")).toBeNull();
    expect(screen.getByText(/use a \| b to pipe/)).toBeInTheDocument();
  });

  it("does not take a header with a different cell count as a table", () => {
    draw(["a | b | c", "| --- | --- |"].join("\n"));
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("keeps an escaped pipe inside a cell", () => {
    draw(["| expr |", "| --- |", "| a \\| b |"].join("\n"));
    expect(screen.getByRole("cell").textContent).toBe("a | b");
  });
});
