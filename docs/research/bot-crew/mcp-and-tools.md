# MCP and tools

Which servers to use for the prototype's eight chips, and how auth should
work. Crew bots that need tools use MCP; code threads get the same servers
on ACP `session/new`
([session-setup](https://agentclientprotocol.com/protocol/v1/session-setup)).

## Decision

JaBot owns an **MCP catalog** (one connection per provider, not per bot).
Each bot's `tools[]` is an allowlist of catalog ids. The host is the MCP
**client** for worker loops. For Code, the host **passes** connection
details into the harness (stdio command or HTTP URL + headers).

Auth pattern (data-and-persistence implements storage; this is the
policy):

1. **Remote MCP** (Google, GitHub, Notion, Slack): OAuth 2.1 authorization
   code + PKCE. JaBot is a public desktop client. Access/refresh tokens go
   in the **OS secret store**, never in crew JSON or SQLite plaintext.
2. **Local stdio MCP** (Playwright, optional GitHub docker, community
   Workspace fallback): no bearer to store, or a PAT in the same secret
   store injected as env at spawn.
3. **Reuse grants** across bots. Inbox Mgr and Writer both using Gmail =
   one Google grant, two allowlists.
4. **Do not** invent a second "JaBot Gmail API" beside MCP.

On macOS, "Keychain" is the right *backend*. Prefer the shell's native
API (Electron [`safeStorage`](https://electronjs.org/docs/latest/api/safe-storage)
— Keychain-backed; Tauri stronghold/keyring) over the unmaintained
`keytar` module. Ciphertext-on-disk + OS wrapping is fine; raw tokens in
`~/Library/Application Support/jabot` is not.

## ACP attachment

Stdio is **required** of ACP agents; HTTP is optional
(`mcpCapabilities.http`). Official Google / GitHub / Notion / Slack
servers are **remote HTTP**. That implies:

- Prefer harnesses that advertise HTTP MCP (Claude's ACP adapter
  documents client MCP servers).
- If a harness is stdio-only, spawn a **local MCP proxy** (stdio in,
  streamable HTTP out with the bearer from the keychain) rather than
  giving the subprocess the refresh token on a long-lived env if we can
  avoid it. Short-lived access token in `headers` on `session/new` is
  the ACP HTTP shape.

v1 `session/new` example (stdio):

```json
{
  "method": "session/new",
  "params": {
    "cwd": "/abs/path",
    "mcpServers": [
      {
        "name": "github",
        "command": "/abs/github-mcp-server",
        "args": ["stdio"],
        "env": [{ "name": "GITHUB_PERSONAL_ACCESS_TOKEN", "value": "from-keychain" }]
      }
    ]
  }
}
```

HTTP when the agent supports it:

```json
{
  "type": "http",
  "name": "gmail",
  "url": "https://gmailmcp.googleapis.com/mcp/v1",
  "headers": [{ "name": "Authorization", "value": "Bearer …" }]
}
```

Do not put refresh tokens in ACP params that might be logged.

## Tool-by-tool

Solid = official or Microsoft/GitHub-owned, documented, OAuth or local
profile, good enough for MVP. Community stdio is a **fallback**, not the
headline.

### Gmail — solid (official remote)

- URL: `https://gmailmcp.googleapis.com/mcp/v1`
- Docs: [Configure Google Workspace MCP](https://developers.google.com/workspace/guides/configure-mcp-servers)
- Status: public developer preview (rolled out from 1 May 2026;
  [announcement](https://workspaceupdates.googleblog.com/2026/05/agent-tools-and-security-updates-for-workspace-developers.html))
- Auth: OAuth 2.0; Google Cloud project; enable Gmail API + `gmailmcp.googleapis.com`
- Scopes (from Google's own list): `gmail.readonly`, `gmail.compose`
- Tools Google documents for tests: `gmail.search_threads`,
  `gmail.get_thread`, `gmail.create_draft`

**Fits Inbox Mgr and Writer.** Prototype: "Park drafts for anything that
needs my voice" maps to **compose/draft, not send**. Keep send behind a
permission prompt even if a write scope appears later.

JaBot will need its own OAuth client (Web application + loopback /
`jabot://` redirect), not Antigravity's or Claude.ai's callback URLs.

### Calendar — solid enough (official remote)

- URL: `https://calendarmcp.googleapis.com/mcp/v1`
- Docs: [Calendar MCP](https://developers.google.com/workspace/calendar/api/guides/configure-mcp-server)
  and the same Workspace guide.
- Blog copy: "finding available times and managing events."
- Consent scopes currently listed on the configure page are **read-ish**
  (`calendar.events.readonly`, freebusy, calendarlist). **Confirm write
  tools at implement time.** If preview Calendar cannot create events,
  use a maintained community stdio server as fallback
  ([taylorwilsdon/google_workspace_mcp](https://github.com/taylorwilsdon/google_workspace_mcp)
  is the usual full-suite option) until Google's write surface lands.

**Fits Scheduler.** Do not use EventKit as the MCP. EventKit is a native
calendar store; the prototype's tool is Google Calendar (and later others
as more catalog entries).

### Drive — solid (official remote)

- URL: `https://drivemcp.googleapis.com/mcp/v1`
- Tools Google cites: `drive.search_files`, `drive.read_file_content`
- Scopes listed: `drive.readonly`, `drive.file`

**Fits Expense** (receipts, monthly report). Prefer `drive.file` over
full-drive write.

### GitHub — solid (official)

- Repo: [github/github-mcp-server](https://github.com/github/github-mcp-server)
- Remote: `https://api.githubcopilot.com/mcp/` (OAuth via a GitHub App
  JaBot registers, or PAT in headers)
- Local: binary/docker + `GITHUB_PERSONAL_ACCESS_TOKEN` or local OAuth
  (token kept in memory by their login helper — we should still persist
  *our* PAT in keychain if we use PAT)

**Fits Code.** Issues, PRs, code search. The harness can also `gh` via
Terminal; MCP is structured and permission-friendlier. Ship GitHub MCP
on Code sessions; do not require it for a hello-world script.

### Terminal — not an MCP for MVP

There is no "official Terminal MCP" we should standardize on. ACP
`execute` / harness bash **is** Terminal for Code (and for Ops work that
is actually a folded code session).

If a worker chip is Terminal (Ops template), interpret it as **permission
to spawn a code session**, not a free shell inside the thin loop.

A later, tiny sandboxed exec MCP is allowed; do not start there.

### Browser — solid (Microsoft Playwright MCP)

- [microsoft/playwright-mcp](https://github.com/microsoft/playwright-mcp)
  (`npx @playwright/mcp@latest`)
- Accessibility-tree tools, not screenshot-only.
- **Persistent profile** is the default for the MCP server
  ([user profile](https://playwright.dev/mcp/configuration/user-profile)):
  cookies survive. Point `--user-data-dir` at a JaBot-owned directory
  (treat like a credential). `--isolated` for one-shot research.

**Fits Research, Talent, Social.** Headed first-run so the user can log
into sites; then reuse the profile. One profile lock at a time — don't
run two Playwright MCP processes on the same user-data-dir.

Chrome DevTools MCP / other browser servers exist; Playwright is the
one with first-party Microsoft maintenance and huge adoption. Use it.

### Notion — solid (official remote)

- Docs: [developers.notion.com/docs/mcp](https://developers.notion.com/docs/mcp)
- URL: `https://mcp.notion.com/mcp` (SSE fallback `/sse`)
- Auth: OAuth 2.0 + PKCE only on the hosted server
  ([build a client](https://developers.notion.com/guides/mcp/build-mcp-client))
- Local `@notionhq/notion-mcp-server` + `NOTION_TOKEN` is
  **soft-deprecated**; Notion is investing in remote. Use local token
  only for headless cron if OAuth refresh is painful — not the default.

**Fits Research and Writer.**

### Slack — solid (official remote)

- User guide: [Guide to the Slack MCP server](https://slack.com/help/articles/48855576908307-Guide-to-the-Slack-MCP-server)
- URL used by custom clients: `https://mcp.slack.com/mcp`
- Auth: per-user OAuth (user token). JaBot must be a Slack app with MCP
  enabled — partner one-click is for Claude/Cursor, not us.
  Practical writeup: [Adam Jones, custom client](https://adamjones.me/blog/slack-mcp-custom-client/).
- Tools: search, read/send messages, canvases, member info.

**Fits Ops.** Workspace admin approval may block personal installs;
surface that in the connection UI.

Community Slack stdio servers (bot tokens) are worse at search (need
user tokens). Prefer official remote.

## Catalog vs chips

Prototype chips → catalog ids:

| Chip | Catalog entry | Transport | Auth |
|---|---|---|---|
| Gmail | Google Gmail MCP | HTTP | OAuth (Google) |
| Calendar | Google Calendar MCP | HTTP | OAuth (Google, same project) |
| Drive | Google Drive MCP | HTTP | OAuth (Google) |
| GitHub | GitHub MCP | HTTP or stdio | OAuth app or PAT |
| Browser | `@playwright/mcp` | stdio | persistent profile |
| Notion | Notion MCP | HTTP | OAuth (Notion) |
| Slack | Slack MCP | HTTP | OAuth (Slack user) |
| Terminal | *(none)* | ACP execute | harness login |

Google can share one OAuth client and multiple scopes; still **separate
MCP URLs** per product. Connect Gmail without Calendar if the user never
chips Calendar.

## Auth UX

Settings / Crew: **Connections** list (Gmail connected as
you@domain, GitHub as @user, …). Bot editor only toggles chips. A chip
that isn't connected shows "Connect Gmail" instead of silently failing.

OAuth: system browser + loopback (`127.0.0.1`) or custom URL scheme.
PKCE required ([MCP authorization](https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/authorization)).
Stdio MCP OAuth-in-the-server is fragile; for Google/Notion/Slack/GitHub
we want **JaBot to complete OAuth**, then attach the bearer.

Revoke: delete keychain items + provider disconnect. Do not leave PATs
in `.env` files next to crew config.

## What we skip

- Rolling our own Gmail/IMAP.
- Putting API keys in bot instruction text.
- One mega "Google Workspace" MCP as the only option — Google split
  products; our chips match that split.
- MCP-over-ACP ([RFD](https://agentclientprotocol.com/rfds/mcp-over-acp))
  until adapters actually advertise `mcpCapabilities.acp`. Nice for
  host-side tools later; not MVP.
