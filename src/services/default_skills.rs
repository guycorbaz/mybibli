//! Story 17-6 (#656, FR177) — the MCP skills mybibli ships with.
//!
//! Each body is a versioned file under `skills/defaults/`, embedded in the
//! binary; its one-line description sits beside it in [`DEFAULT_SKILLS`].
//! The files double as examples for writing other skills.
//!
//! At startup [`insert_defaults`] offers each default **once per
//! database** (`data_repairs::run_once`, marker `default-skill:<name>`):
//!
//! - inserted **inactive**, with no author and `provided_by_mybibli` set,
//!   so Admin → Skills reads "provided by mybibli"; the admin turns it on;
//! - a name already held by any skill, live or in the Trash, is skipped
//!   with an `info` log and the admin's row is never touched; the marker is
//!   written all the same (the default was offered);
//! - a default the admin deletes, or purges, never comes back.
//!
//! **A later release never updates a default automatically** (story 17-6,
//! D5): once inserted, the row belongs to the admin. An improved text ships
//! as the new version of its file and is announced in the release notes;
//! the admin copies it if they want it. A *new* default added by a later
//! release has its own marker, so it is inserted on existing databases.

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::mcp_skill::McpSkillModel;
use crate::services::data_repairs;
use crate::services::mcp_skills::{SkillFieldError, ValidSkill, validate};

/// One default skill as shipped.
#[derive(Debug, Clone, Copy)]
pub struct DefaultSkill {
    pub name: &'static str,
    pub description: &'static str,
    pub body: &'static str,
}

/// The defaults, by name. Adding one here (and its file) is enough: its
/// marker is new, so the next startup inserts it everywhere.
pub const DEFAULT_SKILLS: &[DefaultSkill] = &[
    DefaultSkill {
        name: "comics-covers",
        description: "Propose the cover of the right edition for comics without one.",
        body: include_str!("../../skills/defaults/comics-covers.md"),
    },
    DefaultSkill {
        name: "comics-series-attach",
        description: "Propose to attach titles to their existing series, at their position.",
        body: include_str!("../../skills/defaults/comics-series-attach.md"),
    },
    DefaultSkill {
        name: "comics-series-create",
        description: "Propose to create a missing series with the library's titles.",
        body: include_str!("../../skills/defaults/comics-series-create.md"),
    },
    DefaultSkill {
        name: "comics-title-spelling",
        description: "Propose corrections of clear spelling mistakes in comic titles.",
        body: include_str!("../../skills/defaults/comics-title-spelling.md"),
    },
];

impl DefaultSkill {
    /// The `data_repairs` marker of this default.
    pub fn marker(&self) -> String {
        format!("default-skill:{}", self.name)
    }

    /// The default as a skill to store: validated like any skill, inactive.
    pub fn to_valid(&self) -> Result<ValidSkill, SkillFieldError> {
        validate(self.name, self.description, self.body, false)
    }
}

/// What one startup run did, by name.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DefaultsReport {
    pub inserted: Vec<&'static str>,
    /// The name was held by a skill (live or in the Trash) of the admin's.
    pub name_taken: Vec<&'static str>,
    /// Offered on an earlier startup (marker present): nothing done.
    pub already_offered: Vec<&'static str>,
}

/// Offer every default once per database. A failure on one default is
/// logged and leaves no marker (retried next boot); the others go on.
pub async fn insert_defaults(pool: &DbPool) -> DefaultsReport {
    let mut report = DefaultsReport::default();
    for default in DEFAULT_SKILLS {
        match offer(pool, default).await {
            Ok(Some(true)) => {
                tracing::info!(skill = default.name, "Default MCP skill inserted (inactive)");
                report.inserted.push(default.name);
            }
            Ok(Some(false)) => {
                tracing::info!(
                    skill = default.name,
                    "Default MCP skill skipped: a skill with this name already exists (live or in the Trash)"
                );
                report.name_taken.push(default.name);
            }
            Ok(None) => report.already_offered.push(default.name),
            Err(e) => tracing::error!(
                skill = default.name,
                error = %e,
                "Default MCP skill: insertion failed (non-fatal, retried next boot)"
            ),
        }
    }
    report
}

/// `Some(inserted)` once; `None` when the default was already offered.
async fn offer(pool: &DbPool, default: &DefaultSkill) -> Result<Option<bool>, AppError> {
    let skill = default.to_valid().map_err(|e| {
        AppError::Internal(format!(
            "default skill {} fails validation: {}",
            default.name,
            e.message("en")
        ))
    })?;
    data_repairs::run_once(pool, &default.marker(), move |conn| {
        Box::pin(async move { McpSkillModel::insert_default_in(conn, &skill).await })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_passes_skill_validation_unchanged() {
        assert_eq!(DEFAULT_SKILLS.len(), 4);
        for d in DEFAULT_SKILLS {
            let valid = d.to_valid().unwrap_or_else(|e| panic!("{}: {e:?}", d.name));
            assert_eq!(valid.name, d.name, "the name is stored as shipped");
            assert_eq!(valid.description, d.description, "no trimming needed");
            assert_eq!(valid.body, d.body, "the body is stored exactly as shipped");
            assert!(!valid.active, "defaults are inserted inactive");
        }
    }

    #[test]
    fn names_and_markers_are_unique_and_fit_the_marker_column() {
        let mut names: Vec<_> = DEFAULT_SKILLS.iter().map(|d| d.name).collect();
        let sorted = {
            let mut s = names.clone();
            s.sort_unstable();
            s
        };
        assert_eq!(names, sorted, "kept by name");
        names.dedup();
        assert_eq!(names.len(), DEFAULT_SKILLS.len());
        for d in DEFAULT_SKILLS {
            assert_eq!(d.marker(), format!("default-skill:{}", d.name));
            assert!(d.marker().len() <= 100, "data_repairs.name is VARCHAR(100)");
        }
    }

    /// Story 17-6 review: a default is never updated once inserted, so a
    /// tool it names must exist when it ships. Every backticked word shaped
    /// like a tool name (`get_…`, `list_…`, `propose_…`, `search_…`) is a
    /// registered MCP tool.
    #[test]
    fn every_tool_a_default_names_is_registered() {
        let registered: Vec<&str> = crate::mcp::tools::registry()
            .iter()
            .map(|t| t.name())
            .collect();
        for d in DEFAULT_SKILLS {
            let mut named = 0;
            for word in d.body.split('`').skip(1).step_by(2) {
                let tool_like = ["get_", "list_", "propose_", "search_"]
                    .iter()
                    .any(|p| word.starts_with(p))
                    && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
                if tool_like {
                    named += 1;
                    assert!(registered.contains(&word), "{}: unknown tool `{word}`", d.name);
                }
            }
            assert!(named > 0, "{}: names no tool", d.name);
        }
    }

    #[test]
    fn bodies_are_the_comic_texts_of_654() {
        for d in DEFAULT_SKILLS.iter().filter(|d| d.name.starts_with("comics-")) {
            assert!(d.body.contains("## Ground rules"), "{}", d.name);
            assert!(d.body.contains("`media_type` = `bd`"), "{}", d.name);
            assert!(d.body.ends_with('\n') && !d.body.ends_with("\n\n"), "{}", d.name);
        }
    }
}
