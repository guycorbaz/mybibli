// mybibli application entry point
(function () {
    "use strict";

    function initKeyboardShortcuts() {
        document.addEventListener("keydown", function (e) {
            // Ctrl+K / Cmd+K → navigate to /catalog
            if ((e.ctrlKey || e.metaKey) && e.key === "k") {
                var role = document.body.dataset.userRole;
                if (role === "librarian" || role === "admin") {
                    e.preventDefault();
                    window.location.href = "/catalog";
                }
            }
        });
    }

    function init() {
        initKeyboardShortcuts();
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
