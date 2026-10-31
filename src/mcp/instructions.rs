//! Story 12-3 (#560, FR150) — the server instructions an MCP client
//! receives at `initialize`: what the server is, and which sources
//! mybibli already queries automatically, so an agent looks elsewhere
//! first. Those sources update their catalogs: they are a lower
//! priority for the agent, never an exclusion (decision of 2026-10-30).
//!
//! Pure; `transport.rs` feeds it the live registry and the cover
//! fallbacks on every `initialize` (a provider key set by an admin shows
//! on the next one).

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
        "\n\nSources mybibli already queries automatically, in this order, when a title is \
         catalogued or its metadata is downloaded again — a title listed by \
         list_titles_with_incomplete_metadata or list_titles_without_cover went through them \
         without a usable result at its last lookup:\n",
    );
    out.push_str(&lines.join("\n"));
    if !cover_sources.is_empty() {
        out.push_str(&format!(
            "\n- covers: the metadata source's own cover, then {}",
            cover_sources.join(", ")
        ));
    }
    out.push_str(
        "\n\nSearch other sources first (national and union library catalogs, publishers, \
         booksellers, Wikidata, specialised databases). The sources above update their \
         catalogs, so they may hold the record now: check them again when other sources \
         find nothing. Cite every page you rely on in `sources`.",
    );
    out
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
            text.contains("- covers: the metadata source's own cover, then Inventaire, Open Library Covers"),
            "{text}"
        );
    }

    #[test]
    fn says_first_and_may_never_exclusion() {
        let text = server_instructions(&chains(), &[]).to_lowercase();
        assert!(text.contains("search other sources first"));
        assert!(text.contains("may hold the record now"));
        assert!(text.contains("at its last lookup"));
        for forbidden in ["do not use", "never use", "don't use", "skip ", "avoid "] {
            assert!(!text.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn without_any_chain_only_the_opening_remains() {
        assert_eq!(server_instructions(&[], &["Inventaire"]), ABOUT);
    }
}
