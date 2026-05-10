// tooltip.js — contextual-help tooltip controller (story 9-19).
//
// Toggles the visibility of `<span role="tooltip">` elements paired with
// help-icon buttons (`<button data-tooltip-trigger="...">`) per the
// markup contract from `templates/components/tooltip.html`.
//
// Activation modes:
//   - Hover (mouse only — gated via matchMedia('(hover: hover)') so touch
//     devices don't fire spurious mouseenter on tap).
//   - Focus (always — keyboard-a11y contract).
//   - Click / tap (toggles; on touch it's the primary path).
//   - Escape (closes if focus-shown; restores focus to the trigger).
//   - Outside-click mousedown (closes any open tooltip).
//
// Invariant: at most ONE tooltip open at a time. Opening tooltip B closes
// the currently-open A.
//
// CSP-clean: no inline handlers, no eval, all listeners via
// addEventListener.
//
// `prefers-reduced-motion` honored via the Tailwind `motion-safe:transition-
// opacity` class on the tooltip span itself; the `class="hidden"` toggle
// is instant in both modes.
//
// Mirror of `static/js/nav.js`'s IIFE shape + `dataset.wired` idempotency.
(function () {
    "use strict";

    var TRIGGER_SELECTOR = "[data-tooltip-trigger]";
    // Mirror of scanner-guard.js's MODAL_SELECTOR. When a UX-DR8 modal is
    // open, tooltip.js's document-level Escape handler must defer to
    // modal.js — pressing Escape on a tooltip-inside-modal must close
    // ONLY the topmost surface (the modal), not both.
    var MODAL_SELECTOR = 'dialog[open], [aria-modal="true"]';

    function isModalOpen() {
        return document.querySelector(MODAL_SELECTOR) !== null;
    }

    // Closure-scoped state. Only one tooltip span open at a time across
    // the whole page; opening a different help icon closes the previous
    // tooltip first.
    var state = {
        openTriggerEl: null,    // the <button> currently displaying its tooltip
        openTooltipEl: null,    // the matching <span role="tooltip">
        openShownByFocus: false, // true when focus-shown — Escape restores focus
        // P1 fix (story 9-19 code review): suppress the next focus auto-
        // show when a mousedown just occurred on the trigger. Without
        // this, a desktop mouse click sequence is: mousedown → focus
        // (auto-show) → click (toggle → close), producing a visible
        // flash. With this flag, the focus from a mouse-click is
        // suppressed; the click handler then shows the tooltip cleanly.
        // Keyboard tab → focus has no preceding mousedown, so the flag
        // stays false and focus auto-show works normally.
        suppressNextFocusShow: false,
    };

    // Detect whether the primary input device supports hover. `false` on
    // pure-touch devices (phones, tablets without trackpad). When false,
    // hover events are ignored and tap is the only show-path.
    var hoverCapable = typeof window.matchMedia === "function" &&
        window.matchMedia("(hover: hover)").matches;

    function getTooltip(triggerEl) {
        var id = triggerEl.dataset.tooltipTrigger;
        if (!id) return null;
        return document.getElementById(id);
    }

    function show(triggerEl, byFocus) {
        var tooltip = getTooltip(triggerEl);
        if (!tooltip) return;
        // One-at-a-time invariant: close any other open tooltip first.
        if (state.openTooltipEl && state.openTooltipEl !== tooltip) {
            hide();
        }
        tooltip.classList.remove("hidden");
        state.openTriggerEl = triggerEl;
        state.openTooltipEl = tooltip;
        state.openShownByFocus = !!byFocus;
    }

    function hide() {
        if (!state.openTooltipEl) return;
        state.openTooltipEl.classList.add("hidden");
        state.openTriggerEl = null;
        state.openTooltipEl = null;
        state.openShownByFocus = false;
    }

    function wireTrigger(triggerEl) {
        if (triggerEl.dataset.wired === "true") return;
        triggerEl.dataset.wired = "true";

        // Hover (mouse): gated by matchMedia. mouseenter shows;
        // mouseleave hides UNLESS the tooltip is focus-shown (the user
        // moved focus to the icon while the mouse was elsewhere).
        if (hoverCapable) {
            triggerEl.addEventListener("mouseenter", function () {
                show(triggerEl, false);
            });
            triggerEl.addEventListener("mouseleave", function () {
                if (state.openShownByFocus) return;
                if (state.openTriggerEl === triggerEl) hide();
            });
        }

        // Mousedown sets the suppressNextFocusShow flag so the focus
        // event that browsers fire on click-to-focus does NOT auto-show
        // the tooltip (the click handler will show it instead, cleanly).
        // Keyboard tab → focus path skips this flag (no mousedown
        // happened) and auto-shows normally.
        triggerEl.addEventListener("mousedown", function () {
            state.suppressNextFocusShow = true;
        });

        // Focus (keyboard): always show. focusout hides.
        triggerEl.addEventListener("focus", function () {
            if (state.suppressNextFocusShow) {
                state.suppressNextFocusShow = false;
                return;
            }
            show(triggerEl, true);
        });
        triggerEl.addEventListener("blur", function () {
            if (state.openTriggerEl === triggerEl) hide();
        });

        // Click / tap: toggle. On touch this is the primary path.
        triggerEl.addEventListener("click", function (e) {
            e.preventDefault();
            // Defensive: clear the suppress flag on click so a future
            // keyboard focus (Tab away then Tab back) auto-shows.
            state.suppressNextFocusShow = false;
            if (state.openTriggerEl === triggerEl) {
                hide();
            } else {
                show(triggerEl, false);
            }
        });
    }

    function init() {
        // Wire all CURRENT triggers. HTMX-injected fragments may add new
        // triggers later — see `htmx:afterSwap` listener below.
        var triggers = document.querySelectorAll(TRIGGER_SELECTOR);
        for (var i = 0; i < triggers.length; i++) {
            wireTrigger(triggers[i]);
        }

        // Document-level Escape close. If a tooltip is open AND it was
        // focus-shown, close + restore focus to the trigger (mirrors
        // modal.js's Escape pattern).
        //
        // P3 fix (story 9-19 code review): defer to modal.js when a
        // UX-DR8 modal is open. modal.js's onKeydown also listens at
        // document level and does NOT stopPropagation; without this
        // gate, a tooltip-inside-modal would close BOTH surfaces on a
        // single Escape. Mirror of nav.js's isModalOpen() gate.
        document.addEventListener("keydown", function (e) {
            if (e.key !== "Escape") return;
            if (!state.openTooltipEl) return;
            if (isModalOpen()) return;
            var triggerToRefocus = state.openShownByFocus ? state.openTriggerEl : null;
            hide();
            if (triggerToRefocus && document.contains(triggerToRefocus)) {
                try { triggerToRefocus.focus(); } catch (_) { /* ignore */ }
            }
        });

        // Document-level outside-click close. mousedown (not click) so a
        // drag-from-inside-to-outside (text selection) doesn't count as a
        // backdrop click. Mirror of modal.js's mousedown gate.
        document.addEventListener("mousedown", function (e) {
            if (!state.openTooltipEl) return;
            var t = e.target;
            if (!t) {
                hide();
                return;
            }
            if (state.openTriggerEl && state.openTriggerEl.contains(t)) return;
            if (state.openTooltipEl.contains(t)) return;
            hide();
        });

        // HTMX-injected fragments: re-scan for new triggers after every
        // successful swap. Idempotent via `dataset.wired`. Without this,
        // tooltips inside HTMX-loaded forms (e.g., admin/system tabs,
        // setup wizard steps) would be inert.
        //
        // P2 fix (story 9-19 code review): if the panel that contained
        // the open tooltip was just swapped out, state.openTooltipEl
        // references a detached node. Reset state so subsequent
        // mousedown / Escape don't operate on dead references.
        if (document.body) {
            document.body.addEventListener("htmx:afterSwap", function () {
                if (state.openTooltipEl && !document.contains(state.openTooltipEl)) {
                    state.openTriggerEl = null;
                    state.openTooltipEl = null;
                    state.openShownByFocus = false;
                    state.suppressNextFocusShow = false;
                }
                var newTriggers = document.querySelectorAll(TRIGGER_SELECTOR);
                for (var i = 0; i < newTriggers.length; i++) {
                    wireTrigger(newTriggers[i]);
                }
            });
        }
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
