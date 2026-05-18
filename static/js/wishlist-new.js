// CR #242 — toggle the two sub-forms ("By ISBN" vs "Free-form") on
// /wishlist/new. CSP-clean: no inline handlers, no eval. Idempotent
// via `dataset.wired` on the picker container — surviving an HTMX
// swap would re-run init without duplicating listeners.
(function () {
    "use strict";

    function init() {
        var buttons = document.querySelectorAll("[data-wishlist-mode]");
        if (buttons.length === 0) return;
        var picker = buttons[0].parentElement;
        if (picker && picker.dataset.wired === "true") return;
        if (picker) picker.dataset.wired = "true";

        var isbnSection = document.getElementById("wishlist-mode-isbn");
        var freeformSection = document.getElementById("wishlist-mode-freeform");

        function activate(mode) {
            for (var i = 0; i < buttons.length; i++) {
                var btn = buttons[i];
                var isActive = btn.dataset.wishlistMode === mode;
                btn.setAttribute("aria-pressed", isActive ? "true" : "false");
                if (isActive) {
                    btn.classList.remove(
                        "border-stone-300",
                        "dark:border-stone-600",
                        "text-stone-700",
                        "dark:text-stone-300"
                    );
                    btn.classList.add(
                        "border-indigo-300",
                        "dark:border-indigo-700",
                        "text-indigo-700",
                        "dark:text-indigo-300",
                        "bg-indigo-50",
                        "dark:bg-indigo-900/30"
                    );
                } else {
                    btn.classList.remove(
                        "border-indigo-300",
                        "dark:border-indigo-700",
                        "text-indigo-700",
                        "dark:text-indigo-300",
                        "bg-indigo-50",
                        "dark:bg-indigo-900/30"
                    );
                    btn.classList.add(
                        "border-stone-300",
                        "dark:border-stone-600",
                        "text-stone-700",
                        "dark:text-stone-300"
                    );
                }
            }
            if (isbnSection) isbnSection.hidden = mode !== "isbn";
            if (freeformSection) freeformSection.hidden = mode !== "freeform";
        }

        for (var j = 0; j < buttons.length; j++) {
            (function (b) {
                b.addEventListener("click", function () {
                    activate(b.dataset.wishlistMode);
                });
            })(buttons[j]);
        }
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
