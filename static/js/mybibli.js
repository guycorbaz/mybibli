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
                    entry.style.opacity = String(1 - (age - 10000) / 10000);
                    entry.style.transition = "opacity 0.5s";
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
            if (target) target.style.opacity = "1";

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
            if (target) target.style.opacity = "1";

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
            + '<button type="button" class="text-red-400 hover:text-red-600" onclick="this.closest(\'.feedback-entry\').remove()" aria-label="Dismiss">×</button>'
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

    function init() {
        initKeyboardShortcuts();
        initFeedbackAutoDismiss();
        initFormEscapeHandler();
        initAudioFeedback();
        initHtmxErrorRecovery();
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
