// Story 9-16 — connection-lost overlay controller (UX-DR13).
//
// Listens for `htmx:sendError` (network-failure ONLY — NOT 4xx/5xx, those
// are application errors handled by FeedbackEntry). When fired, surfaces
// a full-viewport overlay (`#connection-lost-overlay`) and starts polling
// `GET /health` every 5s. On success: dismiss overlay + spawn a transient
// "Connection restored" toast.
//
// Coordinated with `#scan-field` — disabled while overlay is shown so
// subsequent scans don't queue blind into a request that will never reach
// the server. Focus is restored on dismissal if the scan field was the
// active element at show time.
//
// Exemption notes:
//   - `/health` is exempt from CSRF middleware naturally (GET, csrf.rs:71).
//   - `/health` is exempt from `session_resolve_middleware` via story 9-16
//     short-circuit at `src/middleware/auth.rs::session_resolve_middleware`
//     (otherwise polling would burn ~720 DB writes/hour during an outage).
//   - `/health` is exempt from `setup_gate` middleware whitelist.
//
// CSP-clean: no inline handlers, no eval, all listeners via addEventListener.
(function () {
    "use strict";

    var POLL_INTERVAL_MS = 5000;
    var TOAST_DISMISS_MS = 3000;
    var HEALTH_URL = "/health";

    var state = {
        shown: false,
        timerId: null,
        previousActiveElement: null,
    };

    function init() {
        var overlay = document.getElementById("connection-lost-overlay");
        if (!overlay) return;
        if (overlay.dataset.wired === "true") return;
        overlay.dataset.wired = "true";

        // Network-failure ONLY (NOT 4xx/5xx — those fire `htmx:responseError`,
        // which we deliberately do NOT listen to per UX-DR27).
        document.body.addEventListener("htmx:sendError", function () {
            showOverlay();
        });

        // Browser-level network state. `offline` fires before any HTMX
        // request fails, so we can show the overlay proactively.
        window.addEventListener("offline", function () {
            showOverlay();
        });
        // `online` doesn't guarantee the server is reachable (Wi-Fi up but
        // the server may still be down), so trigger an immediate poll
        // instead of auto-dismissing.
        window.addEventListener("online", function () {
            if (state.shown) pollHealth();
        });

        // Retry button — delegated listener so we don't have to re-bind on
        // future overlay re-renders. Filter to the button inside the overlay.
        document.body.addEventListener("click", function (e) {
            var target = e.target;
            if (!target || !target.closest) return;
            var retryBtn = target.closest('#connection-lost-overlay [data-action="retry"]');
            if (!retryBtn) return;
            e.preventDefault();
            // Reset the timer so the user gets immediate feedback rather
            // than waiting for the next 5s tick.
            stopTimer();
            pollHealth();
            startTimer();
        });

        // Story 9-16 patch P3 — clean up timers on document teardown.
        // Without this, the polling setInterval + the toast setTimeout
        // can outlive the document (bfcache restore, slow tab unload),
        // leaking timers and possibly firing fetch() on a detached doc.
        // `pagehide` fires on both unload AND bfcache stash; safer than
        // `beforeunload` (which doesn't fire on mobile back/forward).
        window.addEventListener("pagehide", function () {
            stopTimer();
            var staleToast = document.getElementById("connection-restored-toast");
            if (staleToast) staleToast.remove();
        });
    }

    function showOverlay() {
        if (state.shown) return; // idempotent
        var overlay = document.getElementById("connection-lost-overlay");
        if (!overlay) return;

        state.shown = true;
        overlay.classList.remove("hidden");
        // Story 9-16 patch P2 — toggle aria-modal="true" dynamically (NOT
        // statically in base.html) so scanner-guard's MutationObserver
        // (`MODAL_SELECTOR = 'dialog[open], [aria-modal="true"]'`) only
        // captures keystrokes WHILE the overlay is shown. Without this,
        // a USB scanner burst during an outage would still leak into any
        // other text input on the page (the `disabled` attribute on
        // `#scan-field` is per-field, not page-wide).
        overlay.setAttribute("aria-modal", "true");

        // Disable scan field if present — prevents the scanner from
        // queueing key bursts into a buffer that will silently drop on
        // reconnect. Remember the active element for focus restoration.
        var scanField = document.getElementById("scan-field");
        if (scanField) {
            state.previousActiveElement =
                document.activeElement === scanField ? scanField : null;
            scanField.setAttribute("disabled", "true");
        } else {
            state.previousActiveElement = null;
        }

        startTimer();
    }

    function dismissOverlay() {
        if (!state.shown) return; // idempotent
        var overlay = document.getElementById("connection-lost-overlay");
        if (!overlay) return;

        state.shown = false;
        overlay.classList.add("hidden");
        // Story 9-16 patch P2 — release scanner-guard MutationObserver
        // by removing aria-modal so other inputs receive keystrokes again.
        overlay.removeAttribute("aria-modal");
        stopTimer();

        // Re-enable scan field + restore focus.
        var scanField = document.getElementById("scan-field");
        if (scanField) {
            scanField.removeAttribute("disabled");
            if (state.previousActiveElement === scanField) {
                scanField.focus();
            }
        }
        state.previousActiveElement = null;

        // Spawn the "Connection restored" toast. The string is server-
        // rendered into the overlay's data-attr so we don't hardcode
        // EN/FR strings inline (mirror of session-timeout.js but with
        // i18n-via-data-attr — a NEW pattern in this project).
        var toastText = overlay.dataset.i18nRestoredToast || "Connection restored";
        spawnToast(toastText);
    }

    function pollHealth() {
        fetch(HEALTH_URL, { cache: "no-store" })
            .then(function (response) {
                if (response.ok) {
                    dismissOverlay();
                }
                // On non-2xx (e.g., 503 from a maintenance page), keep
                // the overlay shown — the server is responding but not
                // healthy.
            })
            .catch(function () {
                // Network error — keep overlay shown. Don't log; this is
                // expected during the outage and would spam the console.
            });
    }

    function startTimer() {
        if (state.timerId !== null) return;
        state.timerId = setInterval(pollHealth, POLL_INTERVAL_MS);
    }

    function stopTimer() {
        if (state.timerId === null) return;
        clearInterval(state.timerId);
        state.timerId = null;
    }

    function spawnToast(text) {
        // Idempotent: if a previous toast is still on screen (rare —
        // requires a quick disconnect/reconnect cycle), don't stack them.
        var existing = document.getElementById("connection-restored-toast");
        if (existing) existing.remove();

        var toast = document.createElement("div");
        toast.id = "connection-restored-toast";
        toast.setAttribute("role", "status");
        toast.setAttribute("aria-live", "polite");
        toast.className =
            "fixed bottom-4 right-4 z-50 bg-emerald-50 dark:bg-emerald-900/90 border border-emerald-300 dark:border-emerald-600 rounded-lg shadow-lg p-4 max-w-sm motion-safe:transition-opacity motion-safe:duration-200";
        toast.innerHTML =
            '<div class="flex items-start gap-3">' +
            '<svg class="w-5 h-5 text-emerald-600 dark:text-emerald-400 flex-shrink-0 mt-0.5" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd" /></svg>' +
            '<p class="flex-1 text-sm text-emerald-800 dark:text-emerald-200"></p>' +
            "</div>";
        // Set text via .textContent (CSP-safe — no innerHTML interpolation
        // of server-supplied strings).
        toast.querySelector("p").textContent = text;
        document.body.appendChild(toast);

        setTimeout(function () {
            toast.remove();
        }, TOAST_DISMISS_MS);
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
