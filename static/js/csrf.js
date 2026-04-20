// Story 8-2 — CSRF synchronizer-token client plumbing.
//
// Exactly two listeners. No local i18n (the 403 body is server-rendered
// by `feedback_html` via rust_i18n). No window.* exports. Classic
// `<script src>` — matches the 6 existing JS modules' load convention.
(function () {
    "use strict";

    // Listener 1 — token injection. Covers every HTMX-driven mutation
    // (hx-post / hx-put / hx-patch / hx-delete) in the app, including
    // session-timeout.js's htmx.ajax() keep-alive.
    document.body.addEventListener("htmx:configRequest", function (evt) {
        var meta = document.querySelector('meta[name="csrf-token"]');
        // Defensive: if the meta tag is missing (should never happen —
        // base.html emits it on every page) or content is whitespace /
        // null, send the empty string so the middleware returns a clean
        // 403 instead of the browser throwing a TypeError or sending
        // whitespace that never matches the stored token.
        var raw = meta ? meta.getAttribute("content") : "";
        // Pass-2 review B-H2: some htmx.ajax() call sites may initialise
        // `evt.detail.headers` to null; guard against a silent TypeError
        // that would otherwise drop the request with no visible error.
        if (!evt.detail.headers) {
            evt.detail.headers = {};
        }
        evt.detail.headers["X-CSRF-Token"] = raw ? raw.trim() : "";
    });

    // Listener 2 — force-swap the 403 feedback body into the page.
    // Default HTMX behaviour on non-2xx is to discard the response
    // body; we opt in here so the user sees the server-rendered
    // "Session expired" FeedbackEntry without a full-page reload.
    // `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` (set by
    // the middleware) tell HTMX where to drop the fragment.
    //
    // Pass-2 review B-H3: `HX-Trigger` is a comma-separated list per
    // the HTMX spec; a future middleware that appends a second trigger
    // (`csrf-rejected, session-warn`) would silently break the
    // force-swap if compared with strict equality. Parse as a list.
    document.body.addEventListener("htmx:beforeSwap", function (evt) {
        var xhr = evt.detail.xhr;
        if (!xhr || xhr.status !== 403) {
            return;
        }
        var triggerHeader = xhr.getResponseHeader("HX-Trigger") || "";
        var hasCsrfRejected = triggerHeader
            .split(",")
            .map(function (s) { return s.trim(); })
            .indexOf("csrf-rejected") !== -1;
        if (hasCsrfRejected) {
            evt.detail.shouldSwap = true;
            evt.detail.isError = false;
        }
    });
})();
