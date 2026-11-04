// sidebar.js — collapsed / expanded state of the navigation sidebar
// (story 14-1, #547, UX-DR38).
//
// Loaded as a classic blocking script in <head> (like theme.js), so the
// remembered state is applied BEFORE first paint: it sets
// `sidebar-collapsed` on <html> and the `sb-collapsed:` Tailwind variant
// (static/css/input.css) does the rest — no flash of the wrong width, no
// inline script or style (CSP).
//
// State: one localStorage entry per browser (decided by the project lead,
// 2026-11-04), "collapsed" or "expanded"; anything else — absent, garbage,
// or storage blocked (private mode, disabled cookies: every access throws)
// — means expanded, the starting state.
//
// The toggle (`[data-sidebar-toggle]`, components/sidebar.html):
//   - flips the class and stores the new state;
//   - keeps `aria-expanded` in sync (its accessible name is fixed);
//   - AC8 (story 14-1 review, round 2) — a mouse click never takes the
//     focus (mousedown default prevented): whatever held it — the scan
//     field, a title or contributor form input — keeps it. From the
//     keyboard, focus goes back to `#scan-field` only if the scan field
//     held it right before the toggle (so the next scan lands there and a
//     scanner's Enter cannot re-toggle the sidebar); otherwise it stays on
//     the toggle. No feedback entry is touched.
//
// CSP-clean: no inline handlers, no eval, listeners via addEventListener.
(function () {
    "use strict";

    var STORAGE_KEY = "mybibli.sidebar";
    var COLLAPSED_CLASS = "sidebar-collapsed";
    var TOGGLE_SELECTOR = "[data-sidebar-toggle]";
    var SCAN_FIELD_ID = "scan-field";

    function readCollapsed() {
        try {
            return window.localStorage.getItem(STORAGE_KEY) === "collapsed";
        } catch (_) {
            return false;
        }
    }

    function writeCollapsed(collapsed) {
        try {
            window.localStorage.setItem(STORAGE_KEY, collapsed ? "collapsed" : "expanded");
        } catch (_) {
            // Blocked storage: the state lasts for this page only.
        }
    }

    function isCollapsed() {
        return document.documentElement.classList.contains(COLLAPSED_CLASS);
    }

    function apply(collapsed) {
        document.documentElement.classList.toggle(COLLAPSED_CLASS, collapsed);
        var toggles = document.querySelectorAll(TOGGLE_SELECTOR);
        for (var i = 0; i < toggles.length; i++) {
            toggles[i].setAttribute("aria-expanded", collapsed ? "false" : "true");
        }
    }

    // Before first paint: only <html> exists yet; the toggle is synced
    // once the document is parsed.
    apply(readCollapsed());

    // The element focused before the toggle took the focus (keyboard path).
    var lastFocused = null;
    var focusedBeforeToggle = null;
    document.addEventListener("focusin", function (e) {
        var t = e.target;
        if (t && t.matches && t.matches(TOGGLE_SELECTOR)) {
            focusedBeforeToggle = lastFocused;
        } else {
            lastFocused = t;
        }
    });

    function wire() {
        apply(isCollapsed());
        var toggles = document.querySelectorAll(TOGGLE_SELECTOR);
        for (var i = 0; i < toggles.length; i++) {
            var btn = toggles[i];
            if (btn.dataset.wired === "true") continue;
            btn.dataset.wired = "true";
            btn.addEventListener("mousedown", function (e) {
                e.preventDefault();
            });
            btn.addEventListener("click", function () {
                var next = !isCollapsed();
                apply(next);
                writeCollapsed(next);
                var scan = document.getElementById(SCAN_FIELD_ID);
                if (scan && document.activeElement === this && focusedBeforeToggle === scan) {
                    try { scan.focus({ preventScroll: true }); } catch (_) { /* ignore */ }
                }
            });
        }
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", wire);
    } else {
        wire();
    }
})();
