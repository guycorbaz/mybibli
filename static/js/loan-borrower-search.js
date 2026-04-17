// loan-borrower-search.js — borrower autocomplete + new-loan form toggle +
// post-return reload for /loans.
//
// Pre-CSP this lived as an inline <script> at the bottom of
// templates/pages/loans.html plus an inline onclick on the new-loan
// toggle button. Strict `script-src 'self'` requires it to be a
// same-origin module.
(function () {
    "use strict";

    function initLoansPage() {
        if (document.body.dataset.loansWired === "true") return;
        // Only wire on pages that actually expose the loans UI.
        var page = document.body.dataset.page;
        if (page !== "loans") return;
        document.body.dataset.loansWired = "true";

        wireNewLoanToggle();
        wireBorrowerAutocomplete();
        wireScanFieldClear();
        wireReturnReload();
    }

    function wireNewLoanToggle() {
        var btn = document.getElementById("loans-new-loan-toggle");
        var form = document.getElementById("new-loan-form");
        if (!btn || !form) return;
        btn.addEventListener("click", function () {
            form.classList.toggle("hidden");
            var label = document.getElementById("loan-volume-label");
            if (label) label.focus();
        });
    }

    function wireBorrowerAutocomplete() {
        var searchInput = document.getElementById("loan-borrower-search");
        var hiddenInput = document.getElementById("loan-borrower-id");
        var dropdown = document.getElementById("borrower-dropdown");
        if (!searchInput || !hiddenInput || !dropdown) return;

        var debounceTimer;
        searchInput.addEventListener("input", function () {
            var q = searchInput.value.trim();
            clearTimeout(debounceTimer);
            hiddenInput.value = "";
            if (q.length < 2) {
                dropdown.classList.add("hidden");
                dropdown.innerHTML = "";
                return;
            }
            debounceTimer = setTimeout(function () {
                fetch("/borrowers/search?q=" + encodeURIComponent(q))
                    .then(function (resp) { return resp.ok ? resp.json() : null; })
                    .then(function (data) {
                        if (!data) return;
                        dropdown.innerHTML = "";
                        if (data.length === 0) {
                            dropdown.classList.add("hidden");
                            return;
                        }
                        data.forEach(function (b) {
                            var item = document.createElement("div");
                            item.className = "px-3 py-2 cursor-pointer hover:bg-stone-100 dark:hover:bg-stone-600 text-sm text-stone-900 dark:text-stone-100";
                            item.textContent = b.name;
                            item.addEventListener("click", function () {
                                searchInput.value = b.name;
                                hiddenInput.value = b.id;
                                dropdown.classList.add("hidden");
                            });
                            dropdown.appendChild(item);
                        });
                        dropdown.classList.remove("hidden");
                    })
                    .catch(function () { /* swallow network errors */ });
            }, 300);
        });

        document.addEventListener("click", function (e) {
            if (!searchInput.contains(e.target) && !dropdown.contains(e.target)) {
                dropdown.classList.add("hidden");
            }
        });
    }

    function wireScanFieldClear() {
        var scan = document.getElementById("loan-scan-field");
        if (!scan) return;
        // Fire the custom `loan-scan-fire` event on Enter — replaces the
        // pre-CSP `hx-trigger="keydown[key=='Enter'] from:this"` filter
        // expression, which htmx evaluates as JS (blocked by CSP eval rule).
        scan.addEventListener("keydown", function (e) {
            if (e.key !== "Enter") return;
            e.preventDefault();
            scan.dispatchEvent(new CustomEvent("loan-scan-fire"));
        });
        scan.addEventListener("htmx:afterRequest", function () {
            setTimeout(function () { scan.value = ""; scan.focus(); }, 100);
        });
    }

    // After a successful loan return, reload preserving sort/dir params.
    function wireReturnReload() {
        document.body.addEventListener("htmx:afterRequest", function (e) {
            var detail = e.detail;
            if (!detail || !detail.pathInfo || !detail.pathInfo.requestPath) return;
            if (!detail.pathInfo.requestPath.includes("/return")) return;
            if (!detail.successful) return;
            setTimeout(function () {
                var params = new URLSearchParams(window.location.search);
                params.set("page", "1");
                window.location.search = params.toString();
            }, 1500);
        });
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", initLoansPage);
    } else {
        initLoansPage();
    }
})();
