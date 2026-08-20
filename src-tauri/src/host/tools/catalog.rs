//! The host-owned MCP catalog (#18).
//!
//! One entry per provider surface, matching the prototype's chips. The host —
//! not the harness — decides which of these a session sees, so this table is
//! the only place a server URL or a launch command is written down.
//!
//! Three distinctions the taxonomy in
//! `docs/research/setup-porting/findings.md` insists on, and that this file
//! encodes rather than blurs:
//!
//! - A **capability** is what the model may call. That is an entry here plus a
//!   bot's allowlist, enforced by never sending the server (#18).
//! - A **credential** authorises the capability. It is a provider grant in the
//!   vault, keyed by [`Provider`] — Gmail, Calendar and Drive are three
//!   entries sharing one Google login, which is why `provider` is a separate
//!   field from `id`.
//! - A **skill** is neither and does not appear here. Installing one must
//!   never imply access to any of this.
//!
//! Terminal is deliberately **not** an MCP server. There is no official
//! Terminal MCP to standardise on and inventing one would put a shell behind a
//! tool schema; the harness's own `execute` is Terminal, gated by the
//! permission broker (#20). It is in the catalog as a capability the bot
//! editor can chip, with a transport that can never become an `mcpServers`
//! entry.

/// Where a provider grant lives. One row in `tool_connections`, one vault
/// item, shared by every entry that names it — decision #6's "one user-level
/// OAuth grant per provider; each bot allowlists which grants it may use".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    /// Extra authorize-request parameters this provider documents as required
    /// for the grant we need. Kept next to the provider rather than guessed at
    /// request time, and empty unless the vendor documents one.
    pub authorize_params: &'static [(&'static str, &'static str)],
}

pub const GOOGLE: Provider = Provider {
    id: "google",
    label: "Google",
    // Google only issues a refresh token when the authorize request asks for
    // offline access, and only re-issues one on a repeat consent. Without both
    // of these a Google grant dies at the first access-token expiry.
    authorize_params: &[("access_type", "offline"), ("prompt", "consent")],
};

pub const GITHUB: Provider = Provider {
    id: "github",
    label: "GitHub",
    authorize_params: &[],
};

pub const NOTION: Provider = Provider {
    id: "notion",
    label: "Notion",
    authorize_params: &[],
};

pub const SLACK: Provider = Provider {
    id: "slack",
    label: "Slack",
    authorize_params: &[],
};

/// How a tool reaches its provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    /// Remote MCP over streamable HTTP. The host completes OAuth and attaches
    /// a short-lived bearer per session; the refresh token never leaves the
    /// vault (`mcp-and-tools.md`: "do not put refresh tokens in ACP params").
    Http { url: &'static str },
    /// A local MCP subprocess the host spawns through the harness. No bearer
    /// to hold — state is a browser profile directory JaBot owns.
    Stdio {
        command: &'static str,
        args: &'static [&'static str],
        /// Flag that names a JaBot-owned profile directory, appended with a
        /// resolved absolute path at session time. Treat the directory itself
        /// as a credential: it holds the user's logged-in cookies.
        profile_flag: Option<&'static str>,
    },
    /// Not MCP. The harness's own `execute` tool, permission-gated per call.
    HarnessExecute,
}

/// A catalog entry. `id` is what a bot's `tools[]` allowlist names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolEntry {
    pub id: &'static str,
    pub label: &'static str,
    pub blurb: &'static str,
    pub transport: Transport,
    pub provider: Option<Provider>,
    /// What the grant asks for. Least privilege on purpose: draft, not send;
    /// `drive.file`, not full Drive.
    pub scopes: &'static [&'static str],
    pub docs_url: &'static str,
}

impl ToolEntry {
    /// Whether this entry can ever become an `mcpServers` element.
    pub fn is_mcp(&self) -> bool {
        !matches!(self.transport, Transport::HarnessExecute)
    }

    /// Whether using it requires a provider grant.
    pub fn needs_grant(&self) -> bool {
        self.provider.is_some() && matches!(self.transport, Transport::Http { .. })
    }
}

/// The catalog, in the order the bot editor shows its chips.
pub const CATALOG: &[ToolEntry] = &[
    ToolEntry {
        id: "gmail",
        label: "Gmail",
        blurb: "Search threads, read mail, park drafts",
        transport: Transport::Http {
            url: "https://gmailmcp.googleapis.com/mcp/v1",
        },
        provider: Some(GOOGLE),
        // Compose, not send: the prototype's Inbox Mgr parks drafts for
        // anything that needs the user's voice, and a send scope we do not ask
        // for is a send the model cannot be talked into.
        scopes: &[
            "https://www.googleapis.com/auth/gmail.readonly",
            "https://www.googleapis.com/auth/gmail.compose",
        ],
        docs_url: "https://developers.google.com/workspace/guides/configure-mcp-servers",
    },
    ToolEntry {
        id: "calendar",
        label: "Calendar",
        blurb: "Find free time, read and manage events",
        transport: Transport::Http {
            url: "https://calendarmcp.googleapis.com/mcp/v1",
        },
        provider: Some(GOOGLE),
        scopes: &[
            "https://www.googleapis.com/auth/calendar.events.readonly",
            "https://www.googleapis.com/auth/calendar.readonly",
        ],
        docs_url:
            "https://developers.google.com/workspace/calendar/api/guides/configure-mcp-server",
    },
    ToolEntry {
        id: "drive",
        label: "Drive",
        blurb: "Search files and read documents",
        transport: Transport::Http {
            url: "https://drivemcp.googleapis.com/mcp/v1",
        },
        provider: Some(GOOGLE),
        scopes: &[
            "https://www.googleapis.com/auth/drive.readonly",
            "https://www.googleapis.com/auth/drive.file",
        ],
        docs_url: "https://developers.google.com/workspace/guides/configure-mcp-servers",
    },
    ToolEntry {
        id: "github",
        label: "GitHub",
        blurb: "Issues, pull requests, code search",
        transport: Transport::Http {
            url: "https://api.githubcopilot.com/mcp/",
        },
        provider: Some(GITHUB),
        // GitHub scopes come from the app registration, not the authorize
        // request, so asking for none here is not a gap: the grant is whatever
        // the user's registered app was created with.
        scopes: &[],
        docs_url: "https://github.com/github/github-mcp-server",
    },
    ToolEntry {
        id: "browser",
        label: "Browser",
        blurb: "Drive a real browser: navigate, read, fill forms",
        transport: Transport::Stdio {
            command: "npx",
            args: &["-y", "@playwright/mcp@latest"],
            // Persistent profile so a site the user logged into once stays
            // logged in. One process per profile — the directory is locked.
            profile_flag: Some("--user-data-dir"),
        },
        provider: None,
        scopes: &[],
        docs_url: "https://github.com/microsoft/playwright-mcp",
    },
    ToolEntry {
        id: "notion",
        label: "Notion",
        blurb: "Search and edit your Notion workspace",
        transport: Transport::Http {
            url: "https://mcp.notion.com/mcp",
        },
        provider: Some(NOTION),
        scopes: &[],
        docs_url: "https://developers.notion.com/docs/mcp",
    },
    ToolEntry {
        id: "slack",
        label: "Slack",
        blurb: "Search and send messages as you",
        transport: Transport::Http {
            url: "https://mcp.slack.com/mcp",
        },
        provider: Some(SLACK),
        scopes: &[],
        docs_url: "https://slack.com/help/articles/48855576908307-Guide-to-the-Slack-MCP-server",
    },
    ToolEntry {
        id: "terminal",
        label: "Terminal",
        blurb: "Shell access through the harness, one approval at a time",
        transport: Transport::HarnessExecute,
        provider: None,
        scopes: &[],
        docs_url: "https://agentclientprotocol.com/protocol/v1/tool-calls",
    },
];

pub fn find(id: &str) -> Option<&'static ToolEntry> {
    CATALOG.iter().find(|entry| entry.id == id)
}

/// Every entry that draws on one provider grant.
pub fn entries_for_provider(
    provider_id: &str,
) -> impl Iterator<Item = &'static ToolEntry> + use<'_> {
    CATALOG
        .iter()
        .filter(move |entry| entry.provider.is_some_and(|p| p.id == provider_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_can_never_become_an_mcp_server() {
        let terminal = find("terminal").expect("terminal is a catalog capability");
        assert!(!terminal.is_mcp());
        assert!(!terminal.needs_grant());
        assert!(terminal.provider.is_none());
    }

    /// Three chips, one login. The whole reason `provider` is not `id`.
    #[test]
    fn google_entries_share_one_grant() {
        let google: Vec<&str> = entries_for_provider("google").map(|e| e.id).collect();
        assert_eq!(google, vec!["gmail", "calendar", "drive"]);
        for entry in entries_for_provider("google") {
            assert!(
                entry.needs_grant(),
                "{} should need the Google grant",
                entry.id
            );
        }
    }

    /// A remote server without a provider could never be authorised, and a
    /// grant on a local server would be a credential nobody consumes. Both are
    /// mistakes a new entry can make, so the catalog is checked as a whole.
    #[test]
    fn every_remote_entry_has_a_provider_and_every_local_one_does_not() {
        for entry in CATALOG {
            match &entry.transport {
                Transport::Http { url } => {
                    assert!(
                        entry.provider.is_some(),
                        "{} is remote with no provider",
                        entry.id
                    );
                    assert!(url.starts_with("https://"), "{} must be https", entry.id);
                }
                Transport::Stdio { command, .. } => {
                    assert!(
                        entry.provider.is_none(),
                        "{} is local but has a provider",
                        entry.id
                    );
                    assert!(!command.is_empty());
                }
                Transport::HarnessExecute => {
                    assert!(entry.provider.is_none());
                }
            }
        }
    }

    /// The seeded crew's chips have to name entries that exist, or a bot ships
    /// with an allowlist the host silently drops.
    #[test]
    fn seeded_bot_chips_resolve() {
        for id in [
            "gmail", "calendar", "github", "browser", "notion", "terminal",
        ] {
            assert!(find(id).is_some(), "seed.rs allowlists {id}");
        }
    }
}
