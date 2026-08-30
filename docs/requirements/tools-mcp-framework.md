# Tool/MCP framework: catalog, OAuth, allowlists

**Issue:** #18
**Status:** Implemented — `src-tauri/src/host/tools/`

## What it is

The host-owned catalog of tools/MCP servers a crew bot can be granted
(Gmail, Calendar, GitHub, etc.), the OAuth flow that authorizes them, and
the per-bot allowlist that determines which servers actually get passed
into a given ACP `session/new` call.

## Why

Per the architecture decision, MCP servers reaching a session come **only**
from JaBot's own catalog, never from whatever a harness happens to have
configured ambiently — this module is where that boundary is enforced,
and where user credentials for third-party services are acquired and
protected.

## Requirements

1. `catalog.rs` lists available tool/MCP servers with enough metadata
   (name, description, required scopes) for the UI to present a picker
   per bot.
2. `servers.rs` knows how to actually launch/connect each cataloged MCP
   server; `clients.rs` provides the client-side plumbing used when the
   host itself needs to call a tool directly (e.g. for Chief tools, see
   [chief-of-staff-bot.md](chief-of-staff-bot.md)).
3. OAuth authorization (`oauth.rs`, `flow.rs`, `loopback.rs`) runs a
   standard authorization-code flow using a local loopback redirect;
   obtained tokens are stored via the secrets vault
   (`src-tauri/src/host/store/secrets.rs`), never in plain SQLite rows —
   `crypto.rs` provides the encryption used before anything touches
   disk outside the keychain.
4. `http.rs` centralizes outbound HTTP for tool/OAuth calls so timeouts,
   retries, and error mapping are consistent across tools instead of
   reimplemented per integration.
5. A crew bot only receives the MCP servers explicitly allowlisted on it
   (see requirement 1 of [crew-management.md](crew-management.md)); the
   host does not pass a superset "just in case."
6. Ambient harness-configured MCP servers are suppressed at spawn time
   (see requirement 10 of
   [harness-adapter-layer.md](harness-adapter-layer.md)) so the
   allowlist is authoritative.
7. `testing.rs` provides fakes/test doubles for tool servers so tool
   flow (grant → allowlist → session receives it) is verifiable without
   live third-party accounts.
8. Revoking a tool's authorization removes its token from the keychain
   and removes it from any bot's allowlist that referenced it, rather
   than leaving a dangling grant a bot could still silently use.
