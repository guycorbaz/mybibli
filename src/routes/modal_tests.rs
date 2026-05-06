//! Story 9-10 — Modal macro variant render tests.
//!
//! Exercises `templates/components/modal.html::modal` via a tiny test
//! wrapper template (`templates/fragments/modal_test_wrapper.html`). One
//! case per variant + 1 negative (invalid variant defaults to `warning`).
//! AC8 of story 9-10.

use askama::Template;

#[derive(Template)]
#[template(path = "fragments/modal_test_wrapper.html")]
struct ModalTestWrapper {
    variant: &'static str,
    title: &'static str,
    body_html: &'static str,
    confirm_label: &'static str,
    cancel_label: &'static str,
    action_url: &'static str,
    action_method: &'static str,
    csrf_token: &'static str,
    hx_target: &'static str,
    hx_swap: &'static str,
}

fn render(variant: &'static str, action_method: &'static str) -> String {
    render_with_targets(variant, action_method, "", "none")
}

fn render_with_targets(
    variant: &'static str,
    action_method: &'static str,
    hx_target: &'static str,
    hx_swap: &'static str,
) -> String {
    ModalTestWrapper {
        variant,
        title: "Delete Alice?",
        body_html: "<p>Body text.</p>",
        confirm_label: "Delete",
        cancel_label: "Cancel",
        action_url: "/borrower/42",
        action_method,
        csrf_token: "tok123",
        hx_target,
        hx_swap,
    }
    .render()
    .expect("render")
}

/// All variants share the dialog shape that scanner-guard 7-5 watches.
fn assert_scanner_guard_contract(html: &str) {
    assert!(
        html.contains("<dialog open aria-modal=\"true\""),
        "modal must use the `<dialog open aria-modal=\"true\">` selector \
         shape so scanner-guard.js (story 7-5) protects it; got: {html}"
    );
}

/// Cancel must be the FIRST `<button>` in the modal AND carry
/// `data-modal-default-focus` (UX-DR8 invariant — Cancel never destroys).
fn assert_cancel_is_first_button(html: &str) {
    let first_button_pos = html.find("<button").expect("at least one button");
    let cancel_marker_pos = html
        .find("data-modal-default-focus")
        .expect("data-modal-default-focus marker present");
    let confirm_marker_pos = html
        .find("data-modal-confirm")
        .expect("data-modal-confirm marker present");
    assert!(
        cancel_marker_pos > first_button_pos
            && cancel_marker_pos < html[first_button_pos..]
                .find("</button>")
                .map(|p| first_button_pos + p)
                .unwrap_or(usize::MAX),
        "data-modal-default-focus must be inside the FIRST <button> tag; got: {html}"
    );
    assert!(
        confirm_marker_pos > cancel_marker_pos,
        "Confirm button must come AFTER the Cancel button (Cancel is first tab stop); got: {html}"
    );
}

#[test]
fn delete_variant_renders_red_confirm_and_hx_delete() {
    let html = render("delete", "DELETE");
    assert_scanner_guard_contract(&html);
    assert_cancel_is_first_button(&html);
    assert!(
        html.contains("data-modal-variant=\"delete\""),
        "variant attribute must round-trip; got: {html}"
    );
    assert!(
        html.contains("bg-red-600 hover:bg-red-700"),
        "delete variant uses red Tailwind palette; got: {html}"
    );
    assert!(
        html.contains("hx-delete=\"/borrower/42\""),
        "DELETE action method emits hx-delete; got: {html}"
    );
    assert!(html.contains("Delete Alice?"));
    assert!(html.contains("<p>Body text.</p>"));
}

#[test]
fn delete_forever_variant_renders_red_confirm_and_hx_post() {
    let html = render("delete-forever", "POST");
    assert_scanner_guard_contract(&html);
    assert_cancel_is_first_button(&html);
    assert!(
        html.contains("bg-red-600 hover:bg-red-700"),
        "delete-forever variant ALSO uses red palette (same severity tier); got: {html}"
    );
    assert!(
        html.contains("hx-post=\"/borrower/42\""),
        "POST action method emits hx-post; got: {html}"
    );
}

#[test]
fn remove_variant_renders_amber_confirm() {
    let html = render("remove", "POST");
    assert_scanner_guard_contract(&html);
    assert_cancel_is_first_button(&html);
    assert!(
        html.contains("bg-amber-600 hover:bg-amber-700"),
        "remove variant uses amber palette (less severe than delete); got: {html}"
    );
    assert!(html.contains("hx-post=\"/borrower/42\""));
}

#[test]
fn warning_variant_renders_indigo_confirm() {
    let html = render("warning", "POST");
    assert_scanner_guard_contract(&html);
    assert_cancel_is_first_button(&html);
    assert!(
        html.contains("bg-indigo-600 hover:bg-indigo-700"),
        "warning variant uses indigo palette (least severe); got: {html}"
    );
}

/// Invalid variant strings MUST fall through to the `warning` palette
/// rather than panicking or emitting empty class attrs (defense-in-depth
/// against a future renamed enum or typo).
#[test]
fn invalid_variant_falls_back_to_warning_palette() {
    let html = render("not-a-real-variant", "POST");
    assert_scanner_guard_contract(&html);
    assert!(
        html.contains("bg-indigo-600 hover:bg-indigo-700"),
        "unknown variant must fall back to the `warning` (indigo) palette; \
         got: {html}"
    );
    assert!(
        !html.contains("bg-red-600") && !html.contains("bg-amber-600"),
        "fallback must not surface a destructive palette; got: {html}"
    );
}

/// Story 9-11 AC8 — when callers pass an explicit `hx_target` + `hx_swap`,
/// the rendered form MUST emit those attrs verbatim. Default ("", "none")
/// preserves the 9-10 contract (no hx-target attr, hx-swap="none").
#[test]
fn warning_variant_with_hx_target_renders_innerhtml_swap() {
    let html = render_with_targets("warning", "POST", "#loan-feedback", "innerHTML");
    assert!(
        html.contains("hx-swap=\"innerHTML\""),
        "explicit hx_swap must round-trip verbatim; got: {html}"
    );
    assert!(
        html.contains("hx-target=\"#loan-feedback\""),
        "explicit hx_target must render an hx-target attribute; got: {html}"
    );
    assert!(
        !html.contains("hx-swap=\"none\""),
        "default hx-swap=\"none\" must NOT appear when caller overrides; got: {html}"
    );
}

#[test]
fn warning_variant_default_omits_hx_target_attribute() {
    // 9-10 backward-compat: an empty hx_target string must produce NO
    // hx-target= attribute on the form, while hx_swap defaults to "none".
    let html = render("warning", "POST");
    assert!(
        html.contains("hx-swap=\"none\""),
        "default hx_swap=\"none\" must render verbatim; got: {html}"
    );
    assert!(
        !html.contains("hx-target="),
        "empty hx_target must NOT emit an hx-target attribute (backward compat \
         with 9-10 callers); got: {html}"
    );
}

/// Story 9-11 — proves the macro renders byte-identical bodies for the two
/// valid feedback targets, modulo only the target string itself. This locks
/// the AC8 DRY claim at the unit-test layer: a future contributor can't
/// accidentally introduce per-target divergence in the macro.
#[test]
fn warning_variant_byte_identical_across_feedback_targets() {
    let loan_html = render_with_targets("warning", "POST", "#loan-feedback", "innerHTML");
    let borrower_html =
        render_with_targets("warning", "POST", "#borrower-feedback", "innerHTML");
    let scan_html = render_with_targets("warning", "POST", "#scan-result", "innerHTML");

    let normalized_loan = loan_html.replace("#loan-feedback", "TARGET_PLACEHOLDER");
    let normalized_borrower = borrower_html.replace("#borrower-feedback", "TARGET_PLACEHOLDER");
    let normalized_scan = scan_html.replace("#scan-result", "TARGET_PLACEHOLDER");

    assert_eq!(
        normalized_loan, normalized_borrower,
        "modal output must be byte-identical between #loan-feedback and \
         #borrower-feedback (AC8 DRY claim)"
    );
    assert_eq!(
        normalized_loan, normalized_scan,
        "modal output must be byte-identical for the scan-card target too"
    );
}
