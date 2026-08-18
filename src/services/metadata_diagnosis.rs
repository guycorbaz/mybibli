//! #202 tier 2 — turn a chain run into a sentence a librarian can act on.
//!
//! The provider chain has always known why each provider came back
//! empty-handed; before this module the distinctions lived only in the log,
//! and the user got `metadata.redownload_failed` — "we could not find
//! metadata" — for every possible cause. That single message covers three
//! situations demanding three different reactions:
//!
//! - nobody holds the title → nothing to do, catalogue it by hand;
//! - a source was throttled, slow or broken → try again shortly;
//! - a keyed source was never asked → set the key and retry.
//!
//! Per `docs/error-message-style.md` the output is tripartite (what happened →
//! why → what you can do) and never exposes HTTP codes or internal names.
//! Provider names ARE emitted verbatim: they are proper nouns and, per NFR41
//! and the #202 tier-1 precedent, are not translated.

use crate::metadata::chain::{AttemptOutcome, ProviderAttempt};

/// Build the suggestion line shown beside a failed metadata lookup.
///
/// Returns `None` when there is nothing honest to say — an empty attempt list
/// means no provider was consulted (cache hit, or no chain for the media
/// type), and inventing a diagnosis there would be worse than staying silent.
pub fn describe_failure(attempts: &[ProviderAttempt], locale: &str) -> Option<String> {
    if attempts.is_empty() {
        return None;
    }

    let names = |list: Vec<&ProviderAttempt>| -> String {
        list.iter()
            .map(|a| a.provider.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    let unconfigured: Vec<&ProviderAttempt> = attempts
        .iter()
        .filter(|a| a.outcome == AttemptOutcome::NotConfigured)
        .collect();
    let retryable: Vec<&ProviderAttempt> =
        attempts.iter().filter(|a| a.is_worth_retrying()).collect();
    let searched = attempts
        .iter()
        .filter(|a| a.outcome == AttemptOutcome::NoResult)
        .count();

    let mut parts: Vec<String> = Vec::new();

    // Order matters: the actionable causes come first, because a reader who
    // stops after one sentence should stop on the one they can act on. The
    // "nobody holds it" line is the resigned conclusion and goes last.
    if !retryable.is_empty() {
        parts.push(
            rust_i18n::t!(
                "metadata.diagnosis.transient",
                locale = locale,
                providers = names(retryable)
            )
            .to_string(),
        );
    }
    if !unconfigured.is_empty() {
        parts.push(
            rust_i18n::t!(
                "metadata.diagnosis.not_configured",
                locale = locale,
                providers = names(unconfigured)
            )
            .to_string(),
        );
    }
    if searched > 0 {
        // rust_i18n does not select a plural form on its own here — the
        // project picks the key (see wishlist.print_count_one/_other). "1
        // source(s)" is developer shorthand, and the style guide asks for
        // plain language.
        let line = if searched == 1 {
            rust_i18n::t!("metadata.diagnosis.searched_one", locale = locale).to_string()
        } else {
            rust_i18n::t!(
                "metadata.diagnosis.searched_other",
                locale = locale,
                count = searched.to_string()
            )
            .to_string()
        };
        parts.push(line);
    }

    if parts.is_empty() {
        return None;
    }
    Some(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(provider: &str, outcome: AttemptOutcome) -> ProviderAttempt {
        ProviderAttempt {
            provider: provider.to_string(),
            outcome,
            duration_ms: 42,
        }
    }

    #[test]
    fn no_attempts_yields_no_claim() {
        // A cache hit consults nobody. Saying "0 sources searched" would be a
        // statement about the world that was never checked.
        assert!(describe_failure(&[], "en").is_none());
    }

    #[test]
    fn a_single_source_reads_as_singular_not_as_1_source_s() {
        let a = vec![attempt("BnF", AttemptOutcome::NoResult)];
        let msg = describe_failure(&a, "en").expect("some");
        assert!(
            !msg.contains("(s)") && !msg.contains("1 source"),
            "one source must read as prose, not as developer shorthand: {msg}"
        );
    }

    #[test]
    fn all_searched_and_none_hold_it_reports_the_count() {
        let a = vec![
            attempt("BnF", AttemptOutcome::NoResult),
            attempt("Google Books", AttemptOutcome::NoResult),
        ];
        let msg = describe_failure(&a, "en").expect("some");
        assert!(msg.contains('2'), "should count the sources searched: {msg}");
        // No retry advice — retrying an honest miss wastes the user's time.
        assert!(
            !msg.to_lowercase().contains("again"),
            "a genuine miss must not suggest retrying: {msg}"
        );
    }

    #[test]
    fn throttled_provider_is_named_and_retry_is_advised() {
        let a = vec![
            attempt("Google Books", AttemptOutcome::Unavailable),
            attempt("BnF", AttemptOutcome::NoResult),
        ];
        let msg = describe_failure(&a, "en").expect("some");
        assert!(msg.contains("Google Books"), "{msg}");
        assert!(msg.to_lowercase().contains("again"), "{msg}");
        // Style guide: no HTTP codes in user-facing copy.
        assert!(!msg.contains("503") && !msg.contains("429"), "{msg}");
    }

    #[test]
    fn missing_key_is_reported_separately_from_a_miss() {
        // The distinction this whole tier exists for: OMDb returning Ok(None)
        // for want of a key used to read exactly like OMDb searching and
        // finding nothing.
        let a = vec![
            attempt("OMDb", AttemptOutcome::NotConfigured),
            attempt("BnF", AttemptOutcome::NoResult),
        ];
        let msg = describe_failure(&a, "en").expect("some");
        assert!(msg.contains("OMDb"), "{msg}");
        assert!(
            msg.to_lowercase().contains("key"),
            "must name the missing key as the cause: {msg}"
        );
    }

    #[test]
    fn a_timeout_counts_as_retryable_not_as_a_miss() {
        let a = vec![attempt(
            "Library of Congress",
            AttemptOutcome::TimedOut { after_secs: 10 },
        )];
        let msg = describe_failure(&a, "en").expect("some");
        assert!(msg.contains("Library of Congress"), "{msg}");
        assert!(msg.to_lowercase().contains("again"), "{msg}");
        // Never leak the tuning knob's value as if it were the user's problem.
        assert!(!msg.contains("10"), "{msg}");
    }

    #[test]
    fn every_locale_produces_a_non_empty_translated_line() {
        let a = vec![
            attempt("OMDb", AttemptOutcome::NotConfigured),
            attempt("BnF", AttemptOutcome::NoResult),
        ];
        for loc in ["de", "en", "fr", "it"] {
            let msg = describe_failure(&a, loc).expect("some");
            assert!(!msg.trim().is_empty(), "{loc}");
            // A missing key renders as the raw key path; catch that here
            // rather than in production.
            assert!(
                !msg.contains("metadata.diagnosis"),
                "{loc} is missing a translation: {msg}"
            );
            assert!(msg.contains("OMDb"), "provider names are never translated: {loc}");
        }
    }

    #[test]
    fn answered_alone_is_not_a_failure_description() {
        // Defensive: describe_failure is only called on the failure path, but
        // if it ever sees a successful run it must not manufacture a
        // complaint.
        let a = vec![attempt("BnF", AttemptOutcome::Answered)];
        assert!(describe_failure(&a, "en").is_none());
    }
}
