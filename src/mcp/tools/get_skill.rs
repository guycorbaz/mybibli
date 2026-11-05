//! Story 15-3 (#614, FR163) — `get_skill`: the body of one of the
//! library's skills (Markdown instructions an admin wrote for the agent,
//! story 15-2). The `initialize` instructions list the active skills;
//! this tool reads one. Skills only describe conventions: reading one
//! grants nothing (a `propose` key stays a `propose` key).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{args, to_json};
use crate::mcp::tool::{McpTool, ToolContext, ToolError, ToolFuture};
use crate::models::mcp_skill::ActiveSkill;
use crate::services::mcp_skills::{
    NAME_MAX_CHARS, ServedSkills, is_served_name, served_skill, served_skills,
};
use crate::services::proposal_history::rfc3339_utc;

pub struct GetSkill;

const TOOL: &str = "get_skill";

/// The 15-2 name shape, as a JSON Schema pattern.
const NAME_PATTERN: &str = "^[a-z0-9]+(-[a-z0-9]+)*$";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    name: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct Answer {
    name: String,
    description: String,
    /// Markdown, exactly as the admin wrote it.
    body: String,
    /// ISO 8601, UTC.
    updated_at: String,
}

impl From<ActiveSkill> for Answer {
    fn from(s: ActiveSkill) -> Self {
        Answer {
            name: s.name,
            description: s.description,
            body: s.body,
            updated_at: rfc3339_utc(s.updated_at),
        }
    }
}

/// The message for a name no active skill has: the names the agent may
/// ask for instead (or that there is none).
fn not_found(name: &str, served: &ServedSkills) -> ToolError {
    let what = format!("No active skill is named \"{name}\".");
    if served.listed.is_empty() {
        return ToolError::NotFound(format!("{what} The library has no active skill."));
    }
    let names: Vec<&str> = served.listed.iter().map(|s| s.name.as_str()).collect();
    let more = served.total.saturating_sub(names.len() as u64);
    let tail = if more > 0 {
        format!(" (and {more} more not listed)")
    } else {
        String::new()
    };
    ToolError::NotFound(format!(
        "{what} The active skills are: {}{tail}.",
        names.join(", ")
    ))
}

impl McpTool for GetSkill {
    fn name(&self) -> &'static str {
        TOOL
    }

    fn title(&self) -> &'static str {
        "Read one of the library's skills"
    }

    fn description(&self) -> &'static str {
        "Read a skill: instructions the librarians wrote for agents working on this library \
         (house conventions for cataloguing, series, covers, sources…). The server \
         instructions list the active skills by `name` with a one-line description; read the \
         one that matches your task before acting, and follow it. Returns the skill's `name`, \
         `description`, `body` (Markdown, as the librarian wrote it) and `updated_at` (ISO 8601, \
         UTC). An unknown or inactive name is an error that lists the names of the active \
         skills (at most the first 200). A skill grants no access: it never widens what your \
         key may do. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": NAME_MAX_CHARS,
                    "pattern": NAME_PATTERN,
                    "description": "The skill's name, exactly as listed (lowercase letters, digits and single hyphens, e.g. \"bd-series\")."
                }
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, ctx: &'a ToolContext, args: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let Args { name } = args::parse_object(args, "{\"name\": string}")?;
            if !is_served_name(&name) {
                return Err(ToolError::InvalidArguments(format!(
                    "`name` must be a skill name: lowercase letters, digits and single hyphens, \
                     at most {NAME_MAX_CHARS} characters"
                )));
            }
            match served_skill(&ctx.state.pool, &name).await? {
                Some(skill) => to_json(Answer::from(skill), TOOL),
                // The name is wrong either way: a failed read of the names
                // only drops the list from the answer, never turns it into
                // a server error.
                None => Err(match served_skills(&ctx.state.pool).await {
                    Ok(served) => not_found(&name, &served),
                    Err(e) => {
                        tracing::warn!(error = %e, "get_skill: active names not listed");
                        ToolError::NotFound(format!("No active skill is named \"{name}\"."))
                    }
                }),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::mcp_skill::ActiveSkillSummary;
    use crate::services::mcp_skills::MAX_SERVED_SKILLS;

    #[test]
    fn arguments() {
        assert!(args::parse_object::<Args>(json!({"name": "bd-series"}), "").is_ok());
        for bad in [
            json!({}),
            json!({"name": 3}),
            json!({"name": "a", "x": 1}),
            Value::Null,
        ] {
            assert!(
                matches!(
                    args::parse_object::<Args>(bad.clone(), ""),
                    Err(ToolError::InvalidArguments(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_schema_pattern_is_the_15_2_slug() {
        let schema = GetSkill.input_schema();
        assert_eq!(schema["properties"]["name"]["pattern"], NAME_PATTERN);
        // The schema pattern and the 15-2 validation accept the same names.
        let pattern = regex::Regex::new(NAME_PATTERN).unwrap();
        for name in [
            "covers", "bd-series", "a1-b2-c3", "0ss-1", "Covers", "bd--series", "-bd", "bd-",
            "bd_series", "bd series", "é", "",
        ] {
            assert_eq!(
                pattern.is_match(name),
                crate::services::mcp_skills::validate_name(name).is_ok(),
                "{name:?}"
            );
        }
        assert_eq!(schema["properties"]["name"]["maxLength"], 64);
        assert!(GetSkill.description().contains("first 200"));
        assert_eq!(MAX_SERVED_SKILLS, 200);
    }

    #[test]
    fn answer_carries_the_body_and_a_utc_date() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 11, 4)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        let answer = Answer::from(ActiveSkill {
            name: "covers".into(),
            description: "Which cover".into(),
            body: "# Covers\n\nPrefer the publisher's.".into(),
            updated_at: at,
        });
        assert_eq!(
            serde_json::to_value(answer).unwrap(),
            json!({
                "name": "covers",
                "description": "Which cover",
                "body": "# Covers\n\nPrefer the publisher's.",
                "updated_at": "2026-11-04T09:30:00Z"
            })
        );
    }

    #[test]
    fn not_found_lists_the_active_names() {
        let none = not_found("x", &ServedSkills::default());
        assert_eq!(
            none.message(),
            "No active skill is named \"x\". The library has no active skill."
        );
        let served = ServedSkills {
            listed: vec![
                ActiveSkillSummary {
                    name: "a".into(),
                    description: "d".into(),
                },
                ActiveSkillSummary {
                    name: "b".into(),
                    description: "d".into(),
                },
            ],
            total: 5,
        };
        assert_eq!(
            not_found("x", &served).message(),
            "No active skill is named \"x\". The active skills are: a, b (and 3 more not listed)."
        );
    }
}
