//! Story 15-2 (#614, FR162) — validation of MCP skills.
//!
//! A skill is a named piece of Markdown an administrator writes once for
//! the agent (story 15-3 serves the active ones over MCP). This module
//! decides what a valid skill looks like; `models::mcp_skill` stores it and
//! `routes::admin_skills` is the web editor.
//!
//! Foundation Rule #20: every field is checked against a **positive
//! shape** and refused otherwise — nothing is rewritten to fit. A name with
//! an uppercase letter is refused, not lowercased; a description with a
//! line break is refused, not joined. Only surrounding whitespace of the
//! name and the description is trimmed (a form-input convention); the body
//! is stored exactly as typed.

/// Longest accepted name, in characters (the slug is ASCII, so bytes too).
pub const NAME_MAX_CHARS: usize = 64;
/// Longest accepted description, in characters (`VARCHAR(200)`).
pub const DESCRIPTION_MAX_CHARS: usize = 200;
/// Largest accepted body, in bytes of UTF-8 (64 KiB).
pub const BODY_MAX_BYTES: usize = 64 * 1024;

/// Why a field was refused. Each variant is one field-validation hint
/// (terse, per `docs/error-message-style.md` § 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillFieldError {
    NameRequired,
    NameTooLong,
    NameShape,
    DescriptionRequired,
    DescriptionTooLong,
    DescriptionNotOneLine,
    BodyRequired,
    BodyTooLarge,
    BodyControlCharacter,
}

impl SkillFieldError {
    fn key(self) -> &'static str {
        match self {
            SkillFieldError::NameRequired => "error.mcp_skills.name_required",
            SkillFieldError::NameTooLong => "error.mcp_skills.name_too_long",
            SkillFieldError::NameShape => "error.mcp_skills.name_shape",
            SkillFieldError::DescriptionRequired => "error.mcp_skills.description_required",
            SkillFieldError::DescriptionTooLong => "error.mcp_skills.description_too_long",
            SkillFieldError::DescriptionNotOneLine => {
                "error.mcp_skills.description_not_one_line"
            }
            SkillFieldError::BodyRequired => "error.mcp_skills.body_required",
            SkillFieldError::BodyTooLarge => "error.mcp_skills.body_too_large",
            SkillFieldError::BodyControlCharacter => "error.mcp_skills.body_control_character",
        }
    }

    /// The localized hint.
    pub fn message(self, loc: &str) -> String {
        rust_i18n::t!(
            self.key(),
            locale = loc,
            max_name = NAME_MAX_CHARS,
            max_description = DESCRIPTION_MAX_CHARS,
            max_kib = BODY_MAX_BYTES / 1024
        )
        .to_string()
    }
}

/// A skill that passed every check, ready to be stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidSkill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub active: bool,
}

/// `true` when `name` is a slug: one or more groups of `[a-z0-9]`, joined
/// by single hyphens (`^[a-z0-9]+(-[a-z0-9]+)*$`). Written by hand rather
/// than with a regex crate — the grammar is three rules long.
pub fn is_slug(name: &str) -> bool {
    !name.is_empty()
        && name.split('-').all(|group| {
            !group.is_empty()
                && group
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Validate a name (trimmed first).
pub fn validate_name(raw: &str) -> Result<String, SkillFieldError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(SkillFieldError::NameRequired);
    }
    if name.chars().count() > NAME_MAX_CHARS {
        return Err(SkillFieldError::NameTooLong);
    }
    if !is_slug(name) {
        return Err(SkillFieldError::NameShape);
    }
    Ok(name.to_string())
}

/// Validate a description (trimmed first): one line of text, no control
/// character (a line break, a tab, …), at most 200 characters.
pub fn validate_description(raw: &str) -> Result<String, SkillFieldError> {
    let description = raw.trim();
    if description.is_empty() {
        return Err(SkillFieldError::DescriptionRequired);
    }
    if description.chars().any(char::is_control) {
        return Err(SkillFieldError::DescriptionNotOneLine);
    }
    if description.chars().count() > DESCRIPTION_MAX_CHARS {
        return Err(SkillFieldError::DescriptionTooLong);
    }
    Ok(description.to_string())
}

/// Validate a body, kept exactly as typed: not blank, at most 64 KiB of
/// UTF-8, and no control character other than line breaks and tabs
/// (Markdown text; a NUL or an escape sequence has no place in it).
pub fn validate_body(raw: &str) -> Result<String, SkillFieldError> {
    if raw.trim().is_empty() {
        return Err(SkillFieldError::BodyRequired);
    }
    if raw.len() > BODY_MAX_BYTES {
        return Err(SkillFieldError::BodyTooLarge);
    }
    if raw
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(SkillFieldError::BodyControlCharacter);
    }
    Ok(raw.to_string())
}

/// Validate a whole skill. The first refused field wins, in form order.
pub fn validate(
    name: &str,
    description: &str,
    body: &str,
    active: bool,
) -> Result<ValidSkill, SkillFieldError> {
    Ok(ValidSkill {
        name: validate_name(name)?,
        description: validate_description(description)?,
        body: validate_body(body)?,
        active,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_names() {
        for name in [
            "a",
            "z9",
            "catalog-rules",
            "bd-tome-2",
            "2026",
            "a-b-c-d",
            &"x".repeat(NAME_MAX_CHARS),
        ] {
            assert_eq!(validate_name(name), Ok(name.to_string()), "{name}");
        }
        // Surrounding whitespace is a form-input artefact.
        assert_eq!(validate_name("  catalog-rules \n"), Ok("catalog-rules".into()));
    }

    #[test]
    fn refused_names() {
        assert_eq!(validate_name(""), Err(SkillFieldError::NameRequired));
        assert_eq!(validate_name("   "), Err(SkillFieldError::NameRequired));
        assert_eq!(
            validate_name(&"x".repeat(NAME_MAX_CHARS + 1)),
            Err(SkillFieldError::NameTooLong)
        );
        for name in [
            "Catalog",       // uppercase is refused, never lowercased
            "catalog_rules", // underscore
            "catalog rules", // inner space
            "-catalog",      // leading hyphen
            "catalog-",      // trailing hyphen
            "cata--log",     // double hyphen
            "catalogué",     // non-ASCII letter
            "cat.log",
            "cat/log",
            "<script>",
        ] {
            assert_eq!(validate_name(name), Err(SkillFieldError::NameShape), "{name}");
        }
    }

    #[test]
    fn is_slug_matches_the_documented_grammar() {
        assert!(is_slug("a-1"));
        assert!(!is_slug(""));
        assert!(!is_slug("-"));
        assert!(!is_slug("a--b"));
    }

    #[test]
    fn descriptions() {
        assert_eq!(
            validate_description("  Rules for cataloguing comics "),
            Ok("Rules for cataloguing comics".into())
        );
        assert_eq!(
            validate_description(&"é".repeat(DESCRIPTION_MAX_CHARS)),
            Ok("é".repeat(DESCRIPTION_MAX_CHARS)),
            "the limit counts characters, not bytes"
        );
        assert_eq!(validate_description(" "), Err(SkillFieldError::DescriptionRequired));
        assert_eq!(
            validate_description(&"a".repeat(DESCRIPTION_MAX_CHARS + 1)),
            Err(SkillFieldError::DescriptionTooLong)
        );
        for bad in ["line one\nline two", "a\rb", "a\tb", "a\u{0}b"] {
            assert_eq!(
                validate_description(bad),
                Err(SkillFieldError::DescriptionNotOneLine),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn bodies() {
        let md = "# Heading\r\n\n- item\twith tab\n<b>kept as text</b>\n";
        assert_eq!(validate_body(md), Ok(md.to_string()), "body stored exactly as typed");
        assert_eq!(validate_body(" \n\t "), Err(SkillFieldError::BodyRequired));
        let max = "a".repeat(BODY_MAX_BYTES);
        assert_eq!(validate_body(&max), Ok(max.clone()));
        assert_eq!(
            validate_body(&format!("{max}a")),
            Err(SkillFieldError::BodyTooLarge)
        );
        // The limit is in bytes: 32 768 two-byte characters fill it exactly.
        assert!(validate_body(&"é".repeat(BODY_MAX_BYTES / 2)).is_ok());
        assert_eq!(
            validate_body(&"é".repeat(BODY_MAX_BYTES / 2 + 1)),
            Err(SkillFieldError::BodyTooLarge)
        );
        assert_eq!(
            validate_body("text\u{0}nul"),
            Err(SkillFieldError::BodyControlCharacter)
        );
        assert_eq!(
            validate_body("esc\u{1b}[31m"),
            Err(SkillFieldError::BodyControlCharacter)
        );
    }

    #[test]
    fn validate_reports_the_first_refused_field_in_form_order() {
        assert_eq!(
            validate("Bad", "", "", true),
            Err(SkillFieldError::NameShape)
        );
        assert_eq!(
            validate("ok", "two\nlines", "", true),
            Err(SkillFieldError::DescriptionNotOneLine)
        );
        assert_eq!(validate("ok", "fine", "", false), Err(SkillFieldError::BodyRequired));
        let ok = validate("ok", "fine", "body", false).unwrap();
        assert!(!ok.active);
    }

    #[test]
    fn every_error_is_translated_in_every_locale() {
        let all = [
            SkillFieldError::NameRequired,
            SkillFieldError::NameTooLong,
            SkillFieldError::NameShape,
            SkillFieldError::DescriptionRequired,
            SkillFieldError::DescriptionTooLong,
            SkillFieldError::DescriptionNotOneLine,
            SkillFieldError::BodyRequired,
            SkillFieldError::BodyTooLarge,
            SkillFieldError::BodyControlCharacter,
        ];
        for loc in ["en", "fr", "de", "it"] {
            for err in all {
                let msg = err.message(loc);
                assert!(!msg.contains("error.mcp_skills"), "{loc}: {err:?} untranslated");
                assert!(!msg.contains("%{"), "{loc}: {err:?} has an unfilled placeholder: {msg}");
            }
        }
    }
}
