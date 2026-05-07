//! Story 9-15 — `status_message` macro variant + role-gating render tests.
//!
//! Exercises `templates/components/status_message.html::status_message` via
//! a tiny test wrapper template (`templates/fragments/status_message_test_wrapper.html`).
//! Mirrors the structure of `src/routes/modal_tests.rs` (story 9-10).
//!
//! Cases per AC9:
//!   - basic empty render (no CTA, no role gate)
//!   - CTA rendering on `(label, url)` non-empty pair
//!   - CTA omission when label OR url is empty
//!   - librarian role-gate: hides for anonymous, shows for librarian + admin
//!   - admin role-gate: hides for librarian, shows for admin
//!   - body_html `|safe` passes raw markup through
//!   - data-status-message + data-variant stable selectors

use askama::Template;

#[derive(Template)]
#[template(path = "fragments/status_message_test_wrapper.html")]
struct StatusMessageTestWrapper {
    variant: &'static str,
    heading: &'static str,
    body_html: &'static str,
    cta_label: &'static str,
    cta_url: &'static str,
    cta_role_gate: &'static str,
    role: &'static str,
}

fn render(
    variant: &'static str,
    heading: &'static str,
    body_html: &'static str,
    cta_label: &'static str,
    cta_url: &'static str,
    cta_role_gate: &'static str,
    role: &'static str,
) -> String {
    StatusMessageTestWrapper {
        variant,
        heading,
        body_html,
        cta_label,
        cta_url,
        cta_role_gate,
        role,
    }
    .render()
    .expect("render")
}

/// Default-shaped render (no CTA, no role gate, anonymous role).
fn render_basic() -> String {
    render("empty", "No matches", "Try a broader term.", "", "", "", "anonymous")
}

#[test]
fn empty_variant_renders_heading_and_body() {
    let html = render_basic();
    assert!(
        html.contains("<h2"),
        "must render an <h2> heading; got: {html}"
    );
    assert!(
        html.contains("No matches"),
        "heading text must round-trip; got: {html}"
    );
    assert!(
        html.contains("<p"),
        "must render a <p> body; got: {html}"
    );
    assert!(
        html.contains("Try a broader term."),
        "body text must round-trip; got: {html}"
    );
}

#[test]
fn cta_renders_when_label_and_url_both_non_empty() {
    let html = render(
        "empty",
        "No series yet",
        "Create a series to organize your titles.",
        "Create a series",
        "/series/new",
        "",
        "anonymous",
    );
    assert!(
        html.contains(r#"href="/series/new""#),
        "CTA href must render verbatim when label+url both set; got: {html}"
    );
    assert!(
        html.contains("Create a series"),
        "CTA label must render; got: {html}"
    );
}

#[test]
fn cta_omitted_when_label_empty() {
    let html = render(
        "empty",
        "Heading",
        "Body",
        "",
        "/foo",
        "",
        "anonymous",
    );
    assert!(
        !html.contains("href=\"/foo\""),
        "empty cta_label must suppress the <a> element; got: {html}"
    );
}

#[test]
fn cta_omitted_when_url_empty() {
    let html = render(
        "empty",
        "Heading",
        "Body",
        "Some label",
        "",
        "",
        "anonymous",
    );
    assert!(
        !html.contains("Some label"),
        "empty cta_url must suppress the <a> element entirely (label disappears too); got: {html}"
    );
    assert!(
        !html.contains("<a "),
        "no <a> tag must appear when cta_url is empty; got: {html}"
    );
}

#[test]
fn cta_role_gate_librarian_hides_for_anonymous() {
    let html = render(
        "empty",
        "Heading",
        "Body",
        "Add",
        "/foo",
        "librarian",
        "anonymous",
    );
    assert!(
        !html.contains("href=\"/foo\""),
        "librarian-gated CTA must be hidden for anonymous role; got: {html}"
    );
}

#[test]
fn cta_role_gate_librarian_shows_for_librarian() {
    let html = render(
        "empty",
        "Heading",
        "Body",
        "Add",
        "/foo",
        "librarian",
        "librarian",
    );
    assert!(
        html.contains("href=\"/foo\""),
        "librarian-gated CTA must be visible for librarian role; got: {html}"
    );
}

#[test]
fn cta_role_gate_librarian_shows_for_admin() {
    // Admin > Librarian elevation: admin always passes a librarian gate.
    let html = render(
        "empty",
        "Heading",
        "Body",
        "Add",
        "/foo",
        "librarian",
        "admin",
    );
    assert!(
        html.contains("href=\"/foo\""),
        "librarian-gated CTA must be visible for admin role (admin > librarian); got: {html}"
    );
}

#[test]
fn cta_role_gate_admin_hides_for_librarian() {
    let html = render(
        "empty",
        "Heading",
        "Body",
        "Purge",
        "/admin/danger",
        "admin",
        "librarian",
    );
    assert!(
        !html.contains("href=\"/admin/danger\""),
        "admin-gated CTA must be hidden for librarian role; got: {html}"
    );
}

#[test]
fn cta_role_gate_admin_shows_for_admin() {
    let html = render(
        "empty",
        "Heading",
        "Body",
        "Purge",
        "/admin/danger",
        "admin",
        "admin",
    );
    assert!(
        html.contains("href=\"/admin/danger\""),
        "admin-gated CTA must be visible for admin role; got: {html}"
    );
}

#[test]
fn body_html_safe_passes_markup_through() {
    // `|safe` rendering means caller-controlled markup lands raw. Caller
    // is responsible for HTML-escaping any user-supplied interpolation
    // BEFORE constructing body_html (mirror of the modal macro contract).
    let html = render(
        "empty",
        "Heading",
        "<em>important</em>",
        "",
        "",
        "",
        "anonymous",
    );
    assert!(
        html.contains("<em>important</em>"),
        "body_html must render raw markup (not entity-escape); got: {html}"
    );
    assert!(
        !html.contains("&lt;em&gt;"),
        "body_html must NOT entity-escape (caller's responsibility); got: {html}"
    );
}

#[test]
fn data_attributes_are_stable_selectors() {
    let html = render_basic();
    assert!(
        html.contains("data-status-message"),
        "data-status-message attribute must be present (E2E selector); got: {html}"
    );
    assert!(
        html.contains(r#"data-variant="empty""#),
        "data-variant attribute must round-trip the variant name; got: {html}"
    );
}
