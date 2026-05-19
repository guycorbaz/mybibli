// CR #261 — persist the user's expanded / collapsed choice for the home
// page's "Recent additions" <details> across reloads. Mirrors the
// locations-tree fold pattern (#200). CSP-clean: no inline handlers,
// no eval, all listeners via addEventListener.
//
// Storage key per `data-home-fold-key` attribute on the <details>
// element so future fold-able sections on the same page can ride the
// same JS without colliding.
(function () {
    "use strict";

    var STORAGE_PREFIX = "mybibli.home.fold.";

    function init() {
        var detailsEls = document.querySelectorAll("details[data-home-fold-key]");
        for (var i = 0; i < detailsEls.length; i++) {
            wire(detailsEls[i]);
        }
    }

    function wire(detailsEl) {
        if (detailsEl.dataset.wired === "true") return;
        detailsEl.dataset.wired = "true";

        var key = STORAGE_PREFIX + detailsEl.dataset.homeFoldKey;
        // Restore: only OPEN on the explicit "open" sentinel; default
        // (no entry OR any other value) stays closed per the CR brief.
        try {
            if (window.localStorage && window.localStorage.getItem(key) === "open") {
                detailsEl.setAttribute("open", "");
            }
        } catch (_) {
            /* localStorage may throw in privacy modes — fall back to default-closed. */
        }

        detailsEl.addEventListener("toggle", function () {
            try {
                if (!window.localStorage) return;
                window.localStorage.setItem(key, detailsEl.open ? "open" : "closed");
            } catch (_) {
                /* ignore — fold state isn't critical. */
            }
        });
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
