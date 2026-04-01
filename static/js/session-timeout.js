// Session inactivity timeout warning
// Shows a Toast notification 5 minutes before session expires
(function () {
    "use strict";

    var WARNING_BEFORE_SECS = 5 * 60; // 5 minutes before expiry
    var timerId = null;
    var toastEl = null;

    function getTimeoutSecs() {
        var attr = document.body.getAttribute("data-session-timeout");
        return attr ? parseInt(attr, 10) : 0;
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

    function showWarning() {
        if (toastEl) return; // Already showing
        toastEl = createToast();
        // i18n: detect language from <html lang> and use appropriate strings
        var msgEl = toastEl.querySelector("#session-timeout-message");
        var btnEl = toastEl.querySelector("#session-keepalive-btn");
        var lang = document.documentElement.lang || "en";
        var i18n = {
            en: { expiry: "Your session will expire in 5 minutes.", stay: "Stay connected" },
            fr: { expiry: "Votre session expirera dans 5 minutes.", stay: "Rester connecté" },
        };
        var strings = i18n[lang] || i18n.en;
        msgEl.textContent = strings.expiry;
        btnEl.textContent = strings.stay;
        btnEl.addEventListener("click", keepAlive);
        var dismissBtn = toastEl.querySelector("#session-timeout-dismiss");
        if (dismissBtn) dismissBtn.addEventListener("click", hideWarning);
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
            htmx.ajax("POST", "/session/keepalive", { swap: "none" });
        } else {
            fetch("/session/keepalive", { method: "POST" });
        }
        hideWarning();
        resetTimer();
    }

    function resetTimer() {
        var timeout = getTimeoutSecs();
        if (timeout <= 0) return;

        if (timerId) clearTimeout(timerId);

        var warningDelay = (timeout - WARNING_BEFORE_SECS) * 1000;
        if (warningDelay <= 0) return; // Timeout too short for warning

        timerId = setTimeout(showWarning, warningDelay);
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
