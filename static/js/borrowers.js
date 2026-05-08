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

        function showForm(e) {
            if (e) e.preventDefault();
            form.classList.remove("hidden");
            var name = document.getElementById("new-name");
            if (name) name.focus();
        }

        var show = document.getElementById("borrowers-show-add-form");
        if (show && show.dataset.wired !== "true") {
            show.dataset.wired = "true";
            show.addEventListener("click", showForm);
        }

        // Story 9-15 — the empty-state StatusMessage CTA also targets
        // `#add-form`. Without this listener the CTA scrolls to the
        // anchor but the form stays `display: none` (Tailwind `.hidden`),
        // creating a UX dead-end on the encouraging empty state.
        var emptyCtas = document.querySelectorAll('a[href="#add-form"]');
        emptyCtas.forEach(function (cta) {
            if (cta.dataset.wired === "true") return;
            cta.dataset.wired = "true";
            cta.addEventListener("click", showForm);
        });

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
