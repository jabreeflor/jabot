//! Browser axe helper. Contrast is on — this is the half jsdom cannot prove.
//!
//! Critical and serious findings fail the run. Moderate and minor are attached
//! to the Playwright report so they stay visible. Exceptions are listed
//! explicitly in `AXE_EXCEPTIONS`; there is no blanket disable.

import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, type TestInfo } from "@playwright/test";
import type { AxeResults, Result } from "axe-core";

const BLOCKING = new Set(["critical", "serious"]);

/**
 * Known, named exceptions. Empty until a defect is accepted with a reason
 * and (when it exists) an issue number. Do not add "the whole page".
 */
export const AXE_EXCEPTIONS: readonly {
  id: string;
  reason: string;
  issue?: string;
}[] = [];

const EXCEPTED = new Set(AXE_EXCEPTIONS.map((entry) => entry.id));

export async function scanA11y(page: Page): Promise<AxeResults> {
  return new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
    .analyze();
}

export async function expectNoSeriousBrowserA11yViolations(
  page: Page,
  testInfo: TestInfo,
  label: string,
): Promise<AxeResults> {
  const results = await scanA11y(page);
  await testInfo.attach(`axe-${label}`, {
    body: JSON.stringify(
      {
        label,
        url: page.url(),
        violations: results.violations,
        exceptions: AXE_EXCEPTIONS,
      },
      null,
      2,
    ),
    contentType: "application/json",
  });

  const blockers = results.violations.filter((violation) => {
    if (EXCEPTED.has(violation.id)) return false;
    return violation.impact != null && BLOCKING.has(violation.impact);
  });
  expect(blockers, formatViolations(label, blockers)).toEqual([]);
  return results;
}

function formatViolations(label: string, violations: readonly Result[]): string {
  if (violations.length === 0) return `${label}: no blocking axe violations`;
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
  return `critical/serious axe violations (${label}):\n${body}`;
}
