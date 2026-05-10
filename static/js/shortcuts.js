// shortcuts.js — keyboard shortcuts + cheat-sheet dialog (story 9-20).
//
// Adds:
//   - `?` key → opens the cheat-sheet <dialog id="shortcuts-cheat-sheet">
//     (skips when the user is typing in a text input).
//   - g-chord navigation (g-h, g-c, g-l, g-b, g-a) with 800ms timeout.
//     Role-gated per-shortcut via document.body.dataset.userRole.
//   - footer "Press ? for shortcuts" link [data-shortcuts-help-link]
//     opens the same dialog on click.
//
// Native <dialog>.showModal() handles top-layer rendering, focus trap,
// and Escape close (the `cancel` event). We add backdrop-click close in
// ~3 LOC (native does NOT close on backdrop click by default).
//
// Existing Ctrl+K / Ctrl+Shift+B / Ctrl+N shortcuts in mybibli.js are
// UNCHANGED — they're listed in the cheat sheet for discoverability.
//
// CSP-clean: no inline handlers, no eval, all listeners via
// addEventListener.
(function () {
    "use strict";

    var DIALOG_ID = "shortcuts-cheat-sheet";
    var FOOTER_LINK_SELECTOR = "[data-shortcuts-help-link]";
    var CHORD_TIMEOUT_MS = 800;

    var TEXT_INPUT_TYPES = {
        "": true, text: true, search: true, email: true, url: true,
        tel: true, password: true, number: true,
    };

    function isTextInput(el) {
        if (!el || el.nodeType !== 1) return false;
        if (el.isContentEditable) return true;
        var tag = el.tagName;
        if (tag === "TEXTAREA") return true;
        if (tag === "INPUT") {
            var type = (el.getAttribute("type") || "").toLowerCase();
            return TEXT_INPUT_TYPES[type] === true;
        }
        return false;
    }

    var state = {
        chordPending: false,
        chordTimer: null,
    };

    function getRole() {
        return (document.body && document.body.dataset && document.body.dataset.userRole) || "anonymous";
    }

    function openCheatSheet() {
        var dialog = document.getElementById(DIALOG_ID);
        if (!dialog) return;
        if (typeof dialog.showModal === "function" && !dialog.open) {
            try { dialog.showModal(); } catch (_) { /* ignore */ }
        }
    }

    function cancelChord() {
        state.chordPending = false;
        if (state.chordTimer !== null) {
            clearTimeout(state.chordTimer);
            state.chordTimer = null;
        }
    }

    function handleChordSecond(key) {
        var role = getRole();
        var path = null;
        switch (key) {
            case "h": path = "/"; break;
            case "c": path = "/catalog"; break;
            case "l": if (role !== "anonymous") path = "/loans"; break;
            case "b": if (role !== "anonymous") path = "/borrowers"; break;
            case "a": if (role === "admin") path = "/admin"; break;
            default: break;
        }
        cancelChord();
        if (path && path !== window.location.pathname) {
            window.location = path;
        }
    }

    function init() {
        var dialog = document.getElementById(DIALOG_ID);
        if (!dialog) return;
        if (dialog.dataset.wired === "true") return;
        dialog.dataset.wired = "true";

        // Backdrop-click close — native <dialog> does NOT close on
        // backdrop click. When the click target IS the dialog itself
        // (not a descendant), the user clicked outside the form's
        // bounding rect. Close.
        dialog.addEventListener("click", function (e) {
            if (e.target === dialog) {
                try { dialog.close(); } catch (_) { /* ignore */ }
            }
        });

        // Footer-link click → open dialog. Delegated so HTMX-injected
        // re-renders are covered. Same delegate also handles the
        // dialog's Close button via [data-cheat-sheet-close]. We use
        // <button type="button"> + JS instead of <form method="dialog">
        // + <button type="submit"> to avoid polluting `form
        // button[type="submit"]` selectors on every page.
        document.addEventListener("click", function (e) {
            if (!e.target || !e.target.closest) return;
            if (e.target.closest(FOOTER_LINK_SELECTOR)) {
                e.preventDefault();
                openCheatSheet();
                return;
            }
            if (e.target.closest("[data-cheat-sheet-close]")) {
                var dialog = document.getElementById(DIALOG_ID);
                if (dialog && dialog.open) {
                    try { dialog.close(); } catch (_) { /* ignore */ }
                }
            }
        });

        // Keydown listener for `?` and g-chord. Skips when typing in
        // a text input (so `?` typed in a search box stays as text).
        document.addEventListener("keydown", function (e) {
            if (e.ctrlKey || e.metaKey || e.altKey) {
                cancelChord();
                return;
            }
            if (isTextInput(document.activeElement)) {
                cancelChord();
                return;
            }
            if (e.key === "?") {
                e.preventDefault();
                cancelChord();
                openCheatSheet();
                return;
            }
            // Ignore non-printable / multi-char keys outside our chord
            // alphabet (Tab, Shift, ArrowDown, etc.). Don't cancel the
            // chord on these — a user pressing g then looking around
            // before typing the second char shouldn't lose the chord.
            if (!e.key || e.key.length !== 1) {
                return;
            }
            if (state.chordPending) {
                e.preventDefault();
                handleChordSecond(e.key);
                return;
            }
            if (e.key === "g") {
                e.preventDefault();
                state.chordPending = true;
                state.chordTimer = setTimeout(cancelChord, CHORD_TIMEOUT_MS);
                return;
            }
            // Any other printable char while no chord is pending → noop.
        });
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
