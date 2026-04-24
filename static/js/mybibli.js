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
            var message = document.documentElement.lang === "fr"
                ? "Erreur serveur (" + status + ") — veuillez réessayer."
                : "Server error (" + status + ") — please try again.";
            injectErrorFeedback(message);
            restoreScanField();
        });

        document.body.addEventListener("htmx:sendError", function (e) {
            if (!document.getElementById("feedback-list")) return;
            var target = e.detail.target;
            if (target) target.classList.add("htmx-opacity-reset");

            var message = document.documentElement.lang === "fr"
                ? "Connexion perdue — vérifiez votre réseau."
                : "Connection lost — check your network.";
            injectErrorFeedback(message);
            restoreScanField();
        });
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

    // Mobile hamburger button toggles #mobile-nav visibility + ARIA state.
    // Pre-CSP this lived as an inline onclick on #mobile-menu-toggle.
    function initMobileMenuToggle() {
        var btn = document.getElementById("mobile-menu-toggle");
        var menu = document.getElementById("mobile-nav");
        if (!btn || !menu) return;
        btn.addEventListener("click", function () {
            var nowHidden = menu.classList.toggle("hidden");
            btn.setAttribute("aria-expanded", String(!nowHidden));
        });
    }

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
    function initConfirmationNameValidation() {
        document.addEventListener("input", function (e) {
            var input = e.target.closest && e.target.closest("[data-confirm-name]");
            if (!input) return;
            var expectedName = input.dataset.confirmName;
            var btnId = input.dataset.confirmBtn;
            if (!expectedName || !btnId) return;
            var btn = document.getElementById(btnId);
            if (!btn) return;
            btn.disabled = input.value !== expectedName;
        });
    }

    function init() {
        initKeyboardShortcuts();
        initFeedbackAutoDismiss();
        initFormEscapeHandler();
        initAudioFeedback();
        initHtmxErrorRecovery();
        initMobileMenuToggle();
        initFeedbackDismiss();
        initBorrowerDetailReload();
        initTitleEditFormEscape();
        initOmnibusToggle();
        initSeriesTypeToggle();
        initLocationsTreeToggle();
        initConfirmationNameValidation();
        initOpacityResetCleanup();
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
