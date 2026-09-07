//! Deterministic host seeds for visual / a11y states. RPC is for
//! prerequisites only — the click or send under test still goes through the UI.

import {
  CREW_LIST,
  CREW_THREAD,
  CREW_UPDATE,
  PERMISSION_PENDING,
  SCHEDULE_CREATE,
  SESSION_PROMPT,
  THREAD_FOLD,
  THREAD_TRANSCRIPT,
  type BotView,
  type CrewListResult,
  type PermissionPendingResult,
  type ScheduleView,
  type ThreadTranscriptResult,
} from "../../../src/host/protocol";
import {
  CHIEF_BOT_ID,
  CHIEF_THREAD_ID,
  FAKE_ACP_ASK_ID,
  FAKE_ACP_REPLY,
  RECRUITER_BOT_ID,
  RECRUITER_THREAD_ID,
} from "./constants";
import { rpc } from "./rpc";

export async function putChiefOnFakeAcp(baseUrl: string): Promise<void> {
  await rpc(baseUrl, CREW_UPDATE, {
    botId: CHIEF_BOT_ID,
    harnessId: "fake-acp",
  });
}

export async function seedChiefConversation(baseUrl: string): Promise<void> {
  await putChiefOnFakeAcp(baseUrl);
  await rpc(baseUrl, CREW_THREAD, { botId: CHIEF_BOT_ID });
  await rpc(baseUrl, SESSION_PROMPT, {
    threadId: CHIEF_THREAD_ID,
    content: "hello from the visual suite",
  });
  await until(async () => {
    const transcript = await rpc<ThreadTranscriptResult>(
      baseUrl,
      THREAD_TRANSCRIPT,
      { threadId: CHIEF_THREAD_ID },
    );
    return JSON.stringify(transcript).includes(FAKE_ACP_REPLY);
  }, "fake-acp never replied on Chief's standing thread");
}

export async function seedPermissionAsk(baseUrl: string): Promise<void> {
  const crew = await rpc<CrewListResult>(baseUrl, CREW_LIST);
  const recruiter = named(crew.bots, "Bot Recruiter");
  await rpc(baseUrl, CREW_UPDATE, {
    botId: recruiter.botId ?? RECRUITER_BOT_ID,
    harnessId: FAKE_ACP_ASK_ID,
  });
  await rpc(baseUrl, CREW_THREAD, {
    botId: recruiter.botId ?? RECRUITER_BOT_ID,
  });
  await rpc(baseUrl, SESSION_PROMPT, {
    threadId: RECRUITER_THREAD_ID,
    content: "rm -rf",
  });
  await until(
    async () => {
      const pending = await rpc<PermissionPendingResult>(
        baseUrl,
        PERMISSION_PENDING,
      );
      return pending.requests.length > 0;
    },
    "permission ask never arrived",
  );
  await rpc(baseUrl, THREAD_FOLD, { threadId: RECRUITER_THREAD_ID });
}

export async function seedSchedule(baseUrl: string): Promise<ScheduleView> {
  await putChiefOnFakeAcp(baseUrl);
  return rpc<ScheduleView>(baseUrl, SCHEDULE_CREATE, {
    botId: CHIEF_BOT_ID,
    name: "Morning triage",
    cron: "0 9 * * 1-5",
    prompt: "Summarise overnight mail.",
  });
}

function named(bots: readonly BotView[], name: string): BotView {
  const bot = bots.find((candidate) => candidate.name === name);
  if (!bot) throw new Error(`no bot named ${name}`);
  return bot;
}

async function until(
  predicate: () => Promise<boolean>,
  message: string,
  timeoutMs = 20_000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(message);
}

