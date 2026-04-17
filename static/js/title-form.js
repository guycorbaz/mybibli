// title-form.js — required-field validation, type-specific fields lazy-load,
// cancel button, and Esc-key cancel for the title creation form.
//
// Loaded from base.html and idempotently re-wires after every HTMX swap so
// the form works whether it's present at page load OR inserted later by
// `htmx.ajax("GET", "/catalog/title/new", ...)`.
//
// Pre-CSP this lived as an inline <script> inside templates/components/title_form.html.
// Strict CSP (`script-src 'self'`) requires it to be an external module.
(function () {
    "use strict";

    var REQUIRED_IDS = ["title-field", "media-type-field", "genre-field", "language-field"];

    function isEmpty(el) {
        var v = el.value;
        if (el.tagName === "SELECT") {
            return v === "" || v === "0";
        }
        return v.trim() === "";
    }

    function showError(el) {
        var errP = el.parentElement && el.parentElement.querySelector(".field-error");
        if (!errP) return;
        var msg = el.getAttribute("data-required-error") || "Required";
        errP.textContent = msg;
        errP.classList.remove("hidden");
        el.classList.add("border-red-500");
    }

    function hideError(el) {
        var errP = el.parentElement && el.parentElement.querySelector(".field-error");
        if (!errP) return;
        errP.textContent = "";
        errP.classList.add("hidden");
        el.classList.remove("border-red-500");
    }

    function wireRequiredFields(form) {
        REQUIRED_IDS.forEach(function (id) {
            var el = form.querySelector("#" + id);
            if (!el || el.dataset.wired === "true") return;
            el.dataset.wired = "true";
            el.addEventListener("blur", function () {
                if (isEmpty(el)) showError(el);
            });
            el.addEventListener("input", function () { hideError(el); });
            el.addEventListener("change", function () { hideError(el); });
        });
        // Submit handler is registered ONCE at script load via document-level
        // delegation (see bottom of this module). Doing it here would race
        // with HTMX's own `hx-post` listener (HTMX wins, form submits before
        // our validation runs).
    }

    // Media-type dropdown triggers lazy-load of the type-specific fields
    // fragment via HTMX. Pre-CSP this was an inline onchange.
    function wireMediaTypeChange(form) {
        var sel = form.querySelector("#media-type-field");
        if (!sel || sel.dataset.htmxWired === "true") return;
        sel.dataset.htmxWired = "true";
        sel.addEventListener("change", function () {
            var fields = document.getElementById("type-specific-fields");
            if (!fields) return;
            if (sel.value && typeof htmx !== "undefined") {
                htmx.ajax("GET", "/catalog/title/fields/" + sel.value, {
                    target: "#type-specific-fields",
                    swap: "innerHTML",
                });
            } else {
                fields.innerHTML = "";
            }
        });
    }

    function wireCancelButton(form) {
        var btn = form.querySelector("#title-form-cancel");
        if (!btn || btn.dataset.wired === "true") return;
        btn.dataset.wired = "true";
        btn.addEventListener("click", function () {
            var container = document.getElementById("title-form-container");
            if (container) container.innerHTML = "";
            var scan = document.getElementById("scan-field");
            if (scan) scan.focus();
        });
    }

    function init() {
        var container = document.getElementById("title-form-container");
        var form = container && container.querySelector("form");
        if (!form) return;
        wireRequiredFields(form);
        wireMediaTypeChange(form);
        wireCancelButton(form);
    }

    // Document-level delegated submit guard. Registered ONCE, in the
    // capture phase, so it runs before HTMX's `hx-post` listener and can
    // call preventDefault() to block submission when required fields
    // are empty. Capture is essential — bubble-phase handlers race with
    // HTMX and lose under stricter timing.
    document.addEventListener(
        "submit",
        function (e) {
            var form = e.target;
            if (!form || form.id !== "title-create-form") return;
            var hasError = false;
            REQUIRED_IDS.forEach(function (id) {
                var el = form.querySelector("#" + id);
                if (el && isEmpty(el)) {
                    showError(el);
                    hasError = true;
                }
            });
            if (hasError) {
                e.preventDefault();
                e.stopImmediatePropagation();
            }
        },
        true,
    );

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
    // `htmx:load` fires synchronously after content is swapped into the
    // DOM, so per-field blur/input listeners get wired before the user
    // can interact with the freshly-loaded form.
    document.body.addEventListener("htmx:load", init);
})();
