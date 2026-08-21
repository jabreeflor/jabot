/**
 * The first-run record, pure: the round trip, the validation rules, and above
 * all the failure *directions* — an unreadable store must land in the shell
 * (never a setup loop), a corrupt record must land in setup (never a
 * half-configured shell), and a blank name must be unproducible at both the
 * writer and the reader.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_USER_NAME,
  ONBOARDING_KEY,
  loadOnboarding,
  makeProfile,
  resetOnboarding,
  saveOnboarding,
} from "../onboarding/state";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("makeProfile", () => {
  it("trims the name and cannot produce a blank record", () => {
    expect(makeProfile({ userName: " Ada ", harnessId: null, skipped: false }).userName).toBe("Ada");
    expect(makeProfile({ userName: "   ", harnessId: null, skipped: true }).userName).toBe(DEFAULT_USER_NAME);
    expect(makeProfile({ userName: "", harnessId: null, skipped: true }).userName).toBe(DEFAULT_USER_NAME);
  });

  it("coerces an empty harnessId to null", () => {
    expect(makeProfile({ userName: "Ada", harnessId: "", skipped: false }).harnessId).toBeNull();
    expect(makeProfile({ userName: "Ada", harnessId: "codex", skipped: false }).harnessId).toBe("codex");
  });
});

describe("loadOnboarding", () => {
  it("round-trips through saveOnboarding", () => {
    saveOnboarding(makeProfile({ userName: "Ada Lovelace", harnessId: "codex", skipped: false }));
    const loaded = loadOnboarding();
    expect(loaded).not.toBeNull();
    expect(loaded?.userName).toBe("Ada Lovelace");
    expect(loaded?.harnessId).toBe("codex");
    expect(loaded?.skipped).toBe(false);
  });

  it("treats a readable-but-empty store as a first run", () => {
    // setup-dom seeds a completed profile; empty the store to get first-run.
    window.localStorage.clear();
    expect(loadOnboarding()).toBeNull();
  });

  it("treats a corrupt or shapeless record as a first run", () => {
    for (const raw of ["{not json", '["array"]', '"a string"', '{"userName":"x"}', '{"version":"one"}', '{"version":0}']) {
      window.localStorage.setItem(ONBOARDING_KEY, raw);
      expect(loadOnboarding()).toBeNull();
    }
  });

  it("honours a newer version instead of replaying setup", () => {
    window.localStorage.setItem(
      ONBOARDING_KEY,
      '{"version":2,"userName":"Grace Hopper","theme":"light"}',
    );
    const loaded = loadOnboarding();
    expect(loaded?.version).toBe(2);
    expect(loaded?.userName).toBe("Grace Hopper");
  });

  it("resolves a hand-edited blank name to the default rather than looping", () => {
    window.localStorage.setItem(
      ONBOARDING_KEY,
      '{"version":1,"userName":"   ","harnessId":null,"skipped":false,"completedAt":""}',
    );
    const loaded = loadOnboarding();
    expect(loaded).not.toBeNull();
    expect(loaded?.userName).toBe(DEFAULT_USER_NAME);
  });

  it("treats an unreadable store as already onboarded, with no write", () => {
    const setItem = vi.spyOn(Storage.prototype, "setItem");
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    const loaded = loadOnboarding();
    expect(loaded).not.toBeNull();
    expect(loaded?.userName).toBe(DEFAULT_USER_NAME);
    expect(setItem).not.toHaveBeenCalled();
  });
});

describe("saveOnboarding / resetOnboarding", () => {
  it("does not propagate a storage failure", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota");
    });
    expect(() =>
      saveOnboarding(makeProfile({ userName: "Ada", harnessId: null, skipped: false })),
    ).not.toThrow();
  });

  it("resetOnboarding returns the store to a first run", () => {
    saveOnboarding(makeProfile({ userName: "Ada", harnessId: null, skipped: false }));
    resetOnboarding();
    expect(loadOnboarding()).toBeNull();
  });
});
