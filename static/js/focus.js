// focus.js — autofocus restoration for scan field
(function () {
    "use strict";

    var INTERACTIVE_TAGS = ["INPUT", "TEXTAREA", "SELECT", "BUTTON", "A"];

    function initFocusAttractor() {
        var scanField = document.getElementById("scan-field");
        if (!scanField) return;

        // Primary: restore focus when scan field loses it
        scanField.addEventListener("focusout", function () {
            setTimeout(function () {
                // Don't steal focus from interactive elements or dialogs
                var active = document.activeElement;
                if (!active || active === document.body || active === scanField) {
                    scanField.focus();
                    return;
                }
                // If focus moved to any interactive element, let it stay
                if (
                    INTERACTIVE_TAGS.indexOf(active.tagName) !== -1 ||
                    active.hasAttribute("contenteditable") ||
                    active.closest("[role='dialog']")
                ) {
                    return;
                }
                scanField.focus();
            }, 0);
        });

        // Secondary: restore focus after HTMX settles
        document.addEventListener("htmx:afterSettle", function () {
            // Only restore if no interactive element currently focused
            var active = document.activeElement;
            if (
                !active ||
                active === document.body ||
                INTERACTIVE_TAGS.indexOf(active.tagName) === -1
            ) {
                var field = document.getElementById("scan-field");
                if (field) {
                    field.focus();
                }
            }
        });
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", initFocusAttractor);
    } else {
        initFocusAttractor();
    }
})();
