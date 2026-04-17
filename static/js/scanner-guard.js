// scanner-guard.js — modal keystroke capture for USB barcode scanners.
//
// While any modal surface is open (`<dialog open>` or `[aria-modal="true"]`),
// this module captures `keydown` events at the document-capture phase and
// either forwards them to the modal's focused text input or blocks them.
// This prevents a scanner burst performed while a confirmation dialog is
// on screen from leaking into the background `#scan-field` (duplicate
// scan) AND from accidentally activating the modal's default-focused
// Cancel/Confirm button via the burst's terminating Enter.
//
// Policy:
//   - Event whose target is a text-accepting element INSIDE the top modal
//     → pass through unchanged (user is typing in a modal input).
//   - Scanner-shaped keys (printable length-1 key or Enter, no Ctrl/Meta/Alt)
//     while a modal is open → preventDefault + stopPropagation. If
//     activeElement is a text input inside the modal, forward the printable
//     char (or synthetic Enter) to it; otherwise drop silently.
//   - Navigation / modifier keys (Tab, Arrow*, Escape, Ctrl+C, etc.) → pass
//     through so keyboard a11y and standard shortcuts keep working.
//
// CSP-compliant: no inline handlers, no inline styles, no eval.
// Pattern-agnostic: does NOT re-implement ISBN/V-code heuristics — that
// lives in `scan-field.js`. The guard just gates keystrokes by modal
// openness and focus location.
(function () {
    "use strict";

    if (window.__mybibliScannerGuardWired) return;
    window.__mybibliScannerGuardWired = true;

    if (typeof MutationObserver !== "function") {
        if (typeof console !== "undefined" && console.warn) {
            console.warn("scanner-guard: MutationObserver unavailable; guard disabled");
        }
        return;
    }

    var MODAL_SELECTOR = 'dialog[open], [aria-modal="true"]';
    var TEXT_INPUT_TYPES = {
        "": true, text: true, search: true, email: true, url: true,
        tel: true, password: true, number: true,
    };

    var stack = [];
    var listenerInstalled = false;
    // Test-only counters. Kept behind a flag so production bundles carry
    // zero overhead — a cheap boolean check and a dropped branch otherwise.
    var testHooks = !!window.__MYBIBLI_TEST_HOOKS;
    var listenerAttachCount = 0;
    var listenerDetachCount = 0;

    function isTextAccepting(el) {
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

    function topModal() {
        return stack.length > 0 ? stack[stack.length - 1] : null;
    }

    function refreshStack() {
        // Document order approximates LIFO for nested modals — the last
        // element in the query result is the most-recently-opened. Good
        // enough for the single-librarian use case; a real open-timestamp
        // weakmap can be added if nested-modal ordering ever matters.
        stack = Array.prototype.slice.call(document.querySelectorAll(MODAL_SELECTOR));
        syncListener();
    }

    function handleKeydown(event) {
        var top = topModal();
        if (!top) return;

        var target = event.target;
        var targetInModal = target && target.nodeType === 1 && top.contains(target);

        // Text input inside the modal: pass through unchanged.
        if (targetInModal && isTextAccepting(target)) return;

        // Only intercept keys a USB scanner can actually produce: printable
        // ASCII (length 1) and the burst's terminating Enter. Navigation
        // keys (Tab, Shift+Tab, Arrow*, Escape, Home, End, Page*) and
        // modifier combos (Ctrl+C/V/A, Cmd+*, Alt+*) must pass through so
        // keyboard a11y and standard shortcuts keep working while a modal
        // is open — a real scanner never fires Tab or Ctrl+V.
        var hasModifier = event.ctrlKey || event.metaKey || event.altKey;
        var isPrintable = !!event.key && event.key.length === 1 && !hasModifier;
        var isBurstTerminator = event.key === "Enter" && !hasModifier;
        if (!isPrintable && !isBurstTerminator) return;

        event.preventDefault();
        event.stopPropagation();

        var active = document.activeElement;
        if (!active || !top.contains(active) || !isTextAccepting(active)) return;

        if (event.key === "Enter") {
            // Forward Enter as synthetic keydown+keyup on the focused
            // input so any `keydown[key==="Enter"]` handler on that input
            // fires normally (HTMX form submission triggers, etc.).
            active.dispatchEvent(new KeyboardEvent("keydown", {
                key: "Enter", code: "Enter", bubbles: true, cancelable: true,
            }));
            active.dispatchEvent(new KeyboardEvent("keyup", {
                key: "Enter", code: "Enter", bubbles: true, cancelable: true,
            }));
            return;
        }

        if (event.key && event.key.length === 1) {
            if (active.isContentEditable) {
                active.textContent = (active.textContent || "") + event.key;
            } else {
                active.value = (active.value || "") + event.key;
            }
            // Setting .value in JS does NOT auto-fire `input` — dispatch
            // explicitly so downstream validators / autocomplete / debouncers
            // react.
            active.dispatchEvent(new Event("input", { bubbles: true }));
        }
    }

    function syncListener() {
        var shouldBeInstalled = stack.length > 0;
        if (shouldBeInstalled && !listenerInstalled) {
            document.addEventListener("keydown", handleKeydown, { capture: true });
            listenerInstalled = true;
            if (testHooks) listenerAttachCount++;
        } else if (!shouldBeInstalled && listenerInstalled) {
            document.removeEventListener("keydown", handleKeydown, { capture: true });
            listenerInstalled = false;
            if (testHooks) listenerDetachCount++;
        }
    }

    function startObserver() {
        var body = document.body;
        if (!body) return;
        var observer = new MutationObserver(function () {
            refreshStack();
        });
        observer.observe(body, {
            subtree: true,
            childList: true,
            attributes: true,
            attributeFilter: ["open", "aria-modal"],
        });
        // Initial snapshot in case a modal was already in the DOM at load.
        refreshStack();
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", startObserver);
    } else {
        startObserver();
    }

    window.mybibliScannerGuard = {
        getStackDepth: function () { return stack.length; },
        isActive: function () { return stack.length > 0; },
    };

    if (testHooks) {
        window.__mybibliScannerGuardTestHooks = {
            listenerAttachCount: function () { return listenerAttachCount; },
            listenerDetachCount: function () { return listenerDetachCount; },
            refreshStack: refreshStack,
        };
    }
})();
