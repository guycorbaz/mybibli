// nav.js — mobile-menu disclosure controller (story 9-17).
//
// Replaces and supersedes the basic mobile-menu toggle that used to live
// in `mybibli.js` pre-9-17. Adds, on top of the click-to-toggle:
//   - Outside-click close (mousedown on document)
//   - Escape close + focus restoration to the trigger button
//   - Focus trap (Tab/Shift+Tab cycle inside #mobile-nav, mirror of modal.js)
//   - Link-click close (Ctrl-click new-tab also collapses the panel)
//   - Scanner-burst auto-close (a USB barcode scanner burst on the page
//     while the panel is open closes it AND forwards the burst-confirming
//     keystroke to #scan-field if the page has one)
//
// Standalone burst detector — does NOT depend on scanner-guard.js's modal
// stack (the mobile nav panel is neither <dialog open> nor [aria-modal])
// and does NOT duplicate search.js's full state machine. A single
// `lastKeydownAt = Date.now()` timestamp + threshold compare is enough
// for nav.js's needs.
//
// Threshold source: `<input id="search-field" data-scanner-threshold="...">`
// at templates/pages/home.html:33 (only the home page renders the search
// field; the catalog `#scan-field` does NOT carry the attribute), else
// hardcoded fallback 50ms (matches `scanner_burst_threshold_ms` config
// default).
//
// Known v1 limitation: the FIRST keystroke of a burst cannot be classified
// as "burst" (no prior timestamp). It is consumed by the panel's normal
// focus target. Re-scanning is the recovery path. See AC8 for the
// acceptance rationale.
//
// CSP-clean: no inline handlers, no eval, all listeners via addEventListener.
(function () {
    "use strict";

    var TOGGLE_ID = "mobile-menu-toggle";
    var PANEL_ID = "mobile-nav";
    var SCAN_FIELD_ID = "scan-field";
    var SEARCH_FIELD_ID = "search-field";
    var DEFAULT_BURST_THRESHOLD_MS = 50;

    // Mirror of modal.js's FOCUSABLE_SELECTOR. Rule-of-three not yet hit
    // (modal.js + nav.js = 2 callers); a future story can extract once a
    // third surface (e.g., a sidebar) needs the same selector.
    var FOCUSABLE_SELECTOR = [
        'a[href]:not([tabindex="-1"])',
        "button:not([disabled])",
        'input:not([disabled]):not([type="hidden"])',
        "select:not([disabled])",
        "textarea:not([disabled])",
        '[tabindex]:not([tabindex="-1"])',
    ].join(", ");

    function focusableInside(el) {
        return Array.prototype.slice
            .call(el.querySelectorAll(FOCUSABLE_SELECTOR))
            .filter(function (e) { return e.offsetParent !== null; });
    }

    function getBurstThresholdMs() {
        var search = document.getElementById(SEARCH_FIELD_ID);
        if (search && search.dataset && search.dataset.scannerThreshold) {
            var n = parseInt(search.dataset.scannerThreshold, 10);
            if (!isNaN(n) && n > 0) return n;
        }
        return DEFAULT_BURST_THRESHOLD_MS;
    }

    function init() {
        var btn = document.getElementById(TOGGLE_ID);
        var panel = document.getElementById(PANEL_ID);
        if (!btn || !panel) return;
        if (btn.dataset.wired === "true") return;
        btn.dataset.wired = "true";

        var state = { open: false, previousActiveElement: null };
        var lastKeydownAt = 0;
        var burstThresholdMs = getBurstThresholdMs();

        function openPanel() {
            if (state.open) return;
            state.open = true;
            state.previousActiveElement = document.activeElement;
            panel.classList.remove("hidden");
            btn.setAttribute("aria-expanded", "true");
            var first = focusableInside(panel)[0];
            if (first) {
                try { first.focus(); } catch (_) { /* ignore */ }
            }
        }

        function closePanel(opts) {
            if (!state.open) return;
            state.open = false;
            panel.classList.add("hidden");
            btn.setAttribute("aria-expanded", "false");
            var restoreFocus = !opts || opts.restoreFocus !== false;
            if (restoreFocus && document.contains(btn)) {
                try { btn.focus(); } catch (_) { /* ignore */ }
            }
            state.previousActiveElement = null;
        }

        // Trigger click — toggle.
        btn.addEventListener("click", function () {
            if (state.open) closePanel();
            else openPanel();
        });

        // Outside-click close — mousedown (not click) so a drag that started
        // inside the panel and ended outside (text selection) does NOT
        // count as a backdrop click. Mirror of modal.js's mousedown gate.
        document.addEventListener("mousedown", function (e) {
            if (!state.open) return;
            var t = e.target;
            if (t && (panel.contains(t) || btn.contains(t))) return;
            closePanel();
        });

        // Escape close + focus trap — same listener so Escape and Tab share
        // the early `state.open` short-circuit and do not race each other.
        document.addEventListener("keydown", function (e) {
            if (!state.open) return;

            if (e.key === "Escape") {
                e.preventDefault();
                closePanel();
                return;
            }

            if (e.key === "Tab") {
                var items = focusableInside(panel);
                if (items.length === 0) return;
                var first = items[0];
                var last = items[items.length - 1];
                var active = document.activeElement;
                if (e.shiftKey) {
                    if (active === first || !panel.contains(active)) {
                        e.preventDefault();
                        last.focus();
                    }
                } else {
                    if (active === last || !panel.contains(active)) {
                        e.preventDefault();
                        first.focus();
                    }
                }
            }
        });

        // Link-click close — Ctrl-click / middle-click new-tab keeps the
        // current page; the panel should still collapse so a re-open is
        // intentional. Use `restoreFocus: false` to NOT fight the
        // navigation in the regular full-page case.
        panel.addEventListener("click", function (e) {
            var link = e.target && e.target.closest && e.target.closest("a[href]");
            if (link && panel.contains(link)) {
                closePanel({ restoreFocus: false });
            }
        });

        // Scanner-burst auto-close. Track lastKeydownAt unconditionally so
        // the FIRST keydown of a burst seeds the timestamp; the SECOND
        // keydown is the one that classifies the pair as "burst" and
        // triggers the close. The first keystroke is consumed by the
        // panel's normal focus target — accepted v1 limitation (AC8).
        document.addEventListener("keydown", function (e) {
            var now = Date.now();
            var delta = now - lastKeydownAt;
            lastKeydownAt = now;
            if (!state.open) return;
            if (e.key && e.key.length !== 1) return;     // navigation/modifier keys excluded
            if (e.ctrlKey || e.metaKey || e.altKey) return;
            if (delta >= burstThresholdMs) return;        // human-paced typing — leave panel open

            // Confirmed burst: close + forward this keystroke to #scan-field
            // if present (catalog page only). Drop silently otherwise.
            closePanel({ restoreFocus: false });
            var scan = document.getElementById(SCAN_FIELD_ID);
            if (scan) {
                scan.value = (scan.value || "") + e.key;
                scan.dispatchEvent(new Event("input", { bubbles: true }));
                try { scan.focus(); } catch (_) { /* ignore */ }
            }
        });

        // pagehide is a no-op — state is closure-scoped and dies with the
        // document. Documented for parity with connection-monitor.js.
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
