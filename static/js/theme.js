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
        // Update theme toggle button aria-label (no-op on bare.html where the
        // button doesn't exist — early-return keeps the file CSP-safe).
        var btn = document.getElementById("theme-toggle");
        if (btn) {
            btn.setAttribute(
                "aria-label",
                theme === "dark" ? "Switch to light mode" : "Switch to dark mode"
            );
        }
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

    // Wire #theme-toggle button (idempotent). Bare.html (login/logout) has
    // no nav, so the button is absent — wireToggle is a no-op there.
    function wireToggle() {
        var btn = document.getElementById("theme-toggle");
        if (!btn || btn.dataset.wired === "true") return;
        btn.dataset.wired = "true";
        btn.addEventListener("click", toggleTheme);
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", wireToggle);
    } else {
        wireToggle();
    }
})();
