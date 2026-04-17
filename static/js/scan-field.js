// scan-field.js — prefix detection, ISBN validation, and HTMX submission for scan fields
(function () {
    "use strict";

    function detectPrefix(value) {
        if (/^V\d{4}$/.test(value)) return "vcode";
        if (/^L\d{4}$/.test(value)) return "lcode";
        if (/^(978|979)\d{10}$/.test(value)) return "isbn";
        if (/^977\d{5,10}$/.test(value)) return "issn";
        if (/^\d{8,13}$/.test(value)) return "upc";
        return "unknown";
    }

    /**
     * Validate ISBN-13 checksum using modulo-10 with alternating weights 1 and 3.
     * @param {string} isbn - 13-digit ISBN string
     * @returns {boolean} true if checksum is valid
     */
    function validateIsbn13(isbn) {
        if (isbn.length !== 13) return false;
        if (!/^\d{13}$/.test(isbn)) return false;

        var sum = 0;
        for (var i = 0; i < 12; i++) {
            var digit = parseInt(isbn[i], 10);
            sum += (i % 2 === 0) ? digit : digit * 3;
        }
        var checkDigit = (10 - (sum % 10)) % 10;
        return checkDigit === parseInt(isbn[12], 10);
    }

    /**
     * Validate V-code format: uppercase V + exactly 4 digits, V0001-V9999.
     * @param {string} code - V-code string
     * @returns {boolean} true if format is valid
     */
    function validateVcode(code) {
        if (!/^V\d{4}$/.test(code)) return false;
        return code !== "V0000";
    }

    /**
     * Check if the title form is currently open and has focus.
     */
    function isFormActiveAndFocused() {
        var container = document.getElementById("title-form-container");
        if (!container || !container.innerHTML.trim()) return false;

        var active = document.activeElement;
        return active && container.contains(active);
    }

    /**
     * Inject a client-side error feedback entry (no server round-trip).
     */
    function injectLocalFeedback(variant, message) {
        var feedbackList = document.getElementById("feedback-list");
        if (!feedbackList) return;

        var colors = {
            error: {
                border: "border-red-500",
                bg: "bg-red-50 dark:bg-red-900/20",
                icon: "text-red-600 dark:text-red-400",
                svg: '<path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.28 7.22a.75.75 0 00-1.06 1.06L8.94 10l-1.72 1.72a.75.75 0 101.06 1.06L10 11.06l1.72 1.72a.75.75 0 101.06-1.06L11.06 10l1.72-1.72a.75.75 0 00-1.06-1.06L10 8.94 8.28 7.22z" clip-rule="evenodd" />'
            }
        };

        var c = colors[variant] || colors.error;
        var escapedMessage = message
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;")
            .replace(/"/g, "&quot;")
            .replace(/'/g, "&#x27;");

        var html = '<div class="p-3 border-l-4 ' + c.border + " " + c.bg +
            ' rounded-r feedback-entry" role="status" data-feedback-variant="' + variant + '">' +
            '<div class="flex items-start gap-2">' +
            '<svg class="' + c.icon + ' w-5 h-5 flex-shrink-0 mt-0.5" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">' +
            c.svg + '</svg>' +
            '<div class="flex-1"><p class="text-stone-700 dark:text-stone-300">' +
            escapedMessage + '</p></div>' +
            '<button type="button" class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-200 p-1 min-w-[44px] min-h-[44px] md:min-w-[36px] md:min-h-[36px] flex items-center justify-center" aria-label="Dismiss" data-action="dismiss-feedback">' +
            '<svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z" /></svg>' +
            '</button></div></div>';

        feedbackList.insertAdjacentHTML("afterbegin", html);
    }

    function initScanFields() {
        var fields = document.querySelectorAll("[data-mybibli-scan-field]");
        fields.forEach(function (field) {
            field.addEventListener("keydown", function (e) {
                if (e.key !== "Enter") return;

                // If title form is open and focused, let the form handle Enter
                if (isFormActiveAndFocused()) return;

                e.preventDefault();

                var code = field.value.trim();
                if (!code) return;

                var prefix = detectPrefix(code);
                field.setAttribute("data-detected-prefix", prefix);

                // ISBN checksum validation before server submission
                if (prefix === "isbn") {
                    if (!validateIsbn13(code)) {
                        var isbnError = field.getAttribute("data-isbn-error") || "Invalid ISBN checksum";
                        injectLocalFeedback("error", isbnError);
                        field.value = "";
                        return;
                    }
                }

                // V-code format validation before server submission
                if (prefix === "vcode") {
                    if (!validateVcode(code)) {
                        var vcodeError = field.getAttribute("data-vcode-error") || "Invalid volume code format";
                        injectLocalFeedback("error", vcodeError);
                        field.value = "";
                        return;
                    }
                }

                // Store last scan code for error recovery (restored by mybibli.js HTMX error handler)
                window.mybibliLastScanCode = code;

                if (typeof htmx !== "undefined") {
                    htmx.ajax("POST", "/catalog/scan", {
                        target: "#feedback-list",
                        swap: "afterbegin",
                        values: { code: code },
                    });
                }

                field.value = "";
            });
        });
    }

    // Self-initialize when DOM is ready
    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", initScanFields);
    } else {
        initScanFields();
    }

    // Expose for testing
    window.mybibliDetectPrefix = detectPrefix;
    window.mybibliValidateIsbn13 = validateIsbn13;
    window.mybibliValidateVcode = validateVcode;
})();
