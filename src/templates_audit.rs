//! CSP regression gate — fails the build if any inline `<script>`,
//! `<style>`, `style="..."`, or inline event-handler attribute appears
//! in the `templates/` tree.
//!
//! Story 7.4 / NFR15 / AR16 — strict CSP (`script-src 'self'`,
//! `style-src 'self'`) blocks every form of inline executable / inline
//! style. Eyeballing the diff is not a reliable gate; this test is.
//!
//! Allowances:
//! - `<script src="...">` — external script, fine under `script-src 'self'`.
//! - `<script type="application/json">` / `application/ld+json` /
//!   `text/x-template` — non-executable data islands, not blocked by CSP.
//! - Empty `<script></script>` (whitespace only) — no executable body.
//!
//! The test scopes its walk to the project's `templates/` directory and
//! ignores anything else (e.g. `_bmad-output/` notes that may quote
//! template snippets in markdown).
//!
//! Story 7.5 — a fifth regex freezes the `hx-confirm=` attribute at the
//! five grandfathered sites. Any new occurrence must route through the
//! UX-DR8 Modal component (Epic 9) so it automatically inherits scanner
//! burst protection via `scanner-guard.js`.

#![cfg(test)]

use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

/// Grandfathered `hx-confirm=` sites — the only templates allowed to carry
/// this attribute. The count is the exact expected number of occurrences
/// per file; a mismatch (new destructive button, or an Epic-9 migration
/// removing one) forces the PR to update this list, which is the whole
/// point of the audit: a reviewer is always in the loop.
const ALLOWED_HX_CONFIRM_SITES: &[(&str, usize)] = &[
    ("templates/pages/loans.html", 1),
    ("templates/pages/borrower_detail.html", 2),
    ("templates/pages/contributor_detail.html", 1),
    ("templates/pages/series_detail.html", 1),
];

#[test]
fn no_inline_markup_in_templates() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
    assert!(
        root.is_dir(),
        "templates directory not found at {}",
        root.display()
    );

    // Inline event handler attribute, anchored on a word boundary so unrelated
    // tokens like `python-on-foo=` or `iron-on=` don't match.
    let handler = Regex::new(
        r#"\bon(click|change|submit|focus|blur|input|key(down|up|press))\s*=\s*""#,
    )
    .unwrap();
    // Inline executable <script> block. Allow:
    //   - src="..."   → external script
    //   - type="application/json" / "application/ld+json" / "text/x-template" → data island, not executed
    //   - empty / whitespace-only block → no body to execute
    // Requires at least one non-whitespace char after the opening tag.
    let inline_script = Regex::new(
        r#"<script\b(?P<attrs>[^>]*)>(?P<body>\s*\S[\s\S]*?)</script>"#,
    )
    .unwrap();
    let script_src_or_safe_type = Regex::new(
        r#"\bsrc\s*=|\btype\s*=\s*"(application/json|application/ld\+json|text/x-template)""#,
    )
    .unwrap();
    let style_block = Regex::new(r#"<style\b[^>]*>"#).unwrap();
    let style_attr = Regex::new(r#"\bstyle\s*=\s*""#).unwrap();

    let mut violations: Vec<(PathBuf, usize, &'static str, String)> = Vec::new();
    visit(&root, &mut |path| {
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            return;
        }
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return,
        };
        // Strip HTML comments before scanning so prose mentions of
        // `<style>` / `onclick=` / etc. inside `<!-- ... -->` don't trip
        // the regexes. Whitespace replacement keeps line numbers aligned
        // with the original file.
        let content = strip_html_comments(&raw);

        // Inline scripts: regex spans multiple lines, so we map match
        // start offset → 1-indexed line number for reporting.
        for m in inline_script.captures_iter(&content) {
            let attrs = m.name("attrs").map(|x| x.as_str()).unwrap_or("");
            // Skip scripts with src="..." or whitelisted type="..." (data islands).
            if script_src_or_safe_type.is_match(attrs) {
                continue;
            }
            let pos = m.get(0).unwrap().start();
            let line = 1 + content[..pos].matches('\n').count();
            let snippet = m
                .get(0)
                .unwrap()
                .as_str()
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            violations.push((path.to_path_buf(), line, "inline <script>", snippet));
        }

        for (line_idx, line) in content.lines().enumerate() {
            let line_no = line_idx + 1;
            if handler.is_match(line) {
                violations.push((
                    path.to_path_buf(),
                    line_no,
                    "inline event handler",
                    line.trim().to_string(),
                ));
            }
            if style_block.is_match(line) {
                violations.push((
                    path.to_path_buf(),
                    line_no,
                    "inline <style> block",
                    line.trim().to_string(),
                ));
            }
            if style_attr.is_match(line) {
                violations.push((
                    path.to_path_buf(),
                    line_no,
                    "inline style= attribute",
                    line.trim().to_string(),
                ));
            }
        }
    });

    if !violations.is_empty() {
        let mut report = String::from(
            "CSP-blocking inline markup found in templates/ — refactor required:\n",
        );
        for (path, line, kind, snippet) in &violations {
            let rel = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(path);
            report.push_str(&format!(
                "  {}:{} [{}] {}\n",
                rel.display(),
                line,
                kind,
                snippet
            ));
        }
        panic!("{report}");
    }
}

/// Replace every `<!-- ... -->` block with same-length whitespace
/// (preserving newlines), so the rest of the audit scans only live markup
/// while line-number reporting still maps back to the original file.
fn strip_html_comments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"<!--" {
            let mut j = i + 4;
            while j + 3 <= bytes.len() && &bytes[j..j + 3] != b"-->" {
                j += 1;
            }
            // Replace comment span (including delimiters) with whitespace,
            // preserving any newlines inside it.
            let end = (j + 3).min(bytes.len());
            for &b in bytes.iter().take(end).skip(i) {
                out.push(if b == b'\n' { '\n' } else { ' ' });
            }
            i = end;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn visit(dir: &Path, f: &mut impl FnMut(&Path)) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, f);
        } else {
            f(&path);
        }
    }
}

#[test]
fn hx_confirm_matches_allowlist() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let templates = root.join("templates");
    assert!(
        templates.is_dir(),
        "templates directory not found at {}",
        templates.display()
    );

    let re = Regex::new(r#"\bhx-confirm\s*=\s*""#).unwrap();

    // Grouped count of `hx-confirm=` occurrences per path (relative to
    // repo root, using forward slashes so the allowlist entries match
    // verbatim on Linux and on Windows).
    let mut counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    visit(&templates, &mut |path| {
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            return;
        }
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return,
        };
        let content = strip_html_comments(&raw);
        let n = re.find_iter(&content).count();
        if n == 0 {
            return;
        }
        let rel = path.strip_prefix(&root).unwrap_or(path);
        let rel_str = rel
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        counts.insert(rel_str, n);
    });

    let mut violations: Vec<String> = Vec::new();

    // (a) new file with hx-confirm=, OR (b) allowlisted file's count differs.
    for (path, actual) in &counts {
        match ALLOWED_HX_CONFIRM_SITES.iter().find(|(p, _)| *p == path) {
            Some((_, expected)) => {
                if expected != actual {
                    violations.push(format!(
                        "  {}: {} hx-confirm= attribute(s), expected {}",
                        path, actual, expected
                    ));
                }
            }
            None => {
                violations.push(format!(
                    "  {}: {} hx-confirm= attribute(s) — file not in allowlist",
                    path, actual
                ));
            }
        }
    }

    // (c) stale allowlist entry — allowlisted file no longer exists or now
    // has zero occurrences.
    for (path, expected) in ALLOWED_HX_CONFIRM_SITES {
        let present = counts.contains_key(*path);
        let on_disk = root.join(path).is_file();
        if !on_disk {
            violations.push(format!(
                "  {}: allowlisted file missing from disk — remove the stale entry",
                path
            ));
        } else if !present && *expected > 0 {
            violations.push(format!(
                "  {}: expected {} hx-confirm= attribute(s), found 0 — remove the stale entry",
                path, expected
            ));
        }
    }

    if !violations.is_empty() {
        let header = "hx-confirm= audit failed (Story 7.5):\n\
                      A count change in a grandfathered file means either a new destructive \
                      button was added (use the UX-DR8 Modal component — Epic 9 — not \
                      `hx-confirm=`), or an Epic-9 migration removed one; in either case \
                      update `ALLOWED_HX_CONFIRM_SITES` in the same PR.\n";
        let report = format!("{}{}", header, violations.join("\n"));
        panic!("{report}");
    }
}

// ─── Story 8-2 — CSRF audit guards ─────────────────────────────────
//
// Two gates, one per audit target:
//   - `forms_include_csrf_token`: every `<form method="POST">` in
//     `templates/` must have `<input … name="_csrf_token" …>` as one of
//     its first inputs.
//   - `csrf_exempt_routes_frozen`: the only `CSRF_EXEMPT_ROUTES` entry
//     is `("POST", "/login")`. Adding a new exempt route requires a
//     visible edit to this constant — the PR cannot sneak past review.

#[test]
fn forms_include_csrf_token() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let templates = root.join("templates");

    // Match the opening <form> tag together with the text following it so
    // we can inspect the first few inputs inline. `(?s)` enables dot-matches-newline.
    // Accept `method="POST"`, `method='POST'`, or unquoted `method=POST`.
    // Strict bare-word form `method=post\b` avoids matching `method=post-junk`.
    let form_open = Regex::new(
        r#"(?is)<form\b[^>]*\bmethod\s*=\s*(?:["']post["']|post\b)[^>]*>"#,
    )
    .unwrap();
    let csrf_token_input =
        Regex::new(r#"(?is)<input\b[^>]*\bname\s*=\s*["']_csrf_token["']"#).unwrap();
    let any_input = Regex::new(r#"(?is)<input\b"#).unwrap();
    let form_close = Regex::new(r#"(?is)</form>"#).unwrap();

    let mut violations: Vec<String> = Vec::new();
    visit(&templates, &mut |path| {
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            return;
        }
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return,
        };
        let content = strip_html_comments(&raw);

        for open in form_open.find_iter(&content) {
            let after = &content[open.end()..];
            // Form body = everything up to the closing </form>. If no
            // close tag is found, fall back to the whole tail — that
            // still exercises the CSRF-input presence check.
            let body_end = form_close
                .find(after)
                .map(|m| m.start())
                .unwrap_or(after.len());
            let body = &after[..body_end];

            let has_csrf = csrf_token_input.is_match(body);
            if !has_csrf {
                let line = 1 + content[..open.start()].matches('\n').count();
                let rel = path.strip_prefix(&root).unwrap_or(path);
                let rel_str = rel
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                violations.push(format!("  {}:{} — POST form without `_csrf_token` hidden input", rel_str, line));
                continue;
            }
            // Bonus: make sure _csrf_token is near the top of the
            // form (within the first ~5 inputs) so the audit stays
            // strict. Walk at most ~500 chars / 5 inputs of body.
            let mut seen_inputs = 0usize;
            let mut found_at: Option<usize> = None;
            let mut cursor = 0usize;
            while cursor < body.len().min(2000) && seen_inputs < 5 {
                let rest = &body[cursor..];
                let Some(m) = any_input.find(rest) else { break };
                seen_inputs += 1;
                let abs_start = cursor + m.start();
                let abs_end = cursor + m.end();
                // Read to the next `>` to inspect this input's attrs.
                let tag_end_rel = body[abs_end..].find('>').unwrap_or(0);
                let attrs = &body[abs_start..abs_end + tag_end_rel];
                if csrf_token_input.is_match(attrs) {
                    found_at = Some(seen_inputs);
                    break;
                }
                cursor = abs_end + tag_end_rel + 1;
            }
            if found_at.is_none() {
                let line = 1 + content[..open.start()].matches('\n').count();
                let rel = path.strip_prefix(&root).unwrap_or(path);
                let rel_str = rel
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                violations.push(format!(
                    "  {}:{} — `_csrf_token` input not among the first 5 inputs of this POST form",
                    rel_str, line
                ));
            }
        }
    });

    if !violations.is_empty() {
        let header = "CSRF form-input audit failed (Story 8-2):\n\
                      Every `<form method=\"POST\">` in templates/ must include \
                      `<input type=\"hidden\" name=\"_csrf_token\" value=\"{{ csrf_token|e }}\">` \
                      as one of its first children. Without it, the global CSRF \
                      middleware rejects the submission with 403.\n";
        let report = format!("{}{}", header, violations.join("\n"));
        panic!("{report}");
    }
}

#[test]
fn csrf_exempt_routes_frozen() {
    use crate::middleware::csrf::CSRF_EXEMPT_ROUTES;
    // Full-slice equality: any addition, removal, reorder, or edit of an
    // exempt entry fails the assertion. Len-only + index-0 checks let a
    // second entry sneak in with a one-line len update.
    let expected: &[(&str, &str)] = &[("POST", "/login")];
    assert_eq!(
        CSRF_EXEMPT_ROUTES, expected,
        "CSRF exempt-route allowlist changed — this is a review signal. \
         If adding a new exempt route is genuinely required, update this \
         expected list in the same PR and justify in the review description."
    );
}
