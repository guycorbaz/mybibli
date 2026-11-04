// Dark/light theme toggle with prefers-color-scheme detection and localStorage persistence
(function () {
    "use strict";

    var STORAGE_KEY = "mybibli-theme";

    // Story 14-1 review — every storage access is guarded: with storage
    // blocked (private mode, disabled cookies) `localStorage` throws, which
    // used to abort this script before the toggles were wired. Blocked
    // storage = no stored choice: the OS preference applies, and the
    // toggle still works for the page.
    function readStored() {
        try {
            return window.localStorage.getItem(STORAGE_KEY);
        } catch (_) {
            return null;
        }
    }

    function writeStored(theme) {
        try {
            window.localStorage.setItem(STORAGE_KEY, theme);
        } catch (_) {
            // Blocked storage: the choice lasts for this page only.
        }
    }

    function getPreferredTheme() {
        var stored = readStored();
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
        writeStored(next);

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

    // Apply theme immediately to prevent flash. Story 14-1 review — the
    // `dark:` utilities follow this class (input.css `@custom-variant
    // dark`), so with nothing stored the OS preference is applied here.
    applyTheme(getPreferredTheme());

    // Listen for system theme changes
    window
        .matchMedia("(prefers-color-scheme: dark)")
        .addEventListener("change", function (e) {
            if (!readStored()) {
                applyTheme(e.matches ? "dark" : "light");
            }
        });

    // Wire every `[data-theme-toggle]` button (idempotent). Since story
    // 14-2 there is one, the sidebar's `#theme-toggle` (the sidebar is the
    // drawer below lg). Bare.html (login/logout) has no nav, so none is
    // present — wireToggle is a no-op there.
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
