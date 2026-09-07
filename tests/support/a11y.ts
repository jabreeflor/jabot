//! Axe helper for the unit suite.
//!
//! jsdom cannot paint, so this gate is the name, role, and structure half of
//! WCAG — the half a React unit test can actually prove. Color-contrast is
//! disabled on purpose. Critical and serious findings still fail the run;
//! moderate and minor stay visible in the axe report but do not break CI.

import axe, { type Result } from "axe-core";

const BLOCKING = new Set(["critical", "serious"]);

export async function expectNoSeriousA11yViolations(
  container: HTMLElement,
): Promise<void> {
  const results = await axe.run(container, {
    runOnly: {
      type: "tag",
      values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"],
    },
    rules: {
      // jsdom has no layout or paint; contrast checks here are noise, not proof.
      "color-contrast": { enabled: false },
    },
  });
  const blockers = results.violations.filter(
    (violation) => violation.impact != null && BLOCKING.has(violation.impact),
  );
  if (blockers.length > 0) {
    throw new Error(formatViolations(blockers));
  }
}

function formatViolations(violations: readonly Result[]): string {
  const body = violations
    .map((violation) => {
      const nodes = violation.nodes
        .map(
          (node) =>
            `    ${node.target.join(" > ")}\n      ${node.failureSummary ?? ""}`,
        )
        .join("\n");
      return `[${violation.impact}] ${violation.id}: ${violation.help}\n${nodes}`;
    })
    .join("\n\n");
  return `critical/serious axe violations:\n${body}`;
}
