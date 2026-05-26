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
    //
    // Fix #24 (v1.7.9) sub-item 5: the `attrs` group walks attribute
    // chars as either a non-special char, a complete double-quoted
    // value, or a complete single-quoted value. Without this, a
    // literal `>` inside a quoted attr (e.g. `<script attr="a > b">`)
    // would terminate the match prematurely and the regex would
    // re-anchor on the next `<script>` token. No current template
    // hits this, but the regex now matches the HTML spec.
    let inline_script = Regex::new(
        r#"<script\b(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>(?P<body>\s*\S[\s\S]*?)</script>"#,
    )
    .unwrap();
    let script_src_or_safe_type = Regex::new(
        r#"\bsrc\s*=|\btype\s*=\s*"(application/json|application/ld\+json|text/x-template)""#,
    )
    .unwrap();
    // Same quote-aware attrs grammar as inline_script (sub-item 5).
    let style_block = Regex::new(r#"<style\b(?:[^>"']|"[^"]*"|'[^']*')*>"#).unwrap();
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
        // the regexes. Then mask SVG inner content so an inline
        // `<svg><style>...</style></svg>` (sub-item 4) doesn't false-
        // positive the bare-`<style>` check. Whitespace replacement keeps
        // line numbers aligned with the original file in both passes.
        let content = strip_svg_inner_blocks(&strip_html_comments(&raw));

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

/// Replace every `<!-- ... -->` block with same-width whitespace
/// (preserving newlines), so the rest of the audit scans only live markup
/// while line-number reporting still maps back to the original file.
///
/// Fix #24 (v1.7.9):
///   * Unterminated `<!--` no longer consumes the rest of the file —
///     pre-fix any inline-markup violation past an accidentally-truncated
///     comment was invisible to the audit.
///   * Operates on `&str` slices instead of byte-by-byte `as char`, so
///     accented characters in the failure-report snippets render
///     correctly instead of producing `Ã©`-style mojibake.
fn strip_html_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 4..];
        match after_open.find("-->") {
            Some(end) => {
                let span = &rest[start..start + 4 + end + 3];
                for ch in span.chars() {
                    out.push(if ch == '\n' { '\n' } else { ' ' });
                }
                rest = &rest[start + 4 + end + 3..];
            }
            None => {
                // Unterminated comment: keep the rest verbatim. The
                // template will fail to render anyway, but the audit
                // must not silently swallow later violations on top.
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Replace every `<svg ...>...</svg>` block's inner content with same-width
/// whitespace so an inline `<svg><style>...</style></svg>` does not false-
/// positive the bare-`<style>` check. The opening `<svg>` tag itself is
/// preserved unchanged so its attributes (`class="..."` etc.) still scan.
/// Fix #24 (v1.7.9) sub-item 4 — defensive; no current SVG ships inline
/// `<style>`, but a future contributor pasting one in would land a CSP
/// hole that the audit silently misses.
fn strip_svg_inner_blocks(input: &str) -> String {
    let open_re = Regex::new(r"<svg\b[^>]*>").unwrap();
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(m) = open_re.find(rest) {
        out.push_str(&rest[..m.end()]);
        let after_open = &rest[m.end()..];
        match after_open.find("</svg>") {
            Some(end) => {
                for ch in after_open[..end].chars() {
                    out.push(if ch == '\n' { '\n' } else { ' ' });
                }
                out.push_str("</svg>");
                rest = &after_open[end + 6..];
            }
            None => {
                out.push_str(after_open);
                return out;
            }
        }
    }
    out.push_str(rest);
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

/// #48 — companion to `forms_include_csrf_token`: scans `src/**/*.rs`
/// for `<form method="POST">` HTML literals emitted from Rust code
/// (`format!()`, `push_str()`, etc.) and panics if the same string
/// region does not also contain `name="_csrf_token"` nearby. The
/// runtime CSRF middleware would 403 a submission from such a form,
/// but without this audit the regression surfaces only at user
/// runtime — long after the offending change merged.
///
/// Skips `templates_audit.rs` itself (its own regex contains the
/// literal `<form method=…>` for documentation/audit purposes).
#[test]
fn rust_emitted_post_forms_include_csrf_token() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src = root.join("src");
    assert!(
        src.is_dir(),
        "src directory not found at {}",
        src.display()
    );

    // Match `<form` followed by attrs containing `method="POST"` in
    // any quoting style (Rust escaped `\"POST\"`, raw `"POST"`, single
    // quotes). Case-insensitive.
    let form_post = Regex::new(
        r#"(?is)<form\b[^>]*\bmethod\s*=\s*\\?["']post\\?["'][^>]*>"#,
    )
    .unwrap();
    // Window — give the form ~1500 bytes to declare its CSRF hidden
    // input. Real Rust-emitted forms are short (locations.rs:301 is
    // ~800 bytes), so 1500 is a comfortable margin without crossing
    // into unrelated literal regions.
    let csrf_input = Regex::new(
        r#"(?is)\bname\s*=\s*\\?["']_csrf_token\\?["']"#,
    )
    .unwrap();

    let mut violations: Vec<String> = Vec::new();
    visit(&src, &mut |path| {
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            return;
        }
        // Skip the audit module itself — its source contains a
        // documentation reference to `<form method="POST">`.
        if path.file_name().and_then(|s| s.to_str()) == Some("templates_audit.rs") {
            return;
        }
        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return,
        };
        for m in form_post.find_iter(&raw) {
            // Skip matches that live in a Rust line comment (`//`).
            // Doc comments (`///`, `//!`) frequently reference the
            // `<form method="POST">` shape in module-level docs and
            // would otherwise produce false positives.
            let line_start = raw[..m.start()].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line_prefix = &raw[line_start..m.start()];
            if line_prefix.trim_start().starts_with("//") {
                continue;
            }
            let window_end = (m.end() + 1500).min(raw.len());
            let window = &raw[m.start()..window_end];
            if csrf_input.is_match(window) {
                continue;
            }
            let line = 1 + raw[..m.start()].matches('\n').count();
            let rel = path.strip_prefix(&root).unwrap_or(path);
            let rel_str = rel
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            violations.push(format!(
                "  {}:{} — Rust-emitted <form method=\"POST\"> without `name=\"_csrf_token\"` within the next 1500 bytes",
                rel_str, line
            ));
        }
    });

    if !violations.is_empty() {
        let header = "Rust-emitted CSRF form-input audit failed (#48):\n\
                      Every `<form method=\"POST\">` literal in src/**/*.rs must \
                      declare `<input type=\"hidden\" name=\"_csrf_token\" value=\"…\">` \
                      within the same string region. Without it, the global CSRF \
                      middleware rejects the submission with 403 at user runtime.\n";
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

// ─── Fix #24 (v1.7.9) — strip_html_comments + SVG masking ─────────

#[test]
fn strip_html_comments_replaces_closed_blocks_with_whitespace() {
    let input = "a<!-- skip -->b\n<!-- also -->c";
    let out = strip_html_comments(input);
    // Each comment span becomes spaces (newlines preserved). Live chars
    // outside the comments survive verbatim.
    assert!(out.starts_with('a'));
    assert!(out.contains('b'));
    assert!(out.ends_with('c'));
    assert!(
        !out.contains("skip") && !out.contains("also"),
        "comment bodies must be masked"
    );
}

#[test]
fn strip_html_comments_preserves_line_count() {
    let input = "line1\n<!--\n still in comment\n-->line4";
    let out = strip_html_comments(input);
    // Output must carry the same 4 line breaks as the input so error
    // reports point at the right source line.
    assert_eq!(input.matches('\n').count(), out.matches('\n').count());
}

#[test]
fn strip_html_comments_does_not_consume_rest_on_unterminated_open() {
    // Regression for fix #24 sub-item 1: pre-fix this input would have
    // had the trailing `<script>` swallowed into whitespace because the
    // comment loop walked to end-of-file looking for `-->` and then
    // erased the whole tail.
    let input = "<!-- bad comment\n<script>alert(1)</script>";
    let out = strip_html_comments(input);
    assert!(
        out.contains("<script>"),
        "unterminated comment must NOT consume the rest of the file: {out:?}"
    );
}

#[test]
fn strip_html_comments_preserves_utf8_outside_comments() {
    // Regression for fix #24 sub-item 2: pre-fix the byte-by-byte
    // `as char` cast turned `é` (UTF-8 0xC3 0xA9) into `Ã©`. The new
    // implementation slices the source `&str` so the snippet stays
    // readable in the failure report.
    let input = "déjà <!-- skip --> vu";
    let out = strip_html_comments(input);
    assert!(out.starts_with("déjà"));
    assert!(out.ends_with(" vu"));
}

#[test]
fn strip_svg_inner_blocks_masks_inline_style_inside_svg() {
    let input = "<svg viewBox=\"0 0 1 1\"><style>g { fill: red }</style></svg>";
    let out = strip_svg_inner_blocks(input);
    // The opening tag survives (so its attributes still scan) but the
    // inner `<style>` is whitespace-erased, so the bare-`<style>` check
    // above won't false-positive on this fragment.
    assert!(out.starts_with("<svg viewBox=\"0 0 1 1\">"));
    assert!(out.ends_with("</svg>"));
    assert!(!out.contains("<style>"));
}

#[test]
fn strip_svg_inner_blocks_passes_through_when_no_svg() {
    let input = "<div>plain</div>";
    assert_eq!(strip_svg_inner_blocks(input), input);
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
