//! #695 — every directory `static/css/input.css` names as a Tailwind source
//! is copied into the Dockerfile `css` stage, and `src/` is one of them.
//!
//! A local `npx @tailwindcss/cli` run from the repo root finds class strings
//! anywhere in the project through automatic source detection; the Docker
//! `css` stage only sees what it copies. A source missing there builds an
//! image whose `output.css` lacks the classes only that source uses — the
//! `/locations` tree controls stayed at `opacity-0` because
//! `group-hover:opacity-100` lived only in `src/routes/locations.rs`.

// Only the file reader: `mod common;` would compile the MCP / web helpers
// and link the whole crate into a test that reads text files.
#[path = "common/repo.rs"]
mod repo;

use repo::read;

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

/// The paths the shell-form `COPY` lines of the Dockerfile stage named `css`
/// put **at the same place** in the stage as in the repository — what
/// Tailwind, run from the stage's working directory, can read under its
/// repository-relative `@source` paths. `COPY src/ ./src/` counts as `src`;
/// `COPY src/ ./rust/` counts for nothing; `COPY . .` as `.` (options such
/// as `--chown=` skipped, trailing `/` and leading `./` removed).
fn css_stage_copies(dockerfile: &str) -> Vec<String> {
    let mut in_css = false;
    let mut out = Vec::new();
    for line in dockerfile.lines().map(str::trim) {
        if line.starts_with("FROM ") {
            in_css = line.ends_with(" AS css") || line.ends_with(" as css");
            continue;
        }
        if in_css && let Some(rest) = line.strip_prefix("COPY ") {
            let args: Vec<String> = rest
                .split_whitespace()
                .filter(|a| !a.starts_with("--"))
                .map(normalize)
                .collect();
            if let Some((dst, srcs)) = args.split_last() {
                out.extend(srcs.iter().filter(|s| *s == dst).cloned());
            }
        }
    }
    out
}

fn normalize(path: &str) -> String {
    let p = path.trim_end_matches('/');
    let p = p.strip_prefix("./").unwrap_or(p);
    if p.is_empty() || p == "." { ".".to_string() } else { p.to_string() }
}

/// Whether a source path is inside one of the copied paths.
fn is_copied(source: &str, copies: &[String]) -> bool {
    copies
        .iter()
        .any(|c| c == "." || source == c || source.starts_with(&format!("{c}/")))
}

#[test]
fn input_css_declares_every_source_and_detects_none() {
    let css = read("static/css/input.css");
    assert!(
        css.contains(r#"@import "tailwindcss" source(none);"#),
        "automatic source detection must stay off, or a local build sees more than the image"
    );
    let sources = tailwind_sources(&css);
    for needed in ["templates", "static/js", "src"] {
        assert!(sources.contains(&needed.to_string()), "input.css must @source {needed}: {sources:?}");
    }
}

#[test]
fn every_tailwind_source_is_copied_into_the_css_stage() {
    let sources = tailwind_sources(&read("static/css/input.css"));
    let copies = css_stage_copies(&read("Dockerfile"));
    assert!(!copies.is_empty(), "no `FROM … AS css` stage found in the Dockerfile");
    for s in &sources {
        assert!(is_copied(s, &copies), "Tailwind source {s} is not copied into the css stage: {copies:?}");
    }
}

#[test]
fn source_resolution_and_stage_parsing() {
    assert_eq!(
        tailwind_sources("@source \"../js\";\n@source not \"../js/vendor\";\n@source \"../../src\";"),
        vec!["static/js".to_string(), "src".to_string()]
    );
    let df = "FROM rust AS builder\nCOPY src/ ./src/\nFROM node AS css\nCOPY templates/ ./templates/\nCOPY --chown=node static/js/ ./static/js/\nCOPY src/ ./rust/\nCOPY package.json package-lock.json ./\nFROM alpine\nCOPY x/ ./x/";
    assert_eq!(css_stage_copies(df), vec!["templates", "static/js"]);
    assert_eq!(css_stage_copies("FROM node AS css\nCOPY . ."), vec!["."]);
    let copies = vec!["static/js".to_string(), "src".to_string()];
    assert!(is_copied("static/js/foo.js", &copies));
    assert!(is_copied("src", &copies));
    assert!(!is_copied("srcx", &copies));
    assert!(!is_copied("templates", &copies));
    assert!(is_copied("anything", &[".".to_string()]));
}
