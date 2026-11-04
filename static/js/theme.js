// Dark/light theme toggle with prefers-color-scheme detection and localStorage persistence
(function () {
    "use strict";

    var STORAGE_KEY = "mybibli-theme";

    function getPreferredTheme() {
        var stored = localStorage.getItem(STORAGE_KEY);
        if (stored) {
            return stored;
        }
        return window.matchMedia("(prefers-color-scheme: dark)").matches
            ? "dark"
            : "light";
    }

    function applyTheme(theme) {
        if (theme === "dark") {
            document.documentElement.classList.add("dark");
        } else {
            document.documentElement.classList.remove("dark");
        }
        // Story 14-1 (AC9) — the toggles keep their server-rendered,
        // localized name (`nav.theme_toggle`); the hard-coded English
        // aria-label this function used to set is gone.
    }

    function toggleTheme() {
        var current = document.documentElement.classList.contains("dark")
            ? "dark"
            : "light";
        var next = current === "dark" ? "light" : "dark";
        localStorage.setItem(STORAGE_KEY, next);

        // Smooth transition via class toggle (CSP-safe — no inline style).
        // Class lives in browse.css; honour prefers-reduced-motion.
        var prefersReducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
        if (!prefersReducedMotion) {
            document.documentElement.classList.add("theme-transitioning");
            setTimeout(function () {
                document.documentElement.classList.remove("theme-transitioning");
            }, 300);
        }
        applyTheme(next);
    }

    // Apply theme immediately to prevent flash
    applyTheme(getPreferredTheme());

    // Listen for system theme changes
    window
        .matchMedia("(prefers-color-scheme: dark)")
        .addEventListener("change", function (e) {
            if (!localStorage.getItem(STORAGE_KEY)) {
                applyTheme(e.matches ? "dark" : "light");
            }
        });

    // Wire every `[data-theme-toggle]` button (idempotent). Story 14-1 —
    // two exist: the sidebar's `#theme-toggle` (≥ lg) and the small-screen
    // bar's `#theme-toggle-bar`. Bare.html (login/logout) has no nav, so
    // none is present — wireToggle is a no-op there.
    function wireToggle() {
        var buttons = document.querySelectorAll("[data-theme-toggle]");
        for (var i = 0; i < buttons.length; i++) {
            var btn = buttons[i];
            if (btn.dataset.wired === "true") continue;
            btn.dataset.wired = "true";
            btn.addEventListener("click", toggleTheme);
        }
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", wireToggle);
    } else {
        wireToggle();
    }
})();
