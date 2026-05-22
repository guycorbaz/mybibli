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

/// `hx-confirm=` is FORBIDDEN in all templates (post Epic 9 close).
/// The empty allowlist is the steady-state contract: any new occurrence
/// in `templates/` fails `hx_confirm_matches_allowlist` outright. The
/// audit infrastructure is preserved on purpose — re-introducing a
/// destructive flow with `hx-confirm=` requires editing this constant
/// (an explicit, reviewable act), not just adding the attribute.
const ALLOWED_HX_CONFIRM_SITES: &[(&str, usize)] = &[];

/// Issue #138 — companion allowlist for `hx-confirm=` literals emitted
/// from Rust `format!()` calls in `src/**/*.rs`. Same fail-closed
/// contract as `ALLOWED_HX_CONFIRM_SITES`: any NEW Rust-emitted
/// `hx-confirm=` must be migrated to the UX-DR8 Modal component before
/// landing.
///
/// Grandfathered entry: `src/routes/locations.rs` carries one
/// hx-confirm= in the location-tree delete-row button (inherited gap,
/// pre-existing before Story 9-12). Migration to UX-DR8 Modal is
/// tracked as a follow-up — until then, this entry locks-in the count
/// so the gap can't widen.
const ALLOWED_HX_CONFIRM_RUST_SITES: &[(&str, usize)] = &[("src/routes/locations.rs", 1)];

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

#[test]
fn hx_confirm_in_rust_strings_matches_allowlist() {
    // Issue #138: extend the `hx-confirm=` audit to Rust-emitted markup.
    // `templates_audit::hx_confirm_matches_allowlist` only walks `templates/`;
    // Rust `format!()` strings producing `hx-confirm=` previously slipped
    // through. Fail-closed contract: every Rust-emitted `hx-confirm=` MUST
    // appear in `ALLOWED_HX_CONFIRM_RUST_SITES` with an exact count.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = root.join("src");
    assert!(
        src.is_dir(),
        "src directory not found at {}",
        src.display()
    );

    // Require the attribute value to start with a letter or whitespace —
    // a real `hx-confirm="Delete…"` qualifies, but a test assertion like
    // `html.contains("hx-confirm=")` (where the next char after the opening
    // `"` is `)`) does not. The optional backslash supports both Rust
    // string literals (`hx-confirm=\"…\"`) and raw strings (`hx-confirm="…"`).
    let re = Regex::new(r#"\bhx-confirm\s*=\s*\\?"[A-Za-z ]"#).unwrap();

    let mut counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    visit(&src, &mut |path| {
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            return;
        }
        // Skip the audit itself — its source contains the literal
        // `\bhx-confirm` in the regex pattern.
        if path.file_name().and_then(|s| s.to_str()) == Some("templates_audit.rs") {
            return;
        }
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return,
        };
        let n = re.find_iter(&raw).count();
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

    for (path, actual) in &counts {
        match ALLOWED_HX_CONFIRM_RUST_SITES.iter().find(|(p, _)| *p == path) {
            Some((_, expected)) => {
                if expected != actual {
                    violations.push(format!(
                        "  {}: {} hx-confirm= literal(s), expected {}",
                        path, actual, expected
                    ));
                }
            }
            None => {
                violations.push(format!(
                    "  {}: {} hx-confirm= literal(s) — file not in allowlist",
                    path, actual
                ));
            }
        }
    }

    for (path, expected) in ALLOWED_HX_CONFIRM_RUST_SITES {
        let present = counts.contains_key(*path);
        let on_disk = root.join(path).is_file();
        if !on_disk {
            violations.push(format!(
                "  {}: allowlisted file missing from disk — remove the stale entry",
                path
            ));
        } else if !present && *expected > 0 {
            violations.push(format!(
                "  {}: expected {} hx-confirm= literal(s), found 0 — remove the stale entry",
                path, expected
            ));
        }
    }

    if !violations.is_empty() {
        let header = "hx-confirm= Rust-emitted audit failed (Issue #138):\n\
                      A new Rust `format!()` string containing `hx-confirm=` was added \
                      (use the UX-DR8 Modal component — Epic 9 — not `hx-confirm=`), \
                      or a migration removed one; in either case update \
                      `ALLOWED_HX_CONFIRM_RUST_SITES` in the same PR.\n";
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
    // Must match BOTH `name="_csrf_token"` AND a `value="{{ csrf_token …"`
    // binding. Without the value check, a template regression that ships
    // `<input name="_csrf_token" value="">` or a hardcoded literal would
    // pass the audit while leaving every POST 403'd at runtime.
    //
    // Accepts either attribute order (`name` before `value` or vice
    // versa) and either `{{` or `{{-` (whitespace-control) template
    // delimiters.
    let csrf_token_input = Regex::new(
        r#"(?is)<input\b(?:[^>]*\bname\s*=\s*["']_csrf_token["'][^>]*\bvalue\s*=\s*["']\{\{-?\s*csrf_token\b|[^>]*\bvalue\s*=\s*["']\{\{-?\s*csrf_token\b[^>]*\bname\s*=\s*["']_csrf_token["'])"#,
    )
    .unwrap();
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
            // #42 — strict first-child placement. The FIRST <input> after
            // <form method="POST"> must be _csrf_token. Anything else
            // (hidden version, plain text input, etc.) coming before
            // _csrf_token is a violation: a future refactor that moves
            // the token mid-form would silently relax the contract
            // otherwise. Inspect ONLY the first input we encounter.
            let Some(first_m) = any_input.find(body) else {
                // No input in this form. Pure-button POST form with
                // no body fields — already caught above as missing
                // CSRF, would not reach here.
                continue;
            };
            let abs_end = first_m.end();
            let tag_end_rel = body[abs_end..].find('>').unwrap_or(0);
            let first_attrs = &body[first_m.start()..abs_end + tag_end_rel];
            if !csrf_token_input.is_match(first_attrs) {
                let line = 1 + content[..open.start()].matches('\n').count();
                let rel = path.strip_prefix(&root).unwrap_or(path);
                let rel_str = rel
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                violations.push(format!(
                    "  {}:{} — first <input> in this POST form is NOT `_csrf_token` (strict first-child rule, #42)",
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

/// #325 — guards against the v1.7.2 regression where a page-template
/// referenced `hx-target="#feedback-list"` without declaring the matching
/// `<div id="feedback-list">` slot. HTMX raises `htmx:targetError` on
/// submission and the action silently fails from the user's POV.
///
/// Scoped to `templates/pages/*.html`. Components/fragments are exempt:
/// they're rendered INTO a page that owns the slot.
#[test]
fn pages_using_feedback_list_target_declare_slot() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let pages_dir = root.join("templates").join("pages");

    let target_re =
        Regex::new(r#"(?is)hx-target\s*=\s*["']#feedback-list["']"#).unwrap();
    let slot_re = Regex::new(r#"(?is)\bid\s*=\s*["']feedback-list["']"#).unwrap();

    let mut violations: Vec<String> = Vec::new();
    visit(&pages_dir, &mut |path| {
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            return;
        }
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return,
        };
        let content = strip_html_comments(&raw);
        if !target_re.is_match(&content) {
            return;
        }
        if slot_re.is_match(&content) {
            return;
        }
        let rel = path.strip_prefix(&root).unwrap_or(path);
        let rel_str = rel
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        violations.push(format!(
            "  {} — references hx-target=\"#feedback-list\" but does not declare \
             id=\"feedback-list\"",
            rel_str
        ));
    });

    if !violations.is_empty() {
        let header = "feedback-list slot audit failed (#325):\n\
                      Every page template that uses hx-target=\"#feedback-list\" \
                      (directly or via a button/form on that page) must declare \
                      `<div id=\"feedback-list\">` somewhere in the page. \
                      Without the slot, HTMX raises htmx:targetError on submission \
                      and the action silently fails.\n";
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
