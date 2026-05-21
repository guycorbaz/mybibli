//! CR #275 / #276 (v1.7.0) — locale-parity guard.
//!
//! Locks the contract that every translation YAML carries EXACTLY the same
//! flattened key set as `en.yml`, in the same shape (scalar values, not
//! sub-maps). Without this guard, a careless YAML edit on one locale would
//! silently fall through to the `fallback = "en"` for missing keys, and a
//! key typo would never surface until a user picked the affected locale.
//!
//! The test is intentionally a `tests/*.rs` integration file (not a unit
//! `#[cfg(test)] mod tests`) so it can read the YAML files directly without
//! depending on the `rust_i18n!` proc macro's expanded state.
//!
//! New locales should be added to `LOCALES_TO_CHECK` as they ship.

use serde_yaml::Value;
use std::collections::BTreeSet;
use std::fs;

// Extend this list when shipping a new translation YAML. CR #275 (DE) added
// `"de"`; CR #276 (IT) added `"it"` (v1.7.0 bundle).
const LOCALES_TO_CHECK: &[&str] = &["de", "it"];

/// Flatten a nested YAML mapping to a sorted set of dot-joined paths,
/// keeping only paths that resolve to a scalar value. Missing intermediate
/// nodes are NOT errors here — those surface as set differences against the
/// reference set.
fn flatten_keys(prefix: &str, value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Mapping(map) => {
            for (k, v) in map {
                let Some(key_str) = k.as_str() else { continue };
                let next = if prefix.is_empty() {
                    key_str.to_string()
                } else {
                    format!("{prefix}.{key_str}")
                };
                flatten_keys(&next, v, out);
            }
        }
        // Treat strings AND null and other scalars as leaves so a deliberate
        // `key:` (empty value, falls back to placeholder) doesn't trip the
        // parity check.
        _ => {
            out.insert(prefix.to_string());
        }
    }
}

fn load_keys(locale: &str) -> BTreeSet<String> {
    let path = format!("locales/{locale}.yml");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {}", path, e));
    let parsed: Value = serde_yaml::from_str(&raw)
        .unwrap_or_else(|e| panic!("parse {}: {}", path, e));
    let mut out = BTreeSet::new();
    flatten_keys("", &parsed, &mut out);
    out
}

#[test]
fn fr_matches_en_key_set() {
    let en = load_keys("en");
    let fr = load_keys("fr");
    let missing_in_fr: Vec<&String> = en.difference(&fr).collect();
    let extra_in_fr: Vec<&String> = fr.difference(&en).collect();
    assert!(
        missing_in_fr.is_empty() && extra_in_fr.is_empty(),
        "fr.yml drift vs en.yml — missing: {:?}, extra: {:?}",
        missing_in_fr, extra_in_fr
    );
}

#[test]
fn new_locales_match_en_key_set() {
    let en = load_keys("en");
    for locale in LOCALES_TO_CHECK {
        let other = load_keys(locale);
        let missing: Vec<&String> = en.difference(&other).collect();
        let extra: Vec<&String> = other.difference(&en).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "{}.yml drift vs en.yml — missing: {:?}, extra: {:?}",
            locale, missing, extra
        );
    }
}

#[test]
fn placeholder_set_matches_en_for_every_translation() {
    // Per-key check: each translated value MUST carry the same set of
    // `%{name}` placeholders as the EN reference. A translator who omits
    // a placeholder silently produces a string that doesn't substitute
    // the user's data at runtime.

    fn extract_placeholders(s: &str) -> BTreeSet<String> {
        let re = regex::Regex::new(r"%\{([^}]+)\}").unwrap();
        re.captures_iter(s).map(|c| c[1].to_string()).collect()
    }

    fn walk(
        prefix: &str,
        value: &Value,
        out: &mut std::collections::BTreeMap<String, String>,
    ) {
        match value {
            Value::Mapping(map) => {
                for (k, v) in map {
                    let Some(key_str) = k.as_str() else { continue };
                    let next = if prefix.is_empty() {
                        key_str.to_string()
                    } else {
                        format!("{prefix}.{key_str}")
                    };
                    walk(&next, v, out);
                }
            }
            Value::String(s) => {
                out.insert(prefix.to_string(), s.clone());
            }
            _ => {}
        }
    }

    let en_raw = fs::read_to_string("locales/en.yml").expect("read en.yml");
    let en_parsed: Value = serde_yaml::from_str(&en_raw).expect("parse en.yml");
    let mut en_strings = std::collections::BTreeMap::new();
    walk("", &en_parsed, &mut en_strings);

    for locale in LOCALES_TO_CHECK.iter().chain(std::iter::once(&"fr")) {
        let raw = fs::read_to_string(format!("locales/{locale}.yml"))
            .unwrap_or_else(|e| panic!("read {}.yml: {}", locale, e));
        let parsed: Value = serde_yaml::from_str(&raw)
            .unwrap_or_else(|e| panic!("parse {}.yml: {}", locale, e));
        let mut strings = std::collections::BTreeMap::new();
        walk("", &parsed, &mut strings);

        for (key, en_val) in &en_strings {
            let en_holes = extract_placeholders(en_val);
            if en_holes.is_empty() {
                continue;
            }
            let Some(translated) = strings.get(key) else {
                continue;
            };
            let translated_holes = extract_placeholders(translated);
            assert_eq!(
                en_holes, translated_holes,
                "{}.yml — placeholder drift at `{}`: en has {:?}, {} has {:?}",
                locale, key, en_holes, locale, translated_holes
            );
        }
    }
}
