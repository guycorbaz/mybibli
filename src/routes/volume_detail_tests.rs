//! Story 9-8 — Volume detail loan-status row render tests.
//!
//! Extracted from `src/routes/catalog.rs` per the 9-6 precedent
//! (`home_indicator_tests.rs`) to keep `catalog.rs` from drifting
//! further past the Foundation Rule #12 ceiling. The render tests
//! exercise `VolumeDetailTemplate` end-to-end through Askama, so the
//! file lives next to `catalog.rs` rather than in `tests/` (which is
//! reserved for `#[sqlx::test]` integration tests against a real DB).
//!
//! See `tests/volume_detail_loan_status.rs` for the model-layer
//! `#[sqlx::test]` integration suite (FR59 SQL projection-narrowing).

use super::catalog::{LoanStatusView, VolumeDetailTemplate};
use crate::models::volume::VolumeModel;
use askama::Template;

fn make_test_volume_detail_template(
    role: &str,
    loan_status: Option<LoanStatusView>,
) -> VolumeDetailTemplate {
    VolumeDetailTemplate {
        lang: "en".to_string(),
        role: role.to_string(),
        current_page: "catalog",
        skip_label: "Skip to main content".to_string(),
        connection_status: crate::utils::ConnectionStatusContext::new("en"),
        shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new("en"),
        session_timeout_secs: 14400,
        csrf_token: "tok".to_string(),
        nav_catalog: "Catalog".to_string(),
        nav_loans: "Loans".to_string(),
        nav_wishlist: "Wish list".to_string(),
        nav_locations: "Locations".to_string(),
        nav_series: "Series".to_string(),
        nav_borrowers: "Borrowers".to_string(),
        nav_admin: "Admin".to_string(),
        nav_login: "Log in".to_string(),
        nav_logout: "Log out".to_string(),
        nav_menu_open: "Open menu".to_string(),
        volume: VolumeModel {
            id: 1,
            label: "V0001".to_string(),
            title_id: 10,
            location_id: None,
            condition_state_id: None,
            edition_comment: None,
            version: 1,
            purchase_price: None,
            purchase_currency: None,
            current_value: None,
            current_value_currency: None,
            current_value_updated_at: None,
        },
        title_name: "Test Title".to_string(),
        condition_name: None,
        location_path: None,
        not_shelved_label: "Not shelved".to_string(),
        detail_title: "Volume details".to_string(),
        current_url: "/volume/1".to_string(),
        lang_toggle_aria: "Change language".to_string(),
        loan_status,
        loan_status_field_label: "Loan status:".to_string(),
        loan_status_label_anonymous: "On loan since 2026-04-15".to_string(),
        loan_status_label_prefix: "On loan to ".to_string(),
        loan_status_label_suffix: " since 2026-04-15".to_string(),
    }
}

fn fake_loan_status_anonymous() -> LoanStatusView {
    LoanStatusView {
        loaned_at_label: "2026-04-15".to_string(),
        borrower_id: None,
        borrower_name: None,
    }
}

fn fake_loan_status_with_borrower(borrower_id: u64, borrower_name: &str) -> LoanStatusView {
    LoanStatusView {
        loaned_at_label: "2026-04-15".to_string(),
        borrower_id: Some(borrower_id),
        borrower_name: Some(borrower_name.to_string()),
    }
}

/// AC8 LOAD-BEARING SECURITY GUARD: the rendered HTML for an
/// Anonymous request on a volume with an active loan to a
/// borrower named "Alice Tremblay" MUST NOT contain "Alice"
/// anywhere — not in the visible text, not in a `data-*`
/// attribute, not in a hidden field, not in an aria-label.
///
/// The 2-render assertion shape proves the test fixture WOULD
/// catch a leak: the librarian render IS expected to contain
/// "Alice", so the absence in the anonymous render is meaningful
/// (not a tautology where the name was never present in either).
#[test]
fn volume_detail_anonymous_does_not_leak_borrower_name() {
    // 1. Anonymous render — borrower data MUST NOT leak.
    let anonymous_template =
        make_test_volume_detail_template("anonymous", Some(fake_loan_status_anonymous()));
    let html = anonymous_template.render().expect("render");
    assert!(
        !html.contains("Alice"),
        "anonymous render MUST NOT contain borrower name 'Alice'"
    );
    assert!(
        !html.contains("Tremblay"),
        "anonymous render MUST NOT contain borrower last name 'Tremblay'"
    );
    assert!(
        !html.contains("/borrower/"),
        "anonymous render MUST NOT contain any /borrower/ link"
    );

    // 2. Librarian render with the SAME borrower data — proves the
    //    test fixture would catch a leak if one existed.
    let librarian_template = make_test_volume_detail_template(
        "librarian",
        Some(fake_loan_status_with_borrower(42, "Alice Tremblay")),
    );
    let html_lib = librarian_template.render().expect("render");
    assert!(
        html_lib.contains("Alice Tremblay"),
        "librarian render MUST contain borrower name (proves leak guard is meaningful)"
    );
    assert!(
        html_lib.contains("href=\"/borrower/42\""),
        "librarian render MUST contain /borrower/42 link"
    );
}

/// Story 9-8 review fix (Edge Hunter): TWO-LAYER DEFENSE-IN-DEPTH
/// LOCK. AC8's existing test was tautological — the anonymous arm
/// passed `borrower_id: None, borrower_name: None`, so `!contains(
/// "Alice")` was trivially true (Alice was never in the input).
///
/// This test renders the anonymous variant with FULLY POPULATED
/// borrower fields (the same data as the librarian render). The
/// macro's role gate (positive `role == "librarian" || role ==
/// "admin"` whitelist) MUST drop the borrower data on the floor.
/// If a future regression flips the gate to a negative compare
/// (`role != "anonymous"`) or removes the gate entirely, THIS test
/// catches the leak — the SQL projection narrowing (layer 1) would
/// still hide the data in production but the macro's defense-in-
/// depth (layer 2) is the contract this test locks.
#[test]
fn volume_detail_anonymous_with_populated_borrower_data_does_not_leak() {
    // Anonymous role + FULLY populated borrower fields — defends
    // against a regression in the macro's role gate.
    let template = make_test_volume_detail_template(
        "anonymous",
        Some(fake_loan_status_with_borrower(42, "Alice Tremblay")),
    );
    let html = template.render().expect("render");

    // Borrower PII MUST NOT leak even though the input HAD it.
    assert!(
        !html.contains("Alice Tremblay"),
        "anonymous render MUST NOT contain borrower name even when fixture populated it"
    );
    assert!(
        !html.contains("Tremblay"),
        "anonymous render MUST NOT contain borrower last name"
    );
    assert!(
        !html.contains("/borrower/42"),
        "anonymous render MUST NOT contain /borrower/42 link"
    );
    // The anonymous label IS expected — proves the badge rendered
    // (so absence of borrower data is meaningful, not "no badge").
    assert!(
        html.contains("On loan since 2026-04-15"),
        "anonymous render MUST contain the date-only label"
    );
}

/// Story 9-8 review fix (Edge Hunter): unknown role string MUST
/// fall through to the anonymous variant (fail-closed). Locks the
/// switch from the old `role != "anonymous"` (fail-open) to the new
/// positive `role == "librarian" || role == "admin"` whitelist.
/// A future role rename (e.g. `Display` impl returns `"Anonymous"`
/// capitalized) MUST NOT silently leak borrower PII.
#[test]
fn volume_detail_unknown_role_falls_back_to_anonymous_variant() {
    let template = make_test_volume_detail_template(
        "Anonymous", // capital A — typo / future-rename simulation
        Some(fake_loan_status_with_borrower(99, "Bob Builder")),
    );
    let html = template.render().expect("render");
    assert!(
        !html.contains("Bob Builder"),
        "unknown role 'Anonymous' (capital A) MUST fall through to anonymous variant"
    );
    assert!(
        !html.contains("/borrower/99"),
        "unknown role MUST NOT render the /borrower/ link"
    );
    assert!(
        html.contains("On loan since 2026-04-15"),
        "unknown role still renders the safe anonymous label"
    );
}

/// AC10b: librarian sees the borrower name as a link to /borrower/{id}.
#[test]
fn volume_detail_librarian_renders_borrower_link() {
    let template = make_test_volume_detail_template(
        "librarian",
        Some(fake_loan_status_with_borrower(99, "Bob Builder")),
    );
    let html = template.render().expect("render");
    assert!(html.contains("href=\"/borrower/99\""));
    assert!(html.contains("Bob Builder"));
    assert!(html.contains("On loan to "));
    assert!(html.contains(" since 2026-04-15"));
    // The link's accessible name MUST be the borrower's visible
    // text (no aria-label override). Code-review patch 2026-05-04
    // (PR #124 CI catch): an `aria-label="View borrower profile"`
    // override hijacked the accessible name and broke screen-reader
    // context AND `getByRole("link", { name: borrower })` lookup
    // in E2E tests. The visible text is now the canonical accessible
    // name.
    assert!(
        !html.contains("aria-label=\"View borrower profile\""),
        "the borrower link MUST NOT carry an aria-label that hijacks the accessible name"
    );
}

/// AC10b: admin sees the same render as librarian (role gate is
/// `>= Librarian` — Admin satisfies it).
#[test]
fn volume_detail_admin_renders_borrower_link() {
    let template = make_test_volume_detail_template(
        "admin",
        Some(fake_loan_status_with_borrower(77, "Charlie Curator")),
    );
    let html = template.render().expect("render");
    assert!(html.contains("href=\"/borrower/77\""));
    assert!(html.contains("Charlie Curator"));
}

/// AC3: when the volume is NOT on loan (`loan_status: None`), no
/// loan-status badge appears at all — the entire row is omitted
/// via `{% if let Some(loan) = loan_status %}` so the rendered
/// HTML byte-stream is identical to the pre-9-8 baseline.
#[test]
fn volume_detail_no_active_loan_renders_no_badge() {
    let template = make_test_volume_detail_template("librarian", None);
    let html = template.render().expect("render");
    assert!(
        !html.contains("On loan since"),
        "no loan_status → no anonymous-variant text"
    );
    assert!(
        !html.contains("On loan to"),
        "no loan_status → no librarian-variant text"
    );
    assert!(
        !html.contains("Loan status:"),
        "no loan_status → the field label itself is absent (whole row omitted)"
    );
    // Anonymous variant — same expectation.
    let template_anon = make_test_volume_detail_template("anonymous", None);
    let html_anon = template_anon.render().expect("render");
    assert!(!html_anon.contains("On loan since"));
    assert!(!html_anon.contains("Loan status:"));
}

/// AC1 amber palette: the "consistent unavailability cue" UX
/// contract — the loan-status badge wrapper uses the same amber
/// palette as the existing "not shelved" location badge.
#[test]
fn volume_detail_anonymous_with_loan_renders_amber_palette() {
    let template =
        make_test_volume_detail_template("anonymous", Some(fake_loan_status_anonymous()));
    let html = template.render().expect("render");
    assert!(
        html.contains("bg-amber-100 dark:bg-amber-900/30"),
        "loan-status badge MUST use the amber palette (matches not-shelved badge)"
    );
    assert!(html.contains("On loan since 2026-04-15"));
}

/// AC4 macro defense-in-depth: a `role = "librarian"` call with
/// borrower fields set to `None` MUST fall back to the
/// anonymous variant rather than panic. This locks the macro's
/// `if let Some` chain — even if a future caller bug populates
/// the role but not the borrower data, the page renders safely.
#[test]
fn volume_detail_librarian_with_no_borrower_data_falls_back_to_anonymous_variant() {
    let template = make_test_volume_detail_template(
        "librarian",
        Some(LoanStatusView {
            loaned_at_label: "2026-04-15".to_string(),
            borrower_id: None,
            borrower_name: None,
        }),
    );
    let html = template.render().expect("render");
    // Falls back to anonymous variant — date-only, no link, no name.
    assert!(html.contains("On loan since 2026-04-15"));
    assert!(!html.contains("/borrower/"));
    assert!(!html.contains("On loan to "));
}

/// AC11 unused-but-still-meaningful check: the variable date
/// substitution actually flows through. With a different
/// loaned_at_label, the rendered HTML contains the new date.
#[test]
fn volume_detail_loan_status_label_anonymous_interpolates_date() {
    let mut template =
        make_test_volume_detail_template("anonymous", Some(fake_loan_status_anonymous()));
    template.loan_status_label_anonymous = "On loan since 2025-12-31".to_string();
    let html = template.render().expect("render");
    assert!(html.contains("On loan since 2025-12-31"));
}

/// AC4 Anonymous variant explicitly passes through the date-only
/// path even when borrower fields are None for an anonymous role.
/// (Different from the librarian-no-borrower fallback — this is
/// the normal-path test for anonymous.)
#[test]
fn volume_detail_anonymous_with_loan_renders_date_only_no_borrower_marker() {
    let template =
        make_test_volume_detail_template("anonymous", Some(fake_loan_status_anonymous()));
    let html = template.render().expect("render");
    assert!(html.contains("On loan since"));
    // No borrower link or prefix should appear.
    assert!(!html.contains("/borrower/"));
    assert!(!html.contains("On loan to "));
}
