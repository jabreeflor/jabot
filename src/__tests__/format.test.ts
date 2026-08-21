/**
 * Timestamp and initials formatting. The clock branch defers to the user's
 * locale, so it is asserted by shape rather than by a literal string — pinning
 * "10:12" here would only test the test runner's default locale.
 */
import { describe, expect, it } from "vitest";

import { formatWhen, initials } from "../components/format";

const NOW = new Date("2026-08-20T14:30:00Z");

function isoMinutesBefore(minutes: number): string {
  return new Date(NOW.getTime() - minutes * 60_000).toISOString();
}

describe("formatWhen", () => {
  it("counts minutes inside the hour", () => {
    expect(formatWhen(isoMinutesBefore(0), NOW)).toBe("now");
    expect(formatWhen(isoMinutesBefore(12), NOW)).toBe("12m");
    expect(formatWhen(isoMinutesBefore(59), NOW)).toBe("59m");
  });

  it("switches to a clock time later the same day", () => {
    expect(formatWhen(isoMinutesBefore(4 * 60), NOW)).toMatch(
      /^\d{1,2}:\d{2}(\s?[AP]M)?$/,
    );
  });

  it("names yesterday, the weekday, then the date", () => {
    const days = (n: number) => isoMinutesBefore(n * 24 * 60);

    expect(formatWhen(days(1), NOW)).toBe("Yesterday");
    expect(formatWhen(days(3), NOW)).not.toBe("Yesterday");
    expect(formatWhen(days(3), NOW)).toMatch(/^[A-Za-z]{3,}$/);
    expect(formatWhen(days(30), NOW)).toMatch(/\d/);
  });

  it("returns nothing for a value that is not a timestamp", () => {
    expect(formatWhen("not a date", NOW)).toBe("");
  });
});

describe("initials", () => {
  it("takes the first and last name", () => {
    expect(initials("Jabree Flor")).toBe("JF");
    expect(initials("ada lovelace king")).toBe("AK");
  });

  it("survives a mononym and an empty name", () => {
    expect(initials("Prince")).toBe("P");
    expect(initials("   ")).toBe("?");
  });
});
