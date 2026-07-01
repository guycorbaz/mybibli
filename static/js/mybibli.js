// mybibli application entry point
(function () {
    "use strict";

    function initKeyboardShortcuts() {
        document.addEventListener("keydown", function (e) {
            var role = document.body.dataset.userRole;
            if (role !== "librarian" && role !== "admin") return;

            // Ctrl+K / Cmd+K → navigate to /catalog
            if ((e.ctrlKey || e.metaKey) && e.key === "k") {
                e.preventDefault();
                window.location.href = "/catalog";
                return;
            }

            // Ctrl+Shift+B → navigate to /borrowers
            if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === "B") {
                e.preventDefault();
                window.location.href = "/borrowers";
                return;
            }

            // Ctrl+N / Cmd+N → open title creation form (on /catalog only)
            if ((e.ctrlKey || e.metaKey) && e.key === "n") {
                if (window.location.pathname !== "/catalog") return;
                e.preventDefault();

                var container = document.getElementById("title-form-container");
                if (!container) return;

                // If form already open, don't reload
                if (container.innerHTML.trim()) return;

                if (typeof htmx !== "undefined") {
                    htmx.ajax("GET", "/catalog/title/new", {
                        target: "#title-form-container",
                        swap: "innerHTML",
                    });
                }
            }
        });
    }

    // Auto-dismiss feedback entries: success/info fade at 10s, remove at 20s
    // Skeletons (.feedback-skeleton) are excluded — they persist until replaced by OOB swap
    function initFeedbackAutoDismiss() {
        setInterval(function () {
            var entries = document.querySelectorAll(".feedback-entry");
            var now = Date.now();

            entries.forEach(function (entry) {
                var variant = entry.getAttribute("data-feedback-variant");
                if (variant !== "success" && variant !== "info") return;

                // For resolved entries delivered via OOB, use data-resolved-at as start time
                var created = entry.getAttribute("data-feedback-created");
                if (!created) {
                    var resolvedAt = entry.getAttribute("data-resolved-at");
                    if (resolvedAt) {
                        entry.setAttribute("data-feedback-created", resolvedAt);
                        created = resolvedAt;
                    } else {
                        entry.setAttribute("data-feedback-created", String(now));
                        return;
                    }
                }

                var age = now - parseInt(created, 10);
                if (age >= 20000) {
                    entry.remove();
                } else if (age >= 10000) {
                    // Class-based fade (CSS rule in browse.css). CSP strict
                    // mode blocks `entry.style.opacity = ...` writes, so the
                    // fade lives in CSS and JS only flips the trigger class.
                    entry.classList.add("feedback-fading");
                }
            });
        }, 1000);
    }

    // Escape key handler for title form
    function initFormEscapeHandler() {
        document.addEventListener("keydown", function (e) {
            if (e.key !== "Escape") return;

            var container = document.getElementById("title-form-container");
            if (!container || !container.innerHTML.trim()) return;

            e.preventDefault();
            container.innerHTML = "";

            var scanField = document.getElementById("scan-field");
            if (scanField) scanField.focus();
        });
    }

    // Audio integration: play tone when new feedback entries appear
    function initAudioFeedback() {
        var feedbackList = document.getElementById("feedback-list");
        if (!feedbackList || !window.mybibliAudio) return;

        var observer = new MutationObserver(function (mutations) {
            if (!window.mybibliAudio.isEnabled()) return;

            mutations.forEach(function (mutation) {
                mutation.addedNodes.forEach(function (node) {
                    if (node.nodeType !== 1) return;
                    var entry = node.classList && node.classList.contains("feedback-entry") ? node : node.querySelector && node.querySelector(".feedback-entry");
                    if (!entry) return;

                    var variant = entry.getAttribute("data-feedback-variant");
                    switch (variant) {
                        case "success": window.mybibliAudio.playSuccess(); break;
                        case "info": window.mybibliAudio.playInfo(); break;
                        case "warning": window.mybibliAudio.playWarning(); break;
                        case "error": window.mybibliAudio.playError(); break;
                    }
                });
            });
        });

        observer.observe(feedbackList, { childList: true });
    }

    // HTMX error recovery: restore UI state and show error feedback
    function initHtmxErrorRecovery() {
        // Scoped to catalog page only (search.js handles home page errors)
        if (!document.getElementById("feedback-list")) return;

        document.body.addEventListener("htmx:responseError", function (e) {
            if (!document.getElementById("feedback-list")) return;
            var target = e.detail.target;
            if (target) target.classList.add("htmx-opacity-reset");

            var status = e.detail.xhr ? e.detail.xhr.status : "unknown";
            // Issue #403 — template comes from the #i18n-bundle data island
            // (request locale, en/fr/de/it); %{status} substituted here.
            // Degrades to "" if the island is missing, same contract as
            // session-timeout.js.
            var template = "";
            try {
                var bundleEl = document.getElementById("i18n-bundle");
                var bundle = bundleEl ? JSON.parse(bundleEl.textContent) : {};
                template = (bundle.errors || {}).server_error_retry || "";
            } catch (err) {
                template = "";
            }
            var message = template.replace("%{status}", String(status));
            injectErrorFeedback(message);
            restoreScanField();
        });

        // Story 9-16 — REMOVED `htmx:sendError` listener that injected a
        // FeedbackEntry "Connection lost — check your network." into
        // `#feedback-list`. The new `static/js/connection-monitor.js`
        // module subsumes this surface with a full-viewport overlay +
        // automatic recovery polling. Without removal, a network drop on
        // the catalog page would surface 3 concurrent "Connection lost"
        // UIs (overlay + this FeedbackEntry + search.js's red banner).
    }

    function injectErrorFeedback(message) {
        var list = document.getElementById("feedback-list");
        if (!list) return;
        var escaped = message.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
        var html = '<div class="feedback-entry flex items-start gap-3 p-3 rounded-lg border-l-4 border-red-500 bg-red-50 dark:bg-red-950" data-feedback-variant="error" role="status">'
            + '<div class="flex-shrink-0 text-red-500"><svg class="w-5 h-5" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.707 7.293a1 1 0 00-1.414 1.414L8.586 10l-1.293 1.293a1 1 0 101.414 1.414L10 11.414l1.293 1.293a1 1 0 001.414-1.414L11.414 10l1.293-1.293a1 1 0 00-1.414-1.414L10 8.586 8.707 7.293z" clip-rule="evenodd" /></svg></div>'
            + '<div class="flex-1"><p class="text-sm font-medium text-red-800 dark:text-red-200">' + escaped + '</p></div>'
            + '<button type="button" class="text-red-400 hover:text-red-600" data-action="dismiss-feedback" aria-label="Dismiss">×</button>'
            + '</div>';
        list.insertAdjacentHTML("afterbegin", html);
    }

    function restoreScanField() {
        var scanField = document.getElementById("scan-field");
        if (!scanField) return;
        if (window.mybibliLastScanCode) {
            scanField.value = window.mybibliLastScanCode;
        }
        scanField.focus();
    }

    // Story 9-17 — the mobile-menu disclosure (basic click-to-toggle that
    // used to live here) is now owned end-to-end by `static/js/nav.js`,
    // which adds outside-click close, Escape, focus trap, link-click
    // close, and scanner-burst auto-close on top of the original toggle.

    // Delegated dismiss for feedback entries — works for templates AND for
    // the JS-injected error fragment in injectErrorFeedback() (which now
    // emits data-action="dismiss-feedback" instead of an inline onclick).
    // CSP blocks inline handlers even when the attribute was written by JS
    // post-load, so the listener has to live here.
    function initFeedbackDismiss() {
        document.addEventListener("click", function (e) {
            var btn = e.target.closest && e.target.closest("[data-action='dismiss-feedback']");
            if (!btn) return;
            var entry = btn.closest(".feedback-entry");
            if (entry) entry.remove();
        });
    }

    // polish-2 (#9): "Undo last scan action" affordance. The button posts to
    // /catalog/undo via HTMX (hx-post on the button, CSRF auto-injected by
    // csrf.js). This module only adds UX guards: (a) disable the button on
    // click so a double-tap can't fire two undos, and (b) remove the button
    // after the server-side 30-second window elapses, so a stale button can't
    // invite a guaranteed-rejected click. The server stays authoritative on
    // the window and on single-use semantics. CSP-clean — no inline handlers.
    function initScanUndo() {
        var list = document.getElementById("feedback-list");
        if (!list) return;

        document.addEventListener("click", function (e) {
            var btn = e.target.closest && e.target.closest("[data-action='undo-scan']");
            if (!btn) return;
            btn.disabled = true;
        });

        var UNDO_WINDOW_MS = 30000;
        var observer = new MutationObserver(function (mutations) {
            mutations.forEach(function (mutation) {
                mutation.addedNodes.forEach(function (node) {
                    if (node.nodeType !== 1) return;
                    var btn = (node.matches && node.matches("[data-action='undo-scan']"))
                        ? node
                        : (node.querySelector && node.querySelector("[data-action='undo-scan']"));
                    if (!btn) return;
                    setTimeout(function () {
                        if (btn && btn.parentNode) btn.remove();
                    }, UNDO_WINDOW_MS);
                });
            });
        });
        observer.observe(list, { childList: true });
    }

    // Story 8-5: System Settings → Metadata Providers form. Each row has a
    // text input and a "Clear this key on save" checkbox. When the checkbox
    // is checked, the sibling text input is disabled (and its value cleared)
    // so the admin sees that the field is intentionally being wiped.
    // CSP-clean — `data-action` delegated handler.
    function initProviderKeyClearToggle() {
        document.addEventListener("change", function (e) {
            var box = e.target.closest && e.target.closest("[data-action='provider-key-clear-toggle']");
            if (!box) return;
            var inputName = box.getAttribute("data-target-input");
            if (!inputName) return;
            var form = box.closest("form");
            if (!form) return;
            var input = form.querySelector("input[name='" + inputName + "']");
            if (!input) return;
            if (box.checked) {
                input.value = "";
                input.disabled = true;
            } else {
                input.disabled = false;
            }
        });
    }

    // Borrower-detail page: after a successful loan-return POST, reload so
    // the active-loans table reflects the change. The 1500ms delay leaves
    // time for the in-line success feedback to be seen before the reload.
    // Pre-CSP this was an inline <script> at the bottom of
    // borrower_detail.html — moved here behind a data-page guard.
    function initBorrowerDetailReload() {
        if (document.body.dataset.page !== "borrower-detail") return;
        document.body.addEventListener("htmx:afterRequest", function (e) {
            var detail = e.detail;
            if (!detail || !detail.pathInfo || !detail.pathInfo.requestPath) return;
            if (!detail.pathInfo.requestPath.includes("/return")) return;
            if (!detail.successful) return;
            setTimeout(function () { window.location.reload(); }, 1500);
        });
    }

    // Locations tree: ➕ buttons toggle the inline add-child form. The
    // form's id is carried on `data-locations-toggle`. Pre-CSP this was
    // an inline `onclick` written by `src/routes/locations.rs`. Delegated
    // because the tree is server-rendered and may be re-fetched via HTMX.
    function initLocationsTreeToggle() {
        document.body.addEventListener("click", function (e) {
            var btn = e.target.closest && e.target.closest("[data-locations-toggle]");
            if (!btn) return;
            var formId = btn.dataset.locationsToggle;
            if (!formId) return;
            var form = document.getElementById(formId);
            if (form) form.classList.toggle("hidden");
        });
    }

    // CR #275 / #276 (v1.7.0) — auto-submit `<select data-auto-submit>` on
    // change, so the language dropdown in the nav bar saves immediately
    // without a separate Submit button. Delegated at document level so
    // HTMX-injected forms work too. CSP-clean — no inline `onchange`
    // handler (which would be blocked by `script-src 'self'`). The
    // <noscript> fallback in the template surfaces a manual Submit button
    // for JS-disabled clients.
    function initAutoSubmitSelects() {
        document.body.addEventListener("change", function (e) {
            var sel = e.target;
            if (!sel || !sel.dataset || sel.dataset.autoSubmit !== "true") return;
            if (sel.form) sel.form.submit();
        });
    }

    // Title-detail page: omnibus checkbox toggles the end-position field.
    // Pre-CSP this was an inline `onchange="...style.display=..."`.
    function initOmnibusToggle() {
        var cb = document.getElementById("assign-omnibus");
        var grp = document.getElementById("end-position-group");
        if (!cb || !grp || cb.dataset.wired === "true") return;
        cb.dataset.wired = "true";
        cb.addEventListener("change", function () {
            grp.classList.toggle("hidden", !cb.checked);
        });
    }

    // Series form: type=closed reveals the total-count field. Pre-CSP this
    // was an inline `onchange="...style.display=..."`.
    function initSeriesTypeToggle() {
        var sel = document.getElementById("series-type");
        var grp = document.getElementById("total-count-group");
        var totalInput = document.getElementById("series-total");
        if (!sel || !grp || sel.dataset.wired === "true") return;
        sel.dataset.wired = "true";
        sel.addEventListener("change", function () {
            var isClosed = sel.value === "closed";
            grp.classList.toggle("hidden", !isClosed);
            if (!isClosed && totalInput) totalInput.value = "";
        });
    }

    // Esc inside the inline title-edit form clicks its cancel button (which
    // re-fetches the read-only metadata fragment via HTMX). Delegated at
    // body level because the form is HTMX-injected into #title-metadata.
    function initTitleEditFormEscape() {
        document.body.addEventListener("keydown", function (e) {
            if (e.key !== "Escape") return;
            var target = e.target;
            if (!target || !target.closest) return;
            if (!target.closest("#title-edit-form")) return;
            var cancel = document.getElementById("cancel-edit");
            if (cancel) cancel.click();
        });
    }

    // Strip the `htmx-opacity-reset` class on every new HTMX request so the
    // `.htmx-request` dimming can re-apply on subsequent requests. Without
    // this, the `!important` reset sticks forever after the first error
    // and the loading state never paints again on that target.
    function initOpacityResetCleanup() {
        document.body.addEventListener("htmx:beforeRequest", function (e) {
            var target = e.detail && e.detail.target;
            if (target && target.classList) target.classList.remove("htmx-opacity-reset");
        });
    }

    // Permanent delete modal: enable confirm button only when user types the correct item name.
    // Uses data-confirm-name and data-confirm-btn attributes instead of inline script.
    //
    // polish-1 AC1: when the input lives inside a UX-DR8 macro modal,
    // there's no stable id on the Confirm button — but `[data-modal-confirm]`
    // is always present. Fallback lookup: if `data-confirm-btn` id is
    // absent OR doesn't resolve, find `[data-modal-confirm]` inside the
    // nearest dialog ancestor.
    function findConfirmButton(input) {
        var btnId = input.dataset.confirmBtn;
        if (btnId) {
            var byId = document.getElementById(btnId);
            if (byId) return byId;
        }
        var dialog = input.closest("dialog");
        if (dialog) return dialog.querySelector("[data-modal-confirm]");
        return null;
    }
    function syncConfirmButtonDisabled(input) {
        var expectedName = input.dataset.confirmName;
        if (!expectedName) return;
        var btn = findConfirmButton(input);
        if (!btn) return;
        btn.disabled = input.value !== expectedName;
    }
    function initConfirmationNameValidation() {
        // Live updates on every keystroke.
        document.addEventListener("input", function (e) {
            var input = e.target.closest && e.target.closest("[data-confirm-name]");
            if (input) syncConfirmButtonDisabled(input);
        });
        // polish-1 AC1: pre-disable the Confirm button when a modal lands
        // in the DOM. The macro doesn't hardcode `disabled` on the
        // button — that responsibility shifts to this listener. Runs on
        // every HTMX swap so freshly-injected modals are caught.
        document.body.addEventListener("htmx:afterSwap", function () {
            var inputs = document.querySelectorAll("[data-confirm-name]");
            for (var i = 0; i < inputs.length; i++) {
                syncConfirmButtonDisabled(inputs[i]);
            }
        });
    }

    function init() {
        initKeyboardShortcuts();
        initFeedbackAutoDismiss();
        initFormEscapeHandler();
        initAudioFeedback();
        initHtmxErrorRecovery();
        initFeedbackDismiss();
        initScanUndo();
        initProviderKeyClearToggle();
        initBorrowerDetailReload();
        initTitleEditFormEscape();
        initOmnibusToggle();
        initSeriesTypeToggle();
        initLocationsTreeToggle();
        initConfirmationNameValidation();
        initOpacityResetCleanup();
        initAutoSubmitSelects();
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
