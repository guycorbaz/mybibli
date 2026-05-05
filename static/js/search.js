/**
 * Home page search field — scanner detection state machine.
 * Distinguishes barcode scanner bursts from human typing.
 *
 * States: IDLE → DETECTING → SEARCH_MODE → SCAN_PENDING
 *
 * Dispatched events (both bubble from #search-field):
 * - "search-fire" — debounced/Enter input fired in SEARCH_MODE; triggers
 *   the inline browse HTMX request rendering #browse-results.
 * - "scan-fire"  — scanner burst + Enter classified as SCAN_PENDING;
 *   triggers the GET /scan handler which redirects to /title|volume|location|catalog.
 */
(function () {
    "use strict";

    const IDLE = "IDLE";
    const DETECTING = "DETECTING";
    const SEARCH_MODE = "SEARCH_MODE";
    const SCAN_PENDING = "SCAN_PENDING";

    let state = IDLE;
    let lastKeystroke = 0;
    let debounceTimer = null;
    let fieldContentAtScan = "";

    function init() {
        const field = document.getElementById("search-field");
        if (!field) return;

        const scannerThreshold = parseInt(field.dataset.scannerThreshold || "100", 10);
        const debounceDelay = parseInt(field.dataset.debounce || "100", 10);
        const minChars = 2;

        field.addEventListener("keydown", function (e) {
            const now = Date.now();
            const interKey = now - lastKeystroke;
            lastKeystroke = now;

            if (e.key === "Escape") {
                field.value = "";
                state = IDLE;
                clearTimeout(debounceTimer);
                announce(field, "");
                return;
            }

            if (e.key === "Enter") {
                e.preventDefault();
                clearTimeout(debounceTimer);

                if (state === DETECTING && interKey < scannerThreshold) {
                    // Fast burst + Enter = scanner scan → /scan handler
                    fieldContentAtScan = field.value;
                    state = SCAN_PENDING;
                    announce(field, "scanning");
                    fireScan(field);
                } else {
                    // Normal Enter = final search
                    if (field.value.trim().length >= minChars) {
                        announce(field, "searching");
                        fireSearch(field);
                    }
                    state = IDLE;
                }
                return;
            }

            // Non-printable keys
            if (e.key.length > 1) return;

            switch (state) {
                case IDLE:
                    state = DETECTING;
                    break;

                case DETECTING:
                    if (interKey > scannerThreshold) {
                        // Slow typing → search mode
                        state = SEARCH_MODE;
                        announce(field, "searching");
                        startDebounce(field, debounceDelay, minChars);
                    }
                    // Else: still accumulating fast keystrokes
                    break;

                case SEARCH_MODE:
                    // Reset debounce on each keystroke
                    startDebounce(field, debounceDelay, minChars);
                    break;

                case SCAN_PENDING:
                    // User typing during fetch — transition to search mode
                    state = SEARCH_MODE;
                    announce(field, "searching");
                    startDebounce(field, debounceDelay, minChars);
                    break;
            }
        });

        // Handle search input clear button (type="search" native clear)
        field.addEventListener("search", function () {
            if (field.value === "") {
                state = IDLE;
                clearTimeout(debounceTimer);
                announce(field, "");
                // Clear results by firing empty search
                fireSearch(field);
            }
        });

        // HTMX response handling for SCAN_PENDING state.
        // For a successful scan → /scan returns HX-Redirect and HTMX
        // navigates away before this fires (page unloads). This branch
        // only runs if a swap actually landed (e.g., a search-fire result
        // overlapped a SCAN_PENDING). Reset to IDLE / SEARCH_MODE based
        // on whether the user kept typing while the request was in flight.
        // Scope to the browse-results target so unrelated OOB swaps
        // (overdue indicator refresh, pendingUpdates, etc.) don't trip
        // the SEARCH_MODE / SCAN_PENDING state cleanup.
        document.body.addEventListener("htmx:afterSwap", function (e) {
            var targetId = e && e.detail && e.detail.target && e.detail.target.id;
            if (targetId !== "browse-results" && targetId !== "search-state-announcement") {
                return;
            }
            if (state === SCAN_PENDING) {
                if (field.value === fieldContentAtScan) {
                    field.value = "";
                    state = IDLE;
                    announce(field, "");
                } else {
                    state = SEARCH_MODE;
                    announce(field, "searching");
                }
            } else if (state === SEARCH_MODE) {
                // Results landed — clear "Searching" announcement.
                announce(field, "");
            }
        });

        // HTMX error handling — class toggle instead of `.style.opacity`
        // (strict CSP blocks runtime style writes; class lives in browse.css).
        // Story 9-9: SCAN_PENDING resets to IDLE on error so the next
        // keystroke transitions cleanly. The polite aria-live region also
        // gets the localized scan-failed fallback (AC7).
        document.body.addEventListener("htmx:responseError", function () {
            var tbody = document.getElementById("browse-results");
            if (tbody) tbody.classList.add("htmx-opacity-reset");
            if (state === SCAN_PENDING) {
                state = IDLE;
                announce(field, "scanfailed");
            }
        });

        document.body.addEventListener("htmx:sendError", function () {
            var tbody = document.getElementById("browse-results");
            if (tbody) {
                tbody.classList.add("htmx-opacity-reset");
                var msg = field.dataset.connectionLost || "Connection lost";
                tbody.innerHTML =
                    '<div class="text-center py-8 text-red-500">' + msg + '</div>';
            }
            if (state === SCAN_PENDING) {
                state = IDLE;
                announce(field, "scanfailed");
            }
        });
    }

    function startDebounce(field, delay, minChars) {
        clearTimeout(debounceTimer);
        debounceTimer = setTimeout(function () {
            if (field.value.trim().length >= minChars) {
                fireSearch(field);
            }
        }, delay);
    }

    function fireSearch(field) {
        field.dispatchEvent(new Event("search-fire", { bubbles: true }));
    }

    function fireScan(field) {
        field.dispatchEvent(new Event("scan-fire", { bubbles: true }));
    }

    /**
     * Story 9-9 — write a localized state label into the polite aria-live
     * region. `key` is one of "searching" / "scanning" / "" (empty = clear).
     * Labels are pre-translated server-side and exposed as data-announce-*
     * attributes on #search-field (CSP-clean — no t!() in JS).
     */
    function announce(field, key) {
        var region = document.getElementById("search-state-announcement");
        if (!region) return;
        if (!key) {
            region.textContent = "";
            return;
        }
        var attr = "announce" + key.charAt(0).toUpperCase() + key.slice(1);
        region.textContent = field.dataset[attr] || "";
    }

    // Global keyboard shortcut: "/" focuses search field
    document.addEventListener("keydown", function (e) {
        if (e.key === "/" && !isInputFocused()) {
            var field = document.getElementById("search-field");
            if (field) {
                e.preventDefault();
                field.focus();
            }
        }
    });

    function isInputFocused() {
        var el = document.activeElement;
        if (!el) return false;
        var tag = el.tagName.toLowerCase();
        return (
            tag === "input" ||
            tag === "textarea" ||
            tag === "select" ||
            el.isContentEditable
        );
    }

    // Initialize on DOMContentLoaded
    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
