// borrowers.js — show/hide the "add borrower" inline form on /borrowers.
//
// Pre-CSP these were inline onclick handlers on the show/cancel buttons.
// Strict `script-src 'self'` requires them to be wired from an external
// module. Idempotent: only runs when both elements are present and
// guards against double-binding.
(function () {
    "use strict";

    function init() {
        var form = document.getElementById("add-form");
        if (!form) return;

        var show = document.getElementById("borrowers-show-add-form");
        if (show && show.dataset.wired !== "true") {
            show.dataset.wired = "true";
            show.addEventListener("click", function (e) {
                e.preventDefault();
                form.classList.remove("hidden");
                var name = document.getElementById("new-name");
                if (name) name.focus();
            });
        }

        var hide = document.getElementById("borrowers-hide-add-form");
        if (hide && hide.dataset.wired !== "true") {
            hide.dataset.wired = "true";
            hide.addEventListener("click", function () {
                form.classList.add("hidden");
            });
        }
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
