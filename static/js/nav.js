// nav.js — the small-screen navigation drawer (story 14-2, #547, UX-DR39).
//
// Below `lg` (1024 px) the sidebar's `<nav id="sidebar">`
// (templates/components/sidebar.html) is an off-canvas drawer; the menu
// button `#mobile-menu-toggle` of the sticky bar
// (templates/components/nav_bar.html) opens it. Supersedes the story 9-17
// dropdown panel (`#mobile-nav`) and its stand-alone burst detector.
//
// Open:
//   - `nav-drawer-open` on <html>: CSS slides the drawer in, shows the
//     scrim (`nav-open:` variant, static/css/input.css) and stops the page
//     behind from scrolling; `motion-safe:` only, so prefers-reduced-motion
//     gets no animation;
//   - the `#nav-drawer` wrapper becomes `role="dialog"` +
//     `aria-modal="true"` + its accessible name (`data-dialog-label`):
//     scanner-guard.js then treats it as a modal surface — a USB scanner
//     burst neither reaches `#scan-field` nor activates a drawer control;
//   - `#page-column` (bar, <main>, footer) becomes `inert`, and Tab /
//     Shift+Tab cycle inside the drawer (the skip link and the modal
//     slots, outside the column, are still reachable otherwise);
//   - focus moves to the close button.
// Close — Escape, a tap on the scrim, the close button, following a link
// in the drawer, or the viewport growing to `lg`: everything above is
// undone and focus returns to the menu button, except when a link was
// followed (the page is navigating) or the viewport crossed to `lg` (the
// button is no longer displayed). Story 14-2 review — the drawer also
// closes when the session-timeout toast or the connection-lost overlay
// appears, so they are never stuck behind an inert page / a focus trap
// (focus goes back to the menu button only if it was in the drawer).
//
// Keyboard operation while open. scanner-guard.js blocks every printable
// key and Enter outside text inputs while a modal is open — also a human
// Enter on a link, Space on a button, or a key choosing a language in the
// <select>. A drawer that cannot be driven from a keyboard would fail
// WCAG 2.1.1, so a window-capture listener (runs before the guard's
// document-capture one) stops the propagation of such a key when it came
// at human pace — at least the burst threshold after the previous
// keydown — on a drawer link / button (Enter, Space) or on the language
// <select> (Enter, Space, a printable key for type-ahead). The guard never
// sees it and the browser's own default runs (story 14-2 review: no
// synthetic click, so Space cannot activate a button twice). A scanner's
// characters and terminating Enter follow one another within a few ms,
// so they still reach the guard and are blocked. Known limit, as for
// every burst detector here: the FIRST key of a burst has no predecessor
// and passes; on the select it can only match an option's first letter
// (F / E / D / I), which no ISBN / V- / L-code starts with.
//
// Threshold source: `<body data-scanner-threshold>` (the admin's
// `scanner_burst_threshold_ms`, layouts/base.html), else 50 ms, floor
// 10 ms (fix #147).
//
// CSP-clean: no inline handlers, no eval, all listeners via addEventListener.
(function () {
    "use strict";

    var TOGGLE_ID = "mobile-menu-toggle";
    var WRAPPER_ID = "nav-drawer";
    var NAV_ID = "sidebar";
    var SCRIM_ID = "nav-scrim";
    var CLOSE_ID = "nav-drawer-close";
    var COLUMN_ID = "page-column";
    var OPEN_CLASS = "nav-drawer-open";
    var WIDE_QUERY = "(min-width: 1024px)";
    var OVERLAY_ID = "connection-lost-overlay";
    var SESSION_TOAST_ID = "session-timeout-toast";
    var DEFAULT_BURST_THRESHOLD_MS = 50;
    var MIN_BURST_THRESHOLD_MS = 10;

    // Another modal above the drawer (UX-DR8 dialog, connection-lost
    // overlay) owns Escape and Tab while it is open.
    var MODAL_SELECTOR = 'dialog[open], [aria-modal="true"]';

    // Mirror of modal.js's FOCUSABLE_SELECTOR.
    var FOCUSABLE_SELECTOR = [
        'a[href]:not([tabindex="-1"])',
        "button:not([disabled])",
        'input:not([disabled]):not([type="hidden"])',
        "select:not([disabled])",
        "textarea:not([disabled])",
        '[tabindex]:not([tabindex="-1"])',
    ].join(", ");

    function getBurstThresholdMs() {
        var raw = document.body && document.body.getAttribute("data-scanner-threshold");
        var n = parseInt(raw || "", 10);
        if (!isNaN(n) && n >= MIN_BURST_THRESHOLD_MS) return n;
        return DEFAULT_BURST_THRESHOLD_MS;
    }

    function init() {
        var btn = document.getElementById(TOGGLE_ID);
        var wrapper = document.getElementById(WRAPPER_ID);
        var nav = document.getElementById(NAV_ID);
        if (!btn || !wrapper || !nav) return;
        if (btn.dataset.wired === "true") return;
        btn.dataset.wired = "true";

        var scrim = document.getElementById(SCRIM_ID);
        var closeBtn = document.getElementById(CLOSE_ID);
        var column = document.getElementById(COLUMN_ID);
        var root = document.documentElement;
        var wide = window.matchMedia ? window.matchMedia(WIDE_QUERY) : null;
        var burstThresholdMs = getBurstThresholdMs();
        var isOpen = false;
        var lastKeydownAt = 0;

        function focusableInside() {
            return Array.prototype.slice
                .call(nav.querySelectorAll(FOCUSABLE_SELECTOR))
                .filter(function (e) { return e.offsetParent !== null; });
        }

        function otherModalOpen() {
            var all = document.querySelectorAll(MODAL_SELECTOR);
            for (var i = 0; i < all.length; i++) {
                if (all[i] !== wrapper) return true;
            }
            return false;
        }

        function open() {
            if (isOpen) return;
            if (wide && wide.matches) return;
            isOpen = true;
            root.classList.add(OPEN_CLASS);
            wrapper.setAttribute("role", "dialog");
            wrapper.setAttribute("aria-modal", "true");
            wrapper.setAttribute("aria-label", wrapper.getAttribute("data-dialog-label") || "");
            if (column) column.inert = true;
            btn.setAttribute("aria-expanded", "true");
            var target = closeBtn || focusableInside()[0];
            if (target) {
                try { target.focus(); } catch (_) { /* ignore */ }
            }
        }

        // `restoreFocus`: true, false, or "if-inside" (only when the focus
        // is still in the drawer — another surface may have taken it).
        function close(restoreFocus) {
            if (!isOpen) return;
            if (restoreFocus === "if-inside") {
                restoreFocus = nav.contains(document.activeElement);
            }
            isOpen = false;
            root.classList.remove(OPEN_CLASS);
            wrapper.removeAttribute("role");
            wrapper.removeAttribute("aria-modal");
            wrapper.removeAttribute("aria-label");
            if (column) column.inert = false;
            btn.setAttribute("aria-expanded", "false");
            if (restoreFocus && btn.offsetParent !== null) {
                try { btn.focus(); } catch (_) { /* ignore */ }
            }
        }

        btn.addEventListener("click", function () {
            if (isOpen) close(true);
            else open();
        });
        if (closeBtn) {
            closeBtn.addEventListener("click", function () { close(true); });
        }
        if (scrim) {
            scrim.addEventListener("click", function () { close(true); });
        }

        // Following a link closes the drawer (also a Ctrl-click opening a
        // new tab: a re-open must be intentional). No focus return: the
        // page is navigating.
        nav.addEventListener("click", function (e) {
            if (!isOpen) return;
            var link = e.target && e.target.closest && e.target.closest("a[href]");
            if (link && nav.contains(link)) close(false);
        });

        // Growing to `lg` turns the drawer back into the sidebar.
        if (wide) {
            var onWide = function (e) { if (e.matches) close(false); };
            if (wide.addEventListener) wide.addEventListener("change", onWide);
            else if (wide.addListener) wide.addListener(onWide);
        }

        // A surface that must stay reachable appears: close the drawer.
        function interrupting() {
            var overlay = document.getElementById(OVERLAY_ID);
            return (
                !!document.getElementById(SESSION_TOAST_ID) ||
                (!!overlay && !overlay.classList.contains("hidden"))
            );
        }
        if (typeof MutationObserver === "function") {
            new MutationObserver(function () {
                if (isOpen && interrupting()) close("if-inside");
            }).observe(document.body, {
                childList: true,
                subtree: true,
                attributes: true,
                attributeFilter: ["class"],
            });
        }

        // Escape + focus trap.
        document.addEventListener("keydown", function (e) {
            if (!isOpen || otherModalOpen()) return;
            if (e.key === "Escape") {
                e.preventDefault();
                close(true);
                return;
            }
            if (e.key !== "Tab") return;
            var items = focusableInside();
            if (items.length === 0) return;
            var first = items[0];
            var last = items[items.length - 1];
            var active = document.activeElement;
            if (e.shiftKey) {
                if (active === first || !nav.contains(active)) {
                    e.preventDefault();
                    last.focus();
                }
            } else if (active === last || !nav.contains(active)) {
                e.preventDefault();
                first.focus();
            }
        });

        // Human-paced keys on a drawer control (see header): keep them
        // away from scanner-guard so the browser's default runs.
        window.addEventListener("keydown", function (e) {
            var now = Date.now();
            var delta = now - lastKeydownAt;
            lastKeydownAt = now;
            if (!isOpen || otherModalOpen()) return;
            if (e.ctrlKey || e.metaKey || e.altKey) return;
            if (delta < burstThresholdMs) return;
            var t = e.target;
            if (!t || t.nodeType !== 1 || !nav.contains(t)) return;
            var activates = e.key === "Enter" || e.key === " ";
            var printable = !!e.key && e.key.length === 1;
            var ok =
                (t.matches("a[href]") && e.key === "Enter") ||
                (t.matches("button:not([disabled])") && activates) ||
                (t.matches("select:not([disabled])") && (activates || printable));
            if (ok) e.stopPropagation();
        }, true);
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
