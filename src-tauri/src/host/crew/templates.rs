//! Bot templates, shipped as JSON packs (#17).
//!
//! A template is a bot record without an identity (decision #6), so the packs
//! are data rather than a Rust table: adding one is a new file next to these
//! four, and the shape is checked at load rather than trusted.
//!
//! **Adding a template snapshots it.** [`super::HostSession::crew_create`]
//! copies these fields into a `bots` row and keeps only `template_id`, as
//! provenance. Nothing ever reads back through that id — no inheritance, no
//! sync, no "reset to template". That is the whole reason the packs can be
//! edited between releases without rewriting anyone's crew.
//!
//! The packs are `include_str!`-ed rather than read from disk. They ship with
//! the binary and the user does not edit them (a user's own bot is a bot, not
//! a template), so a missing file has to be a build failure — not a crew view
//! that is silently four cards short.

use serde::Deserialize;

use super::super::protocol::methods::BotTemplateView;

/// The shipped packs, in the order the editor's picker lists them.
const PACKS: &[(&str, &str)] = &[
    ("expense.json", include_str!("templates/expense.json")),
    ("talent.json", include_str!("templates/talent.json")),
    ("social.json", include_str!("templates/social.json")),
    ("ops.json", include_str!("templates/ops.json")),
];

/// A pack file. `deny_unknown_fields` so a typo in a shipped pack is a test
/// failure here rather than a silently missing tool in someone's crew.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TemplatePack {
    template_id: String,
    name: String,
    color: String,
    instructions: String,
    #[serde(default)]
    tools: Vec<String>,
    harness_id: String,
}

/// Every shipped template.
///
/// A malformed pack panics, and deliberately: these files are compiled into
/// the binary, so a bad one is a bug that exists before the app has a user,
/// and the tests below run on every build.
pub fn templates() -> Vec<BotTemplateView> {
    PACKS
        .iter()
        .map(|(file, raw)| {
            let pack: TemplatePack = serde_json::from_str(raw)
                .unwrap_or_else(|err| panic!("shipped template {file} is malformed: {err}"));
            BotTemplateView {
                template_id: pack.template_id,
                name: pack.name,
                color: pack.color,
                instructions: pack.instructions,
                tools: pack.tools,
                harness_id: pack.harness_id,
            }
        })
        .collect()
}

/// One template by id, for `crew/create { templateId }`.
pub fn find(template_id: &str) -> Option<BotTemplateView> {
    templates()
        .into_iter()
        .find(|template| template.template_id == template_id)
}

#[cfg(test)]
mod tests {
    use super::super::{is_known_tool, BOT_COLORS};
    use super::*;
    use crate::host::harness::catalog::is_reserved;

    #[test]
    fn every_shipped_pack_parses_and_is_named_once() {
        let templates = templates();
        let ids: Vec<_> = templates
            .iter()
            .map(|t| t.template_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["expense", "talent", "social", "ops"]);
        for template in &templates {
            assert!(!template.name.trim().is_empty(), "{:?}", template);
            assert!(
                template.instructions.len() > 20,
                "a template with no persona is an empty bot: {:?}",
                template
            );
        }
    }

    /// A pack naming a tool or a harness the host does not have would create a
    /// bot whose chips can never be passed to a session, or whose first prompt
    /// dies with `HARNESS_UNAVAILABLE`. The packs are data; this is the check
    /// that keeps them honest.
    #[test]
    fn packs_only_name_things_the_host_actually_has() {
        for template in templates() {
            // Reserved ids are exactly the compiled-in tiers 1 and 2, which
            // are the harnesses that exist before the user has installed
            // anything of their own.
            assert!(
                is_reserved(&template.harness_id),
                "{} names harness {}, which is not in the compiled-in catalog",
                template.template_id,
                template.harness_id
            );
            assert!(
                BOT_COLORS.contains(&template.color.as_str()),
                "{} names colour {}, which the UI cannot render",
                template.template_id,
                template.color
            );
            for tool in &template.tools {
                assert!(
                    is_known_tool(tool),
                    "{} allowlists {tool}, which is in no catalog",
                    template.template_id
                );
            }
        }
    }

    #[test]
    fn find_is_by_id_and_misses_cleanly() {
        assert_eq!(find("ops").unwrap().harness_id, "codex");
        assert!(find("no-such-pack").is_none());
    }
}
