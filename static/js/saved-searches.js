// saved-searches.js — CR #367 home search-bar dropdown toggle.
//
// Pure delegated event handlers, CSP-clean (no inline handlers/styles/globals).
// The dropdown content (save-form + list) is server-rendered by routes::home
// and refreshed via HTMX OOB/target swaps; this module only owns the
// open/close UI affordance:
//   - click on [data-action="toggle-saved-searches"] toggles the panel
//   - click outside the control closes it
//   - Escape closes it (unless a modal is open — modal.js owns Escape then)
//
// Rename/delete go through UX-DR8 modals (#modal-slot + modal.js), so this
// module deliberately does NOT manage them.
(function () {
    "use strict";

    var CONTROL_ID = "saved-searches-control";
    var PANEL_ID = "saved-searches-panel";
    var TOGGLE_ID = "saved-searches-toggle";

    function panel() {
        return document.getElementById(PANEL_ID);
    }
    function toggleBtn() {
        return document.getElementById(TOGGLE_ID);
    }

    function isOpen() {
        var p = panel();
        return p != null && !p.classList.contains("hidden");
    }

    function open() {
        var p = panel();
        if (!p) return;
        p.classList.remove("hidden");
        var btn = toggleBtn();
        if (btn) btn.setAttribute("aria-expanded", "true");
    }

    function close() {
        var p = panel();
        if (!p) return;
        p.classList.add("hidden");
        var btn = toggleBtn();
        if (btn) btn.setAttribute("aria-expanded", "false");
    }

    document.addEventListener("click", function (evt) {
        var t = evt.target;
        if (!t || !t.closest) return;

        // Toggle button (or any child) → flip the panel.
        if (t.closest('[data-action="toggle-saved-searches"]')) {
            evt.preventDefault();
            if (isOpen()) {
                close();
            } else {
                open();
            }
            return;
        }

        // Click outside the whole control → close. But ignore clicks inside
        // a modal (rename/delete dialogs live in #modal-slot, outside the
        // control) so the dropdown stays open behind the modal.
        if (t.closest("#modal-slot") || t.closest("dialog")) return;
        if (isOpen() && !t.closest("#" + CONTROL_ID)) {
            close();
        }
    });

    document.addEventListener("keydown", function (evt) {
        if (evt.key !== "Escape") return;
        // Defer to modal.js when a modal is open (it owns Escape then).
        if (document.querySelector('dialog[open], [aria-modal="true"]')) return;
        if (isOpen()) {
            close();
            var btn = toggleBtn();
            if (btn) btn.focus();
        }
    });
})();
