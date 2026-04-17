/**
 * Home page search field — scanner detection state machine.
 * Distinguishes barcode scanner bursts from human typing.
 *
 * States: IDLE → DETECTING → SEARCH_MODE → SCAN_PENDING
 * Dispatches custom "search-fire" event to trigger HTMX requests.
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
                return;
            }

            if (e.key === "Enter") {
                e.preventDefault();
                clearTimeout(debounceTimer);

                if (state === DETECTING && interKey < scannerThreshold) {
                    // Fast burst + Enter = scanner scan
                    fieldContentAtScan = field.value;
                    state = SCAN_PENDING;
                    fireSearch(field);
                } else {
                    // Normal Enter = final search
                    if (field.value.trim().length >= minChars) {
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
                    startDebounce(field, debounceDelay, minChars);
                    break;
            }
        });

        // Handle search input clear button (type="search" native clear)
        field.addEventListener("search", function () {
            if (field.value === "") {
                state = IDLE;
                clearTimeout(debounceTimer);
                // Clear results by firing empty search
                fireSearch(field);
            }
        });

        // HTMX response handling for SCAN_PENDING state
        document.body.addEventListener("htmx:afterSwap", function (e) {
            if (state === SCAN_PENDING) {
                if (field.value === fieldContentAtScan) {
                    field.value = "";
                    state = IDLE;
                } else {
                    state = SEARCH_MODE;
                }
            }
        });

        // HTMX error handling — class toggle instead of `.style.opacity`
        // (strict CSP blocks runtime style writes; class lives in browse.css).
        document.body.addEventListener("htmx:responseError", function () {
            var tbody = document.getElementById("browse-results");
            if (tbody) tbody.classList.add("htmx-opacity-reset");
        });

        document.body.addEventListener("htmx:sendError", function () {
            var tbody = document.getElementById("browse-results");
            if (tbody) {
                tbody.classList.add("htmx-opacity-reset");
                var msg = field.dataset.connectionLost || "Connection lost";
                tbody.innerHTML =
                    '<div class="text-center py-8 text-red-500">' + msg + '</div>';
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
