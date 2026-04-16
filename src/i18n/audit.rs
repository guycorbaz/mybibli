//! Compile-time-ish audit that every `t!("key", ...)` call site in `src/`
//! has a matching leaf in BOTH `locales/en.yml` and `locales/fr.yml`.
//!
//! Runs under `cargo test`. A pure-bash/grep equivalent was rejected: YAML
//! is nested (`nav.catalog` → `nav:\n  catalog:`), and a grep-only scan
//! silently misses leaves in sub-maps.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Walk `dir` recursively and return every `.rs` file path found.
    /// Panics on `read_dir` errors so a permission mishap surfaces as an audit
    /// failure instead of silently scanning a subset of the tree.
    fn rust_files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let read = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("audit: read_dir failed on {}: {e}", dir.display()));
        for entry in read {
            let entry = entry
                .unwrap_or_else(|e| panic!("audit: entry iter failed under {}: {e}", dir.display()));
            let path = entry.path();
            if path.is_dir() {
                out.extend(rust_files(&path));
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(path);
            }
        }
        out
    }

    /// Extract all first-argument string literals from `t!(...)` calls in `src`.
    /// Matches `t!\s*\(\s*"..."` and returns the captured keys.
    ///
    /// Convention (audit-side): `t!("...")` must not appear inside Rust string
    /// literals, block comments (`/* ... */`), or raw strings — the scanner
    /// is byte-oriented and cannot track those contexts. Line comments (`//`,
    /// `///`, `//!`) and `/* ... */` blocks ARE stripped; illustrative
    /// examples live safely in docs. Writers who need a literal `t!(...)`
    /// inside a `&str` should break it up or use a different form.
    fn extract_t_keys(src: &str) -> Vec<String> {
        // Strip /* ... */ block comments first (simple single-pass scanner —
        // does not support nested /* */ but the codebase does not use them).
        let without_blocks = strip_block_comments(src);
        // Then strip line comments.
        let stripped: String = without_blocks
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    ""
                } else {
                    line
                }
            })
            .collect::<Vec<&str>>()
            .join("\n");
        let bytes = stripped.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i + 2 < bytes.len() {
            // Look for `t!` — bare `t` preceded by a non-identifier char (or start).
            if bytes[i] == b't' && bytes[i + 1] == b'!' {
                // `rust_i18n::t!(...)` is valid — `::` is the path separator, not
                // part of an identifier. Only reject when `t` is preceded by an
                // identifier character ([a-zA-Z0-9_]).
                let prev_ok = i == 0 || {
                    let c = bytes[i - 1];
                    !c.is_ascii_alphanumeric() && c != b'_'
                };
                if prev_ok {
                    // Skip `t!`, then optional whitespace, then `(`.
                    let mut j = i + 2;
                    while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'(' {
                        // Skip optional whitespace after `(`.
                        j += 1;
                        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                            j += 1;
                        }
                        if j < bytes.len() && bytes[j] == b'"' {
                            j += 1;
                            let start = j;
                            while j < bytes.len() && bytes[j] != b'"' {
                                // Skip escapes — keys never contain them in practice,
                                // but guard against `\"`.
                                if bytes[j] == b'\\' && j + 1 < bytes.len() {
                                    j += 2;
                                    continue;
                                }
                                j += 1;
                            }
                            if j < bytes.len() && bytes[j] == b'"' {
                                let key = &stripped[start..j];
                                // Filter out non-key string args (interpolation targets
                                // never sit in the first slot, but a literal like
                                // `t!("…", entity = "title")` still has the first slot
                                // as the key — so unconditional accept is correct).
                                if !key.is_empty() && !key.contains(char::is_whitespace) {
                                    out.push(key.to_string());
                                }
                                i = j + 1;
                                continue;
                            }
                        }
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// Check whether a dotted key (e.g. `nav.catalog`) resolves to a leaf in the
    /// YAML tree. Leaves are scalars; maps are rejected.
    fn key_exists(value: &serde_yaml::Value, dotted: &str) -> bool {
        let mut current = value;
        for segment in dotted.split('.') {
            match current {
                serde_yaml::Value::Mapping(map) => {
                    let k = serde_yaml::Value::String(segment.to_string());
                    match map.get(&k) {
                        Some(v) => current = v,
                        None => return false,
                    }
                }
                _ => return false,
            }
        }
        !matches!(current, serde_yaml::Value::Mapping(_))
    }

    fn project_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Replace `/* ... */` block-comment regions with spaces of equal length
    /// so byte indices are preserved for downstream scanning.
    fn strip_block_comments(src: &str) -> String {
        let bytes = src.as_bytes();
        let mut out = String::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                // Find closing `*/`; if missing, consume the rest.
                let mut j = i + 2;
                while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
                    j += 1;
                }
                let end = if j + 1 < bytes.len() { j + 2 } else { bytes.len() };
                for b in &bytes[i..end] {
                    out.push(if *b == b'\n' { '\n' } else { ' ' });
                }
                i = end;
            } else {
                out.push(bytes[i] as char);
                i += 1;
            }
        }
        out
    }

    #[test]
    fn strip_block_comments_replaces_with_spaces_preserving_newlines() {
        // `/* t!("skip.me") */` is 19 bytes → replaced by 19 spaces. The
        // surrounding single spaces stay as-is, so the gap is 21 chars.
        let stripped = strip_block_comments("before /* t!(\"skip.me\") */ after");
        assert_eq!(stripped.len(), "before /* t!(\"skip.me\") */ after".len());
        assert!(stripped.starts_with("before "));
        assert!(stripped.ends_with(" after"));
        assert!(
            stripped.chars().skip(6).take(stripped.len() - 12).all(|c| c == ' '),
            "block-comment region must be replaced with spaces: {stripped:?}"
        );
        assert_eq!(
            extract_t_keys("let a = /* t!(\"skip.me\") */ 42;"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn all_t_keys_have_both_locales() {
        let root = project_root();
        let src_dir = root.join("src");
        let files = rust_files(&src_dir);
        assert!(!files.is_empty(), "no .rs files found under src/");

        let mut keys: BTreeSet<String> = BTreeSet::new();
        for file in &files {
            // Skip this audit file itself — it contains illustrative example
            // keys in its own unit tests (`one.two`, `keep.me`, …) that must
            // not be treated as production translation requirements.
            if file.ends_with("i18n/audit.rs") {
                continue;
            }
            let content = fs::read_to_string(file)
                .unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
            for k in extract_t_keys(&content) {
                keys.insert(k);
            }
        }

        let en_text = fs::read_to_string(root.join("locales/en.yml")).expect("read en.yml");
        let fr_text = fs::read_to_string(root.join("locales/fr.yml")).expect("read fr.yml");
        let en: serde_yaml::Value = serde_yaml::from_str(&en_text).expect("parse en.yml");
        let fr: serde_yaml::Value = serde_yaml::from_str(&fr_text).expect("parse fr.yml");

        let mut missing_en = Vec::new();
        let mut missing_fr = Vec::new();
        for key in &keys {
            if !key_exists(&en, key) {
                missing_en.push(key.clone());
            }
            if !key_exists(&fr, key) {
                missing_fr.push(key.clone());
            }
        }

        if !missing_en.is_empty() || !missing_fr.is_empty() {
            panic!(
                "i18n audit failed\n  scanned {} keys across {} files\n  missing in en.yml: {:?}\n  missing in fr.yml: {:?}",
                keys.len(),
                files.len(),
                missing_en,
                missing_fr
            );
        }
    }

    #[test]
    fn extract_t_keys_finds_bare_key() {
        let keys = extract_t_keys(r#"rust_i18n::t!("nav.catalog").to_string()"#);
        assert_eq!(keys, vec!["nav.catalog"]);
    }

    #[test]
    fn extract_t_keys_finds_parameterized_key() {
        let keys =
            extract_t_keys(r#"t!("feedback.title_created", title = &title.title).to_string()"#);
        assert_eq!(keys, vec!["feedback.title_created"]);
    }

    #[test]
    fn extract_t_keys_handles_whitespace() {
        let keys = extract_t_keys("t! (   \"login.title\"  )");
        assert_eq!(keys, vec!["login.title"]);
    }

    #[test]
    fn extract_t_keys_multiple_calls() {
        let src = r#"
            let a = t!("one.two");
            let b = rust_i18n::t!("three.four.five", x = "y");
        "#;
        let keys = extract_t_keys(src);
        assert_eq!(keys, vec!["one.two", "three.four.five"]);
    }

    #[test]
    fn extract_t_keys_skips_non_t_macros() {
        // Other macros or identifiers ending in `t!` should NOT match. Our rule
        // rejects identifiers like `foo_t!(...)` via the prev-char check.
        let src = r#"foo_t!("skip.me"); t!("keep.me");"#;
        let keys = extract_t_keys(src);
        assert_eq!(keys, vec!["keep.me"]);
    }

    #[test]
    fn key_exists_accepts_leaf() {
        let v: serde_yaml::Value =
            serde_yaml::from_str("nav:\n  catalog: Catalogue\n  loans: Prêts").unwrap();
        assert!(key_exists(&v, "nav.catalog"));
        assert!(key_exists(&v, "nav.loans"));
    }

    #[test]
    fn key_exists_rejects_missing_path() {
        let v: serde_yaml::Value = serde_yaml::from_str("nav:\n  catalog: Catalogue").unwrap();
        assert!(!key_exists(&v, "nav.missing"));
        assert!(!key_exists(&v, "other.foo"));
    }

    #[test]
    fn key_exists_rejects_mapping_as_leaf() {
        // `nav` itself is a mapping, not a leaf string, so it must not match.
        let v: serde_yaml::Value = serde_yaml::from_str("nav:\n  catalog: Catalogue").unwrap();
        assert!(!key_exists(&v, "nav"));
    }
}
