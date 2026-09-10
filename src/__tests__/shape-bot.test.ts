import { describe, expect, it } from "vitest";

import type { Bot } from "../components/types";
import {
  UNFORMED_BOT_NAME,
  blankBotDraft,
  filterMentionBots,
  insertMention,
  isUnformedBot,
  mentionQuery,
  mentionedBots,
  nextBotColor,
  shapeFromMessage,
  splitMentions,
} from "../views/shape-bot";

const CHIEF: Bot = {
  id: "chief",
  name: "Chief",
  color: "b-teal",
  instructions: "Route work.",
  tools: [],
  harnessId: "claude",
  isChief: true,
};

const RECRUITER: Bot = {
  id: "bot-recruiter",
  name: "Bot Recruiter",
  color: "b-purple",
  instructions: "Shape new bots.",
  tools: [],
  harnessId: "claude",
  isChief: false,
};

describe("isUnformedBot", () => {
  it("treats empty instructions as unformed and a persona as formed", () => {
    expect(isUnformedBot({ instructions: "" })).toBe(true);
    expect(isUnformedBot({ instructions: "   " })).toBe(true);
    expect(isUnformedBot({ instructions: "Route work." })).toBe(false);
  });
});

describe("nextBotColor", () => {
  it("picks the first unused persisted colour id", () => {
    expect(nextBotColor([{ color: "b-teal" }])).toBe("b-yellow");
    expect(nextBotColor([{ color: "b-teal" }, { color: "b-yellow" }])).toBe(
      "b-purple",
    );
  });
});

describe("blankBotDraft", () => {
  it("starts a named blank bot on the first harness", () => {
    expect(blankBotDraft([CHIEF], [{ id: "claude" }])).toEqual(
      expect.objectContaining({
        name: UNFORMED_BOT_NAME,
        instructions: "",
        tools: [],
        harnessId: "claude",
        color: "b-yellow",
      }),
    );
  });
});

describe("shapeFromMessage", () => {
  const current = { name: UNFORMED_BOT_NAME };

  it("reads an explicit You're / You are name", () => {
    expect(
      shapeFromMessage(
        "You're Scout, a research assistant who checks sources.",
        current,
      ),
    ).toEqual({
      name: "Scout",
      instructions: "You're Scout, a research assistant who checks sources.",
    });
  });

  it("reads a Name: field", () => {
    expect(
      shapeFromMessage("Name: Inbox Mgr\nFlag what needs me.", current),
    ).toEqual({
      name: "Inbox Mgr",
      instructions: "Name: Inbox Mgr\nFlag what needs me.",
    });
  });

  it("uses a short title-case first line when there is no formula", () => {
    expect(
      shapeFromMessage("Research inbox. Flag anything that needs me.", current),
    ).toEqual({
      name: "Research inbox",
      instructions: "Research inbox. Flag anything that needs me.",
    });
  });

  it("keeps the current name when the message is just the job", () => {
    expect(
      shapeFromMessage("please help me triage email every morning", current),
    ).toEqual({
      name: UNFORMED_BOT_NAME,
      instructions: "please help me triage email every morning",
    });
  });
});

describe("mentions", () => {
  const bots = [CHIEF, RECRUITER];

  it("splits @Name including multi-word crew names", () => {
    const parts = splitMentions("Ask @Bot Recruiter after @Chief.", bots);
    expect(parts).toEqual([
      { type: "text", value: "Ask " },
      { type: "mention", bot: RECRUITER },
      { type: "text", value: " after " },
      { type: "mention", bot: CHIEF },
      { type: "text", value: "." },
    ]);
  });

  it("dedupes mentioned bots", () => {
    expect(mentionedBots("Hey @Chief — and @Chief again", bots)).toEqual([
      CHIEF,
    ]);
  });

  it("reads a trailing composer query and inserts a mention", () => {
    expect(mentionQuery("ask @Bo")).toBe("Bo");
    expect(mentionQuery("ask @")).toBe("");
    expect(mentionQuery("ask them")).toBeNull();
    expect(insertMention("ask @Bo", "Bot Recruiter")).toBe(
      "ask @Bot Recruiter ",
    );
    expect(filterMentionBots(bots, "bot").map((bot) => bot.id)).toEqual([
      "bot-recruiter",
    ]);
  });
});
