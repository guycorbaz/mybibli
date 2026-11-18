//! #695 — every directory `static/css/input.css` names as a Tailwind source
//! is copied into the Dockerfile `css` stage, and `src/` is one of them.
//!
//! A local `npx @tailwindcss/cli` run from the repo root finds class strings
//! anywhere in the project through automatic source detection; the Docker
//! `css` stage only sees what it copies. A source missing there builds an
//! image whose `output.css` lacks the classes only that source uses — the
//! `/locations` tree controls stayed at `opacity-0` because
//! `group-hover:opacity-100` lived only in `src/routes/locations.rs`.

mod common;

use common::repo::read;

/// The positive `@source "…"` paths of input.css, resolved against
/// `static/css/` to repository-relative directories (`../js` → `static/js`).
fn tailwind_sources(css: &str) -> Vec<String> {
    css.lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("@source \""))
        .filter_map(|rest| rest.split('"').next())
        .map(|rel| {
            let mut parts = vec!["static", "css"];
            for seg in rel.split('/') {
                match seg {
                    ".." => {
                        parts.pop();
                    }
                    "." | "" => {}
                    s => parts.push(s),
                }
            }
            parts.join("/")
        })
        .collect()
}

/// The `COPY <src> ./<dst>` sources of the Dockerfile stage named `css`.
fn css_stage_copies(dockerfile: &str) -> Vec<String> {
    let mut in_css = false;
    let mut out = Vec::new();
    for line in dockerfile.lines().map(str::trim) {
        if line.starts_with("FROM ") {
            in_css = line.ends_with(" AS css") || line.ends_with(" as css");
            continue;
        }
        if in_css && let Some(rest) = line.strip_prefix("COPY ") {
            for src in rest.split_whitespace().rev().skip(1) {
                out.push(src.trim_end_matches('/').to_string());
            }
        }
    }
    out
}

#[test]
fn input_css_sources_rust_html() {
    let sources = tailwind_sources(&read("static/css/input.css"));
    assert!(sources.contains(&"src".to_string()), "input.css must @source src/: {sources:?}");
}

#[test]
fn every_tailwind_source_is_copied_into_the_css_stage() {
    let sources = tailwind_sources(&read("static/css/input.css"));
    let copies = css_stage_copies(&read("Dockerfile"));
    assert!(!copies.is_empty(), "no `FROM … AS css` stage found in the Dockerfile");
    for s in &sources {
        assert!(copies.contains(s), "Tailwind source {s} is not copied into the css stage: {copies:?}");
    }
}

#[test]
fn source_resolution_and_stage_parsing() {
    assert_eq!(
        tailwind_sources("@source \"../js\";\n@source not \"../js/vendor\";\n@source \"../../src\";"),
        vec!["static/js".to_string(), "src".to_string()]
    );
    let df = "FROM rust AS builder\nCOPY src/ ./src/\nFROM node AS css\nCOPY templates/ ./templates/\nCOPY a b ./\nFROM alpine\nCOPY x/ ./x/";
    assert_eq!(css_stage_copies(df), vec!["templates", "b", "a"]);
}
