// Session inactivity timeout warning
// Shows a Toast notification 5 minutes before session expires
(function () {
    "use strict";

    // Warning offset is derived from the total timeout instead of being
    // hardcoded to 5 min so that:
    //   - short timeouts (e.g. admin-configured 60s, or E2E test mode)
    //     still produce a visible warning instead of silent logout;
    //   - the warning always fires roughly one third into the session's
    //     remaining life, capped at 5 min for long timeouts.
    var WARNING_MAX_SECS = 5 * 60;
    var timerId = null;
    var toastEl = null;

    function getTimeoutSecs() {
        var attr = document.body.getAttribute("data-session-timeout");
        return attr ? parseInt(attr, 10) : 0;
    }

    function computeWarningBeforeSecs(timeoutSecs) {
        // At least 1s before expiry, at most 5min, ~1/3 of the window.
        var third = Math.floor(timeoutSecs / 3);
        return Math.max(1, Math.min(WARNING_MAX_SECS, third));
    }

    function createToast() {
        var toast = document.createElement("div");
        toast.id = "session-timeout-toast";
        toast.setAttribute("role", "alert");
        toast.setAttribute("aria-live", "assertive");
        toast.className =
            "fixed bottom-4 right-4 z-50 bg-amber-50 dark:bg-amber-900/90 border border-amber-300 dark:border-amber-600 rounded-lg shadow-lg p-4 max-w-sm";
        toast.innerHTML =
            '<div class="flex items-start gap-3">' +
            '<svg class="w-5 h-5 text-amber-600 dark:text-amber-400 flex-shrink-0 mt-0.5" viewBox="0 0 20 20" fill="currentColor"><path fill-rule="evenodd" d="M8.485 2.495c.673-1.167 2.357-1.167 3.03 0l6.28 10.875c.673 1.167-.17 2.625-1.516 2.625H3.72c-1.347 0-2.189-1.458-1.515-2.625L8.485 2.495zM10 5a.75.75 0 01.75.75v3.5a.75.75 0 01-1.5 0v-3.5A.75.75 0 0110 5zm0 9a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd" /></svg>' +
            '<div class="flex-1">' +
            '<p class="text-sm text-amber-800 dark:text-amber-200" id="session-timeout-message"></p>' +
            '<button type="button" id="session-keepalive-btn" class="mt-2 px-3 py-1.5 text-sm font-medium bg-amber-600 text-white rounded hover:bg-amber-700 min-h-[44px] md:min-h-[36px]"></button>' +
            "</div>" +
            '<button type="button" id="session-timeout-dismiss" class="text-amber-400 hover:text-amber-600 p-1" aria-label="Dismiss">' +
            '<svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor"><path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z" /></svg>' +
            "</button>" +
            "</div>";
        return toast;
    }

    function showWarning(warningBeforeSecs) {
        if (toastEl) return; // Already showing
        toastEl = createToast();
        // i18n: detect language from <html lang> and use appropriate strings
        var msgEl = toastEl.querySelector("#session-timeout-message");
        var btnEl = toastEl.querySelector("#session-keepalive-btn");
        var lang = document.documentElement.lang || "en";
        // Mirrored in locales/{en,fr}.yml under `session.*` — keep in sync.
        // `expiry_soon` = short/parameterless form when remaining < 1 min.
        // `expiry_in_minutes` = parameterized on %{minutes}.
        var i18n = {
            en: {
                expiry_soon: "Your session is about to expire.",
                expiry_in_minutes: "Your session will expire in %{minutes} min.",
                stay: "Stay connected",
                dismiss: "Dismiss",
            },
            fr: {
                expiry_soon: "Votre session va bientôt expirer.",
                expiry_in_minutes: "Votre session expirera dans %{minutes} min.",
                stay: "Rester connecté",
                dismiss: "Fermer",
            },
        };
        var strings = i18n[lang] || i18n.en;
        var minutes = Math.round(warningBeforeSecs / 60);
        var msg =
            minutes >= 1
                ? strings.expiry_in_minutes.replace("%{minutes}", String(minutes))
                : strings.expiry_soon;
        msgEl.textContent = msg;
        btnEl.textContent = strings.stay;
        btnEl.addEventListener("click", keepAlive);
        var dismissBtn = toastEl.querySelector("#session-timeout-dismiss");
        if (dismissBtn) {
            dismissBtn.setAttribute("aria-label", strings.dismiss);
            dismissBtn.addEventListener("click", hideWarning);
        }
        document.body.appendChild(toastEl);
    }

    function hideWarning() {
        if (toastEl) {
            toastEl.remove();
            toastEl = null;
        }
    }

    function keepAlive() {
        if (typeof htmx !== "undefined") {
            // csrf.js's htmx:configRequest listener injects X-CSRF-Token.
            htmx.ajax("POST", "/session/keepalive", { swap: "none" });
        } else {
            // Bare fetch() fallback — no HTMX event fires, so we must read
            // the token off the meta tag ourselves. Optional chaining +
            // nullish coalescing: if the meta tag is missing the server
            // returns a clean 403 instead of the browser throwing.
            // Mirror csrf.js's meta-read convention — use getAttribute()
            // and trim so a null or whitespace-only content becomes the
            // empty string (clean 403 instead of a whitespace-token that
            // never matches the stored token).
            var meta = document.querySelector('meta[name="csrf-token"]');
            var raw = meta ? meta.getAttribute("content") : "";
            var token = raw ? raw.trim() : "";
            fetch("/session/keepalive", {
                method: "POST",
                headers: { "X-CSRF-Token": token },
            });
        }
        hideWarning();
        resetTimer();
    }

    function resetTimer() {
        var timeout = getTimeoutSecs();
        if (timeout <= 0) return;

        if (timerId) clearTimeout(timerId);

        var warningBeforeSecs = computeWarningBeforeSecs(timeout);
        var warningDelay = (timeout - warningBeforeSecs) * 1000;
        if (warningDelay <= 0) return;

        timerId = setTimeout(function () {
            showWarning(warningBeforeSecs);
        }, warningDelay);
    }

    function init() {
        var timeout = getTimeoutSecs();
        if (timeout <= 0) return; // No session timeout (anonymous user)
        resetTimer();

        // Reset timer on user interaction (mirrors server-side last_activity update)
        document.addEventListener("htmx:afterRequest", resetTimer);
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
