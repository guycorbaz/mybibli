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

    function init() {
        initKeyboardShortcuts();
        initFeedbackAutoDismiss();
        initFormEscapeHandler();
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
