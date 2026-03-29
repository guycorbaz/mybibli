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
    }

    function toggleTheme() {
        var current = document.documentElement.classList.contains("dark")
            ? "dark"
            : "light";
        var next = current === "dark" ? "light" : "dark";
        localStorage.setItem(STORAGE_KEY, next);
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

    // Expose toggle for UI buttons
    window.mybibliToggleTheme = toggleTheme;
})();
