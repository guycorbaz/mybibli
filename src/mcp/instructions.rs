//! Story 12-3 (#560, FR150) — the server instructions an MCP client
//! receives at `initialize`: what the server is, and which sources
//! mybibli already queries automatically, so an agent looks elsewhere
//! first. Those sources update their catalogs: they are a lower
//! priority for the agent, never an exclusion (decision of 2026-10-30).
//!
//! Story 15-3 (#614, FR163) adds the "Library skills" section: the
//! active skills an admin wrote (name + description, bounded), and the
//! advice to read the relevant one with `get_skill` before acting.
//!
//! Pure; `transport.rs` feeds it the live registry, the cover fallbacks
//! and the active skills (read from the database) on every `initialize` /
//! `server/discover` (a provider key or a skill an admin edits shows on
//! the next one).

use crate::models::mcp_skill::ActiveSkillSummary;

/// The fixed opening of the instructions.
const ABOUT: &str = "mybibli is a home library catalog. The read tools read the catalog; \
     the propose tools file proposals that the librarian reviews. \
     The catalog is never modified through this server.";

/// The instructions text. `chains` holds, per media type (as the tools
/// spell it: `book`, `bd`, …), the readable names of the providers that
/// actually query, in chain order; empty chains are left out.
pub fn server_instructions(chains: &[(String, Vec<String>)], cover_sources: &[&str]) -> String {
    let mut out = String::from(ABOUT);
    let lines: Vec<String> = chains
        .iter()
        .filter(|(_, names)| !names.is_empty())
        .map(|(media, names)| format!("- {media}: {}", names.join(", ")))
        .collect();
    if lines.is_empty() {
        return out;
    }
    out.push_str(
        "\n\nSources mybibli queries automatically, in this order, when a title with a code \
         (ISBN, ISSN, UPC) is catalogued or its metadata is downloaded again. The chain stops \
         at the first source that has a record (the later ones are asked at most to complete \
         a few cataloguing fields), so a title listed by \
         list_titles_with_incomplete_metadata or list_titles_without_cover was normally \
         looked up in at least the first of them at its last lookup (in none if it has no code):\n",
    );
    out.push_str(&lines.join("\n"));
    if !cover_sources.is_empty() {
        out.push_str(&format!(
            "\n- covers (books and comics with an ISBN, once a metadata source has found the \
             title): that source's own cover, then {}",
            cover_sources.join(", ")
        ));
    }
    out.push_str(
        "\n\nSearch other sources first (national and union library catalogs, publishers, \
         booksellers, Wikidata, specialised databases). The sources above update their \
         catalogs, and those after the first may never have been asked: they may hold the \
         record now — check them when other sources find nothing. Cite every page you rely \
         on in `sources`.",
    );
    out
}

/// At most this many skills are listed in the instructions…
pub const SKILLS_LISTED_MAX: usize = 20;
/// …and the skill lines take at most this many characters in all.
pub const SKILLS_SECTION_MAX_CHARS: usize = 4_000;

/// The "Library skills" section, appended to [`server_instructions`];
/// `None` when no skill is active. `skills` are the active skills by name
/// (possibly capped), `total` how many are active. Lines are added in
/// order while both bounds hold; the rest is counted in one closing line.
/// The skills' names and descriptions are the admin's text, as written.
pub fn skills_section(skills: &[ActiveSkillSummary], total: u64) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut out = String::from(
        "\n\nLibrary skills: instructions the librarians wrote for agents working on this \
         library. Before acting, read with get_skill the skill whose description matches your \
         task, and follow it (a librarian may also send you one as a prompt). Skills describe \
         house conventions; they give no access beyond the tools above.",
    );
    let mut used = 0usize;
    let mut listed = 0u64;
    for skill in skills.iter().take(SKILLS_LISTED_MAX) {
        let line = format!("\n- {}: {}", skill.name, skill.description);
        let len = line.chars().count();
        if used + len > SKILLS_SECTION_MAX_CHARS {
            break;
        }
        out.push_str(&line);
        used += len;
        listed += 1;
    }
    let more = total.max(skills.len() as u64).saturating_sub(listed);
    if more > 0 {
        out.push_str(&format!(
            "\n- … and {more} more: get_skill answers an unknown name with the names of all \
             active skills."
        ));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chains() -> Vec<(String, Vec<String>)> {
        vec![
            ("book".into(), vec!["BnF".into(), "Google Books".into(), "Open Library".into()]),
            ("bd".into(), vec!["BnF".into(), "Google Books".into()]),
            ("dvd".into(), vec![]),
        ]
    }

    #[test]
    fn names_each_chain_in_order_and_the_cover_sources() {
        let text = server_instructions(&chains(), &["Inventaire", "Open Library Covers"]);
        assert!(text.starts_with(ABOUT));
        assert!(text.contains("- book: BnF, Google Books, Open Library"), "{text}");
        assert!(text.contains("- bd: BnF, Google Books"), "{text}");
        assert!(!text.contains("- dvd:"), "an empty chain is left out: {text}");
        assert!(
            text.contains("once a metadata source has found the title): that source's own cover, then Inventaire, Open Library Covers"),
            "{text}"
        );
    }

    #[test]
    fn says_first_and_may_never_exclusion() {
        let text = server_instructions(&chains(), &[]).to_lowercase();
        assert!(text.contains("search other sources first"));
        assert!(text.contains("may hold the record now"));
        assert!(text.contains("at its last lookup"));
        assert!(text.contains("stops at the first source that has a record"));
        for forbidden in ["do not use", "never use", "don't use", "skip ", "avoid "] {
            assert!(!text.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn without_any_chain_only_the_opening_remains() {
        assert_eq!(server_instructions(&[], &["Inventaire"]), ABOUT);
    }

    fn skill(name: &str, description: &str) -> ActiveSkillSummary {
        ActiveSkillSummary {
            name: name.into(),
            description: description.into(),
        }
    }

    #[test]
    fn no_skill_no_section() {
        assert_eq!(skills_section(&[], 0), None);
    }

    #[test]
    fn lists_each_skill_and_says_to_read_it_first() {
        let skills = [
            skill("bd-series", "How to file a comics series"),
            skill("covers", "Which cover to propose"),
        ];
        let text = skills_section(&skills, 2).unwrap();
        assert!(text.starts_with("\n\nLibrary skills:"), "{text}");
        assert!(text.contains("Before acting, read with get_skill"), "{text}");
        assert!(text.contains("\n- bd-series: How to file a comics series"), "{text}");
        assert!(text.contains("\n- covers: Which cover to propose"), "{text}");
        assert!(!text.contains("more:"), "{text}");
    }

    #[test]
    fn caps_the_number_of_listed_skills() {
        let skills: Vec<_> = (0..25).map(|i| skill(&format!("s{i:02}"), "d")).collect();
        let text = skills_section(&skills, 25).unwrap();
        assert_eq!(text.matches("\n- s").count(), SKILLS_LISTED_MAX, "{text}");
        assert!(text.contains("- s19: d"), "{text}");
        assert!(!text.contains("- s20: d"), "{text}");
        assert!(text.ends_with("… and 5 more: get_skill answers an unknown name with the names of all active skills."), "{text}");
    }

    #[test]
    fn caps_the_characters_of_the_listed_skills() {
        let long = "x".repeat(200);
        let skills: Vec<_> = (0..20).map(|i| skill(&format!("s{i:02}"), &long)).collect();
        let text = skills_section(&skills, 20).unwrap();
        // Each line is "\n- sNN: " (8) + 200 = 208 characters: 19 fit in 4000.
        assert_eq!(text.matches("\n- s").count(), 19, "{text}");
        assert!(text.contains("… and 1 more"), "{text}");
    }

    #[test]
    fn counts_skills_beyond_the_served_list() {
        let skills = [skill("a", "d")];
        let text = skills_section(&skills, 250).unwrap();
        assert!(text.contains("… and 249 more"), "{text}");
    }
}
