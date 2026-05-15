// modal.js — UX-DR8 Modal focus-trap + lifecycle (story 9-10).
// Watches `#modal-slot` for `<dialog open>` swaps; installs Tab/Shift+Tab
// cycle, Escape/Cancel/backdrop close, background tabindex sweep (NOT
// aria-hidden — Chrome warning), aria-expanded toggle on the trigger,
// focus restoration. Scanner-guard.js (story 7-5) handles keystroke routing
// via the same `<dialog open aria-modal="true">` shape — the two modules
// cooperate via DOM, never call each other. CSP-clean.
(function () {
    "use strict";

    if (window.__mybibliModalWired) return;
    window.__mybibliModalWired = true;

    var SLOT_ID = "modal-slot";
    var FOCUSABLE_SELECTOR = [
        'a[href]:not([tabindex="-1"])',
        "button:not([disabled])",
        'input:not([disabled]):not([type="hidden"])',
        "select:not([disabled])",
        "textarea:not([disabled])",
        '[tabindex]:not([tabindex="-1"])',
    ].join(", ");

    var state = null; // { dialog, restoredTabindexes, triggerEl, onKeydown, onClick }

    function getSlot() { return document.getElementById(SLOT_ID); }

    function focusableInside(dialog) {
        return Array.prototype.slice
            .call(dialog.querySelectorAll(FOCUSABLE_SELECTOR))
            .filter(function (el) { return el.offsetParent !== null; });
    }

    function findInitialFocus(dialog) {
        var preferred = dialog.querySelector("[data-modal-default-focus]");
        if (preferred && !preferred.disabled) return preferred;
        var fallback = focusableInside(dialog)[0];
        if (fallback && typeof console !== "undefined" && console.warn) {
            console.warn("modal.js: dialog missing [data-modal-default-focus]; using first focusable");
        }
        return fallback || null;
    }

    function sweepBackgroundTabindex(dialog) {
        var saved = [];
        var all = document.querySelectorAll(FOCUSABLE_SELECTOR);
        for (var i = 0; i < all.length; i++) {
            var el = all[i];
            if (el === dialog || dialog.contains(el)) continue;
            saved.push({ el: el, prior: el.hasAttribute("tabindex") ? el.getAttribute("tabindex") : null });
            el.setAttribute("tabindex", "-1");
        }
        return saved;
    }

    function restoreBackgroundTabindex(saved) {
        for (var i = 0; i < saved.length; i++) {
            var item = saved[i];
            if (item.prior === null) item.el.removeAttribute("tabindex");
            else item.el.setAttribute("tabindex", item.prior);
        }
    }

    function open(dialog) {
        if (state) close({ skipFocusRestore: true });

        // polish-1 AC3: promote declarative `<dialog open>` to a native
        // top-layer modal. The `:modal` pseudo is true ONLY when opened
        // via `showModal()`; the declarative `open` attribute alone
        // doesn't grant native modal semantics (top-layer, inert
        // background, ::backdrop). Fixes #65 for every Pattern A modal.
        //
        // showModal() throws InvalidStateError if the dialog is already
        // marked open — so we remove the declarative `open` attribute
        // first (a synchronous attribute change, no events fired —
        // unlike dialog.close() which would dispatch a `close` event
        // that could race with our subsequent listener registration).
        if (!dialog.matches(":modal")) {
            dialog.removeAttribute("open");
            try { dialog.showModal(); } catch (_) { /* ignore — fall back to declarative open below */ }
            if (!dialog.hasAttribute("open")) {
                // showModal() failed (e.g. very old browser, dialog detached
                // mid-promotion). Restore declarative open so the test
                // assertion `dialog[open]` still matches and the existing
                // manual handlers (Escape, backdrop click, focus trap)
                // provide degraded but functional UX.
                dialog.setAttribute("open", "");
            }
        }

        // polish-1 AC3: catch native close events (Escape, native
        // form method=dialog submit) so cleanup runs even when the
        // close didn't go through our manual handlers. Native Escape
        // on a showModal()'d dialog fires the browser's close path
        // (clearing :modal + open) which would otherwise leave our
        // `state` variable stale.
        function onNativeClose() { if (state && state.dialog === dialog) close({ skipFocusRestore: false }); }
        dialog.addEventListener("close", onNativeClose, { once: true });

        var triggerEl = document.querySelector('[data-modal-trigger][data-pressed="true"]');
        if (triggerEl) triggerEl.setAttribute("aria-expanded", "true");
        var restoredTabindexes = sweepBackgroundTabindex(dialog);
        var initialFocus = findInitialFocus(dialog);
        if (initialFocus) {
            try { initialFocus.focus(); } catch (_) { /* ignore */ }
        }

        function onKeydown(evt) {
            if (evt.key === "Escape") {
                evt.preventDefault();
                close();
                return;
            }
            if (evt.key !== "Tab") return;
            var items = focusableInside(dialog);
            if (items.length === 0) return;
            var first = items[0];
            var last = items[items.length - 1];
            var active = document.activeElement;
            if (evt.shiftKey) {
                if (active === first || !dialog.contains(active)) { evt.preventDefault(); last.focus(); }
            } else {
                if (active === last || !dialog.contains(active)) { evt.preventDefault(); first.focus(); }
            }
        }

        // Track mousedown target so a drag that started inside the modal
        // (e.g., text-selection inside the body) does not count as a
        // backdrop click on mouseup outside.
        var mousedownTarget = null;
        function onMousedown(evt) { mousedownTarget = evt.target; }
        function onClick(evt) {
            var cancelBtn = evt.target && evt.target.closest && evt.target.closest("[data-modal-cancel]");
            if (cancelBtn && dialog.contains(cancelBtn)) {
                evt.preventDefault();
                close();
                return;
            }
            // Backdrop click — only when BOTH mousedown AND click target IS
            // the dialog itself (not a child, and not a drag that started
            // inside).
            if (evt.target === dialog && mousedownTarget === dialog) {
                evt.preventDefault();
                close();
            }
        }

        document.addEventListener("keydown", onKeydown, false);
        dialog.addEventListener("mousedown", onMousedown, false);
        dialog.addEventListener("click", onClick, false);

        state = { dialog: dialog, restoredTabindexes: restoredTabindexes, triggerEl: triggerEl, onKeydown: onKeydown, onMousedown: onMousedown, onClick: onClick };
    }

    function close(opts) {
        var slot = getSlot();
        if (!state) {
            if (slot) slot.innerHTML = "";
            return;
        }
        var s = state;
        state = null;

        document.removeEventListener("keydown", s.onKeydown, false);
        if (s.dialog) {
            if (s.onMousedown) s.dialog.removeEventListener("mousedown", s.onMousedown, false);
            if (s.onClick) s.dialog.removeEventListener("click", s.onClick, false);
        }
        restoreBackgroundTabindex(s.restoredTabindexes);
        if (slot) slot.innerHTML = "";

        if (s.triggerEl) {
            s.triggerEl.removeAttribute("data-pressed");
            if (s.triggerEl.hasAttribute("aria-expanded")) {
                s.triggerEl.setAttribute("aria-expanded", "false");
            }
            if (!opts || !opts.skipFocusRestore) {
                if (document.contains(s.triggerEl)) {
                    try { s.triggerEl.focus(); } catch (_) { /* ignore */ }
                }
            }
        }
    }

    // Mark the trigger as pressed on click — gives close() a focus anchor.
    // Delegated (capture) so HTMX-swapped buttons are picked up.
    document.addEventListener("click", function (evt) {
        var trigger = evt.target && evt.target.closest && evt.target.closest("[data-modal-trigger]");
        if (!trigger) return;
        var prior = document.querySelectorAll('[data-modal-trigger][data-pressed="true"]');
        for (var i = 0; i < prior.length; i++) prior[i].removeAttribute("data-pressed");
        trigger.setAttribute("data-pressed", "true");
    }, true);

    // polish-1 AC4.a: tag every Pattern A modal Confirm request with
    // `X-Modal-Confirm: true`. The server-side `ModalConfirmRetargetGuard`
    // middleware reads this header and strips `HX-Retarget`/`HX-Reswap`
    // from error responses so the body lands in our data-modal-error
    // region instead of being retargeted behind the backdrop. Same
    // isConfirm detection shape used by every modal.js listener.
    function originatesFromConfirm(elt) {
        if (!elt || !state || !state.dialog || !state.dialog.contains(elt)) return false;
        return elt.tagName === "FORM"
            || (elt.matches && elt.matches("[data-modal-confirm]"))
            || (elt.closest && elt.closest("[data-modal-confirm]"));
    }
    document.body.addEventListener("htmx:configRequest", function (evt) {
        var detail = evt.detail || {};
        if (!originatesFromConfirm(detail.elt)) return;
        if (!detail.headers) detail.headers = {};
        detail.headers["X-Modal-Confirm"] = "true";
    }, false);

    // polish-1 AC4.c (clear-on-retry): when a Confirm fires a NEW request,
    // clear any stale error message in the modal's data-modal-error
    // region. A retry after an error starts with a clean slate.
    document.body.addEventListener("htmx:beforeRequest", function (evt) {
        var detail = evt.detail || {};
        if (!originatesFromConfirm(detail.elt)) return;
        var region = state.dialog.querySelector("[data-modal-error]");
        if (region) {
            region.innerHTML = "";
            region.classList.add("hidden");
        }
    }, false);

    // After Confirm submit, either close on success OR inject the error
    // body into data-modal-error on failure. HX-Redirect drives
    // navigation; clearing the slot first avoids a stale modal frame.
    // Filter to Confirm form/button only — child HTMX (autocomplete, etc.)
    // must not close the modal.
    document.body.addEventListener("htmx:afterRequest", function (evt) {
        var detail = evt.detail || {};
        // Race guard: state may be null if the user closed the modal
        // between the request firing and the response arriving.
        if (!originatesFromConfirm(detail.elt)) return;

        if (detail.failed || detail.successful === false) {
            // polish-1 AC4.c failed-Confirm path: inject the response body
            // into data-modal-error. Retarget guard (defensive) — if
            // HX-Retarget is set on the response, AC4.b middleware didn't
            // strip it (shouldn't happen for modal Confirms post-Phase 2,
            // but if a future handler ships an explicit retarget we won't
            // double-display).
            var xhr = detail.xhr;
            if (!xhr) return;
            if (xhr.getResponseHeader && xhr.getResponseHeader("HX-Retarget")) return;
            var region = state.dialog.querySelector("[data-modal-error]");
            if (!region) return;
            region.innerHTML = xhr.responseText || "";
            region.classList.remove("hidden");
            return;
        }
        // Success: close.
        close();
    }, false);

    // polish-1 AC2: server-driven modal close via `HX-Trigger: modal-close`.
    // Variant A of the HX-Trigger idiom (post-swap addEventListener
    // native — HTMX dispatches a DOM event from the header value).
    // Broadcast on document.body; both modal.js (this slot, #modal-slot)
    // and inline-form.js (#admin-modal-slot) listen — each acts on its
    // own slot, idempotent if its slot is already empty.
    document.body.addEventListener("modal-close", function () {
        if (state) close();
    }, false);

    function observeSlot() {
        var slot = getSlot();
        if (!slot) return;
        var observer = new MutationObserver(function () {
            var dialog = slot.querySelector("dialog[open]");
            if (dialog && (!state || state.dialog !== dialog)) open(dialog);
            else if (!dialog && state) close({ skipFocusRestore: true });
        });
        observer.observe(slot, { childList: true, subtree: true });
        var existing = slot.querySelector("dialog[open]");
        if (existing) open(existing);
    }

    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", observeSlot);
    else observeSlot();

    window.mybibliModal = {
        isOpen: function () { return state !== null; },
        close: close,
    };
})();
