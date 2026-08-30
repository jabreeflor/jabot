/**
 * The crew, from the wire to the grid (#17).
 *
 * `tests/e2e/crew.test.ts` drives the real host; what is checked here is the
 * renderer's half of the contract — that a `crew/list` result becomes crew
 * cards without being reshaped, that the editor really is the record (a save
 * is a host call and the grid redraws from what came back), and that a refused
 * remove leaves the bot on screen with the host's reason next to it.
 *
 * The fake host keeps a crew and mutates it, because the thing under test is
 * "does the UI show what the host now holds". A `updateBot` that returned a
 * changed row while `listCrew` kept answering the old one would let a broken
 * reload pass.
 */
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "../App";
import {
  connectHost,
  HostRpcError,
  RPC_ERROR,
  type BotView,
  type CrewCreateParams,
  type CrewListResult,
  type CrewRefParams,
  type CrewRemoveResult,
  type CrewUpdateParams,
  type HarnessDoctorResult,
  type HarnessListResult,
  type HelloResult,
  type HostClient,
  type ToolListResult,
} from "../host";
import { botRow, templateRow, withReadiness } from "../views/crew";

vi.mock("../host", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../host")>();
  return { ...actual, connectHost: vi.fn() };
});

const HELLO: HelloResult = {
  protocolVersion: 1,
  hostId: "host-1",
  hostName: "This Mac",
  hostMode: "in-process",
  version: "0.1.0",
  platform: "macos",
  device: { deviceId: "dev-1", name: "This Mac", role: "full" },
  methods: [],
  notifications: [],
};

function bot(overrides: Partial<BotView> = {}): BotView {
  return {
    botId: "writer",
    name: "Writer",
    color: "b-orange",
    instructions: "Draft in my voice: plain, short, no filler.",
    tools: ["gmail"],
    harnessId: "claude",
    isChief: false,
    memoryDir: "/data/bots/writer",
    sortOrder: 5,
    unread: 0,
    createdAt: "2026-08-20T10:00:00Z",
    updatedAt: "2026-08-20T10:00:00Z",
    ...overrides,
  };
}

const CHIEF = bot({
  botId: "chief",
  name: "Chief",
  color: "b-teal",
  instructions: "Route work across the crew.",
  tools: ["handoff_to_bot"],
  isChief: true,
  memoryDir: "/data/bots/chief",
  sortOrder: 0,
});

const TOOLS: ToolListResult = {
  tools: [
    {
      id: "gmail",
      label: "Gmail",
      blurb: "Search threads, read mail, park drafts",
      transport: "http",
      mcp: true,
      provider: "google",
      providerLabel: "Google",
      scopes: [],
      status: "connected",
      detail: "Connected as jabree@example.com",
      docsUrl: "https://example.com/gmail",
    },
    {
      id: "notion",
      label: "Notion",
      blurb: "Read and write pages",
      transport: "http",
      mcp: true,
      scopes: [],
      status: "needs_auth",
      docsUrl: "https://example.com/notion",
    },
  ],
};

const HARNESSES: HarnessListResult = {
  harnesses: [
    {
      id: "claude",
      label: "Claude Code",
      blurb: "Anthropic's coding agent, wrapped in JaBot's UI",
      accent: "var(--h-claude)",
      tier: "shipped",
      command: "claude-agent-acp",
      args: [],
      sessionScope: "thread",
      reserved: true,
    },
    {
      id: "hermes",
      label: "Hermes",
      blurb: "A preset the mock has never heard of",
      accent: "var(--h-hermes)",
      tier: "preset",
      command: "hermes",
      args: ["acp"],
      sessionScope: "profile",
      reserved: true,
    },
  ],
  issues: [],
};

/** What the Doctor says about the two cards above: one engine on disk, one
    not. `remedy` is deliberately different from any `installHint` in the
    catalog, so a test can tell which of the two the card ended up showing. */
const DOCTOR: HarnessDoctorResult = {
  reports: [
    {
      id: "claude",
      label: "Claude Code",
      tier: "shipped",
      status: "ready",
      ready: true,
      detail: "claude-agent-acp 1.2.0",
      command: "/usr/local/bin/claude-agent-acp",
      args: [],
      elapsedMs: 12,
    },
    {
      id: "hermes",
      label: "Hermes",
      tier: "preset",
      status: "cli_missing",
      ready: false,
      detail: "hermes is not on PATH",
      remedy: "Run: npm i -g hermes-acp",
      args: ["acp"],
      elapsedMs: 8,
    },
  ],
  issues: [],
  path: ["/usr/local/bin", "/usr/bin"],
};

const TEMPLATES: CrewListResult["templates"] = [
  {
    templateId: "expense",
    name: "Expense Manager",
    color: "b-green",
    instructions: "Chase receipts.",
    tools: ["gmail"],
    harnessId: "claude",
  },
];

const HOST_TOOLS: CrewListResult["hostTools"] = [
  { id: "handoff_to_bot", label: "Handoff", blurb: "Pass a job to another bot" },
];

describe("botRow / templateRow", () => {
  it("renames the wire shape onto the prop shape and nothing else", () => {
    expect(botRow(bot({ templateId: "expense" }))).toEqual({
      id: "writer",
      name: "Writer",
      color: "b-orange",
      instructions: "Draft in my voice: plain, short, no filler.",
      tools: ["gmail"],
      harnessId: "claude",
      isChief: false,
      templateId: "expense",
      image: null,
      unread: false,
    });
    // Absent template = a bot nobody copied, not an unset field the editor
    // would then send back as the string "undefined". An absent icon is the
    // same shape for the same reason: `null` is "wears its colour and
    // initials", and the editor sends that back as a deliberate clear.
    expect(botRow(bot()).templateId).toBeNull();
    expect(botRow(bot()).image).toBeNull();
    expect(botRow(bot({ image: "data:image/png;base64,AAAA" })).image).toBe(
      "data:image/png;base64,AAAA",
    );
    expect(templateRow(TEMPLATES[0])).toEqual(TEMPLATES[0]);
  });

  /**
   * The dot on a bot's blob (#22, #24).
   *
   * `Bot.unread` has been a prop since the prototype and `Avatar` has drawn
   * the dot for it just as long, but nothing ever set it outside the
   * fixtures: `BotView` had no such field, so in the shipped app the dot could
   * not appear however much was waiting.
   *
   * A dot rather than the count itself: the blob has room to say "something is
   * waiting" and the number is read in the Inbox, which is where it can be
   * acted on.
   */
  it("turns the host's per-bot count into the dot", () => {
    expect(botRow(bot({ unread: 3 })).unread).toBe(true);
    expect(botRow(bot({ unread: 1 })).unread).toBe(true);
    expect(botRow(bot({ unread: 0 })).unread).toBe(false);
  });

  /** An older host does not send the field. No dot is the honest reading, and
      the one the app has been living with. */
  it("draws no dot for a host that does not count", () => {
    const older = bot();
    delete (older as Partial<BotView>).unread;
    expect(botRow(older).unread).toBe(false);
  });

  it("renders a colour it does not know rather than crashing on it", () => {
    // The host keeps `bots.color` inside a closed list, so this can only come
    // from a row something else wrote — a card is a better answer than a blank
    // page.
    expect(botRow(bot({ color: "chartreuse" })).color).toBe("b-green");
  });
});

describe("withReadiness", () => {
  const cards = [
    { id: "claude", label: "Claude Code", blurb: "b1", accent: "a1" },
    { id: "hermes", label: "Hermes", blurb: "b2", accent: "a2", installHint: "from the catalog" },
    { id: "pi", label: "Pi", blurb: "b3", accent: "a3" },
  ];

  it("marks each card with what the Doctor found", () => {
    const [claude, hermes] = withReadiness(cards, DOCTOR.reports);
    expect(claude.available).toBe(true);
    expect(hermes.available).toBe(false);
  });

  it("leaves a card nobody probed alone, rather than calling it missing", () => {
    // "not asked" is not "missing". Defaulting an unprobed card to false would
    // grey out an engine that is very likely installed.
    const pi = withReadiness(cards, DOCTOR.reports)[2];
    expect(pi.available).toBeUndefined();
    expect(pi).toEqual(cards[2]);
  });

  it("prefers the Doctor's remedy over the catalog's install hint", () => {
    // `remedy` is written against what was found on this machine; installHint
    // is a constant. The specific one wins.
    const hermes = withReadiness(cards, DOCTOR.reports)[1];
    expect(hermes.installHint).toBe("Run: npm i -g hermes-acp");
  });

  it("falls back to the catalog hint when the report carries no remedy", () => {
    const reports = [{ ...DOCTOR.reports[1], remedy: undefined }];
    expect(withReadiness(cards, reports)[1].installHint).toBe("from the catalog");
  });

  it("is a no-op when the Doctor reported nothing", () => {
    expect(withReadiness(cards, [])).toEqual(cards);
  });
});

describe("App, once the host has answered with a crew", () => {
  /** A crew the fake host actually holds, so a read after a write sees it. */
  let crew: BotView[] = [];

  const listCrew = vi.fn(
    async (): Promise<CrewListResult> => ({
      bots: crew.map((row) => ({ ...row })),
      templates: TEMPLATES,
      hostTools: HOST_TOOLS,
    }),
  );

  const createBot = vi.fn(async (params: CrewCreateParams): Promise<BotView> => {
    const template = TEMPLATES.find((t) => t.templateId === params.templateId);
    const created = bot({
      botId: `bot-${crew.length}`,
      name: params.name ?? template?.name ?? "Unnamed bot",
      color: params.color ?? template?.color ?? "b-green",
      instructions: params.instructions ?? template?.instructions ?? "",
      tools: params.tools ?? template?.tools ?? [],
      harnessId: params.harnessId ?? template?.harnessId ?? "claude",
      templateId: params.templateId,
      isChief: false,
    });
    crew = [...crew, created];
    return created;
  });

  const updateBot = vi.fn(async (params: CrewUpdateParams): Promise<BotView> => {
    const found = crew.find((row) => row.botId === params.botId);
    if (!found) throw new Error(`no such bot: ${params.botId}`);
    const { botId: _botId, ...patch } = params;
    const saved: BotView = { ...found, ...patch };
    crew = crew.map((row) => (row.botId === saved.botId ? saved : row));
    return saved;
  });

  const removeBot = vi.fn(
    async ({ botId }: CrewRefParams): Promise<CrewRemoveResult> => {
      const found = crew.find((row) => row.botId === botId);
      // The host refuses Chief, and so must anything standing in for it.
      if (found?.isChief) {
        throw new HostRpcError({
          code: RPC_ERROR.CHIEF_REQUIRED,
          message: "Chief cannot be removed",
          data: { botId },
        });
      }
      crew = crew.filter((row) => row.botId !== botId);
      return { botId, removed: true, detachedThreads: 0 };
    },
  );

  /** Reassigned per-test so one case can make the probe fail and another can
      hold it open. Default: the catalog above, probed. */
  let harnessDoctor = vi.fn(async () => DOCTOR);

  function client(): HostClient {
    return {
      disconnect: vi.fn(),
      listCrew,
      createBot,
      updateBot,
      removeBot,
      listTools: vi.fn(async () => TOOLS),
      listHarnesses: vi.fn(async () => HARNESSES),
      harnessDoctor,
      listFolders: vi.fn(async () => ({ folders: [] })),
    } as unknown as HostClient;
  }

  beforeEach(() => {
    crew = [CHIEF, bot()];
    vi.clearAllMocks();
    harnessDoctor = vi.fn(async () => DOCTOR);
    vi.mocked(connectHost).mockResolvedValue({ client: client(), hello: HELLO });
  });

  async function openCrew() {
    render(<App />);
    await screen.findByText("This Mac · v0.1.0");
    await userEvent.click(screen.getByRole("button", { name: /Crew/ }));
    return screen.findByRole("heading", { level: 1, name: "Your Crew" });
  }

  /** The crew grid, not the sidebar strip: a bot's name is on screen twice,
      once as a card and once as a face in the rail. */
  function grid(): HTMLElement {
    const element = document.querySelector(".crew-grid");
    if (!element) throw new Error("the crew grid is not rendered");
    return element as HTMLElement;
  }

  function card(name: string): HTMLElement {
    const element = within(grid()).getByText(name).closest(".crew-card");
    if (!element) throw new Error(`no crew card for ${name}`);
    return element as HTMLElement;
  }

  it("draws the host's crew instead of the fixtures", async () => {
    await openCrew();

    await waitFor(() => expect(card("Writer")).toBeInTheDocument());
    // The five fixture workers are gone the moment the host has an answer.
    expect(within(grid()).queryByText("Inbox Mgr")).not.toBeInTheDocument();
    expect(within(card("Chief")).getByText("CHIEF")).toBeInTheDocument();
    // Chief's host tools are named, not printed as ids.
    expect(within(card("Chief")).getByText("Handoff")).toBeInTheDocument();
    expect(
      within(card("Chief")).queryByText("handoff_to_bot"),
    ).not.toBeInTheDocument();
  });

  /**
   * The dot, end to end from the host's count (#22, #24).
   *
   * `botRow` turning a number into a boolean is only half of it: nothing drew
   * a dot before because `crew/list` had no count to turn, so the case worth
   * pinning is the whole trip — the host says 2, the grid lights that blob and
   * only that blob.
   */
  it("lights the blob of a bot with cards waiting, and no other", async () => {
    crew = [CHIEF, bot({ unread: 2 })];
    await openCrew();

    await waitFor(() => expect(card("Writer")).toBeInTheDocument());
    expect(
      within(card("Writer")).getByTestId("unread-dot"),
    ).toBeInTheDocument();
    expect(within(card("Chief")).queryByTestId("unread-dot")).toBeNull();
  });

  it("offers the host's harnesses in the editor, not the compiled-in three", async () => {
    await openCrew();
    await waitFor(() => expect(card("Writer")).toBeInTheDocument());

    await userEvent.click(
      within(card("Writer")).getByRole("button", { name: "Edit" }),
    );

    // A preset the mock host list never had: proof the picker is live (#13).
    expect(screen.getByRole("button", { name: /Hermes/ })).toBeInTheDocument();
    // And the chip's tooltip is the host's own sentence about the grant (#18).
    expect(screen.getByRole("button", { name: "Gmail" })).toHaveAttribute(
      "title",
      "Gmail — Connected as jabree@example.com",
    );
  });

  it("greys out an engine the Doctor could not find, and says how to install it", async () => {
    await openCrew();
    await waitFor(() => expect(card("Writer")).toBeInTheDocument());
    await userEvent.click(
      within(card("Writer")).getByRole("button", { name: "Edit" }),
    );

    // Hermes is not on this machine, so the card stops advertising itself and
    // says what to do about it instead. The text is the Doctor's `remedy` —
    // the one written knowing what was actually found — not the catalog's
    // constant `installHint`.
    // Re-queried inside the wait: the probe resolves after the picker has
    // already painted, so React replaces this node and a reference taken
    // before then goes stale.
    await waitFor(() =>
      expect(
        within(screen.getByRole("button", { name: /Hermes/ })).getByText(
          "Run: npm i -g hermes-acp",
        ),
      ).toBeInTheDocument(),
    );
    expect(
      within(screen.getByRole("button", { name: /Hermes/ })).queryByText(
        "A preset the mock has never heard of",
      ),
    ).not.toBeInTheDocument();

    // Claude is installed, so nothing about its card changes.
    const claude = screen.getByRole("button", { name: /Claude Code/ });
    expect(
      within(claude).getByText("Anthropic's coding agent, wrapped in JaBot's UI"),
    ).toBeInTheDocument();
  });

  it("keeps the picker usable when the Doctor probe fails", async () => {
    // A probe shells out to vendor CLIs, so it is the call here most likely to
    // hang or blow up. When it does, the catalog is still worth drawing: not
    // knowing whether an engine is installed is a smaller loss than replacing
    // the crew view with an error.
    harnessDoctor = vi.fn(async () => {
      throw new Error("probe timed out");
    });
    vi.mocked(connectHost).mockResolvedValue({ client: client(), hello: HELLO });

    await openCrew();
    await waitFor(() => expect(card("Writer")).toBeInTheDocument());
    await userEvent.click(
      within(card("Writer")).getByRole("button", { name: "Edit" }),
    );

    // Every card still there, still showing its blurb — nothing greyed on the
    // strength of an answer nobody got.
    const hermes = screen.getByRole("button", { name: /Hermes/ });
    expect(
      within(hermes).getByText("A preset the mock has never heard of"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Claude Code/ })).toBeInTheDocument();
    expect(screen.queryByText("probe timed out")).not.toBeInTheDocument();
  });

  it("paints the catalog before the Doctor has answered", async () => {
    // The whole reason `harness/list` and `harness/doctor` are two calls: the
    // cheap one must not wait on the expensive one. This holds the probe open
    // forever and asserts the picker opened anyway.
    harnessDoctor = vi.fn(() => new Promise<HarnessDoctorResult>(() => {}));
    vi.mocked(connectHost).mockResolvedValue({ client: client(), hello: HELLO });

    await openCrew();
    await waitFor(() => expect(card("Writer")).toBeInTheDocument());
    await userEvent.click(
      within(card("Writer")).getByRole("button", { name: "Edit" }),
    );

    const hermes = screen.getByRole("button", { name: /Hermes/ });
    expect(
      within(hermes).getByText("A preset the mock has never heard of"),
    ).toBeInTheDocument();
  });

  it("saves the editor through the host and redraws from what it stored", async () => {
    await openCrew();
    await waitFor(() => expect(card("Writer")).toBeInTheDocument());

    await userEvent.click(
      within(card("Writer")).getByRole("button", { name: "Edit" }),
    );
    await userEvent.clear(screen.getByLabelText("NAME"));
    await userEvent.type(screen.getByLabelText("NAME"), "Ghostwriter");
    await userEvent.click(screen.getByRole("button", { name: "Notion" }));
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(updateBot).toHaveBeenCalledWith(
      expect.objectContaining({
        botId: "writer",
        name: "Ghostwriter",
        tools: ["gmail", "notion"],
        harnessId: "claude",
      }),
    );
    // The grid is showing the row the host now holds, not the form's echo.
    await waitFor(() => expect(card("Ghostwriter")).toBeInTheDocument());
    expect(within(card("Ghostwriter")).getByText("Notion")).toBeInTheDocument();
    expect(screen.queryByLabelText("NAME")).not.toBeInTheDocument();
  });

  it("adds a bot from a template through the host", async () => {
    await openCrew();
    await waitFor(() => expect(card("Writer")).toBeInTheDocument());

    await userEvent.click(screen.getByRole("button", { name: /Add a bot/ }));
    await userEvent.selectOptions(
      screen.getByLabelText("START FROM A TEMPLATE"),
      "expense",
    );
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(createBot).toHaveBeenCalledWith(
      expect.objectContaining({ templateId: "expense", name: "Expense Manager" }),
    );
    await waitFor(() => expect(card("Expense Manager")).toBeInTheDocument());
  });

  it("removes a worker and stops showing it", async () => {
    await openCrew();
    await waitFor(() => expect(card("Writer")).toBeInTheDocument());

    await userEvent.click(
      within(card("Writer")).getByRole("button", { name: "Remove" }),
    );

    expect(removeBot).toHaveBeenCalledWith({ botId: "writer" });
    await waitFor(() => expect(within(grid()).queryByText("Writer")).toBeNull());
    expect(card("Chief")).toBeInTheDocument();
  });

  it("gives Chief no Remove, and keeps it if one is somehow pressed", async () => {
    await openCrew();
    await waitFor(() => expect(card("Chief")).toBeInTheDocument());

    expect(
      within(card("Chief")).queryByRole("button", { name: "Remove" }),
    ).not.toBeInTheDocument();

    // The editor is the other way in, and it hides the button too — so the
    // refusal is asserted where the UI cannot reach: at the host call.
    await expect(removeBot({ botId: "chief" })).rejects.toMatchObject({
      code: RPC_ERROR.CHIEF_REQUIRED,
    });
    await userEvent.click(
      within(card("Chief")).getByRole("button", { name: "Edit" }),
    );
    const editor = screen.getByRole("dialog", { name: "Customize Chief" });
    expect(
      within(editor).queryByRole("button", { name: "Remove" }),
    ).not.toBeInTheDocument();
  });

  it("keeps the form and says why when the host refuses a save", async () => {
    updateBot.mockRejectedValueOnce(
      new HostRpcError({
        code: RPC_ERROR.INVALID_PARAMS,
        message: "unknown tool: telepathy",
      }),
    );
    await openCrew();
    await waitFor(() => expect(card("Writer")).toBeInTheDocument());

    await userEvent.click(
      within(card("Writer")).getByRole("button", { name: "Edit" }),
    );
    await userEvent.clear(screen.getByLabelText("NAME"));
    await userEvent.type(screen.getByLabelText("NAME"), "Ghostwriter");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "unknown tool: telepathy",
    );
    // The editor is still open holding what the user typed.
    expect(screen.getByLabelText("NAME")).toHaveValue("Ghostwriter");
  });
});
