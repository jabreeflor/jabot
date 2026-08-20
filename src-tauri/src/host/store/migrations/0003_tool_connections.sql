-- Tool/MCP connections (#18). The *non-secret* half of a provider grant.
--
-- Decision #6: one user-level OAuth grant per provider (one Gmail login), and
-- each bot allowlists which grants it may use — so the key is the provider,
-- not the bot and not the catalog entry. Gmail, Calendar and Drive are three
-- catalog entries drawing on one Google row.
--
-- Tokens are NOT here. `secret_ref_id` points at a `secret_refs` row whose
-- bytes live in the OS keychain; this table holds only what the bot editor's
-- chips need in order to say "connected as you@example.com" or "needs auth"
-- without touching the vault (and so without a keychain prompt per render).
-- `client_id` is a public OAuth client identifier — it travels in the
-- authorize URL — so it is not a secret; a `client_secret`, if a provider's
-- dynamic registration hands one out, goes in the vault with the tokens.

CREATE TABLE tool_connections (
  provider       TEXT PRIMARY KEY,
  status         TEXT NOT NULL
                   CHECK (status IN ('connected', 'needs_auth', 'error')),
  -- Display only: which account the human authorised, when the provider says.
  account        TEXT,
  scopes_json    TEXT NOT NULL DEFAULT '[]',
  secret_ref_id  TEXT REFERENCES secret_refs(id) ON DELETE SET NULL,
  client_id      TEXT,
  -- Access-token expiry. A grant with a refresh token is still `connected`
  -- past it; the host refreshes on use.
  expires_at     TEXT,
  last_error     TEXT,
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);
