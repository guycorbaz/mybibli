// scan-field.js — prefix detection and HTMX submission for scan fields
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

    function initScanFields() {
        var fields = document.querySelectorAll("[data-mybibli-scan-field]");
        fields.forEach(function (field) {
            field.addEventListener("keydown", function (e) {
                if (e.key !== "Enter") return;
                e.preventDefault();

                var code = field.value.trim();
                if (!code) return;

                var prefix = detectPrefix(code);
                field.setAttribute("data-detected-prefix", prefix);

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
})();
