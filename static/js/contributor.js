// contributor.js — autocomplete for contributor name search
(function () {
    "use strict";

    var debounceTimer = null;
    var DEBOUNCE_MS = 300;
    var MIN_CHARS = 2;

    function initContributorAutocomplete() {
        // Observe DOM for dynamically added contributor forms
        var observer = new MutationObserver(function () {
            var input = document.getElementById("contributor-name-input");
            if (input && !input.dataset.autocompleteInit) {
                input.dataset.autocompleteInit = "true";
                setupAutocomplete(input);
            }
        });

        observer.observe(document.body, { childList: true, subtree: true });

        // Also check immediately
        var input = document.getElementById("contributor-name-input");
        if (input && !input.dataset.autocompleteInit) {
            input.dataset.autocompleteInit = "true";
            setupAutocomplete(input);
        }
    }

    function setupAutocomplete(input) {
        var dropdown = document.getElementById("contributor-autocomplete-dropdown");
        if (!dropdown) return;

        input.setAttribute("role", "combobox");
        input.setAttribute("aria-expanded", "false");
        input.setAttribute("aria-autocomplete", "list");
        input.setAttribute("aria-controls", "contributor-autocomplete-dropdown");
        dropdown.setAttribute("role", "listbox");

        input.addEventListener("input", function () {
            var query = input.value.trim();
            if (query.length < MIN_CHARS) {
                hideDropdown(dropdown, input);
                return;
            }

            clearTimeout(debounceTimer);
            debounceTimer = setTimeout(function () {
                fetchSuggestions(query, dropdown, input);
            }, DEBOUNCE_MS);
        });

        input.addEventListener("keydown", function (e) {
            if (e.key === "Escape") {
                hideDropdown(dropdown, input);
                e.stopImmediatePropagation();
                return;
            }

            var items = dropdown.querySelectorAll("[role='option']");
            var active = dropdown.querySelector("[aria-selected='true']");
            var idx = Array.prototype.indexOf.call(items, active);

            if (e.key === "ArrowDown") {
                e.preventDefault();
                var next = Math.min(idx + 1, items.length - 1);
                selectItem(items, next, dropdown, input);
            } else if (e.key === "ArrowUp") {
                e.preventDefault();
                var prev = Math.max(idx - 1, 0);
                selectItem(items, prev, dropdown, input);
            } else if (e.key === "Enter" && active) {
                e.preventDefault();
                e.stopImmediatePropagation();
                chooseItem(active, input, dropdown);
            }
        });

        // Close on click outside
        document.addEventListener("click", function (e) {
            if (!input.contains(e.target) && !dropdown.contains(e.target)) {
                hideDropdown(dropdown, input);
            }
        });
    }

    function fetchSuggestions(query, dropdown, input) {
        fetch("/catalog/contributors/search?q=" + encodeURIComponent(query))
            .then(function (r) { return r.json(); })
            .then(function (data) {
                renderDropdown(data, dropdown, input);
            })
            .catch(function () {
                hideDropdown(dropdown, input);
            });
    }

    function renderDropdown(items, dropdown, input) {
        dropdown.innerHTML = "";

        if (items.length === 0) {
            hideDropdown(dropdown, input);
            return;
        }

        items.forEach(function (item, idx) {
            var opt = document.createElement("div");
            opt.setAttribute("role", "option");
            opt.setAttribute("aria-selected", "false");
            opt.setAttribute("data-contributor-id", item.id);
            opt.className = "px-3 py-2 cursor-pointer hover:bg-indigo-50 dark:hover:bg-indigo-900/20 text-sm text-stone-700 dark:text-stone-300";
            opt.textContent = item.name;

            opt.addEventListener("click", function () {
                chooseItem(opt, input, dropdown);
            });

            dropdown.appendChild(opt);
        });

        dropdown.classList.remove("hidden");
        input.setAttribute("aria-expanded", "true");

        // Announce to screen readers
        dropdown.setAttribute("aria-live", "polite");
    }

    function selectItem(items, idx, dropdown, input) {
        items.forEach(function (item) {
            item.setAttribute("aria-selected", "false");
            item.classList.remove("bg-indigo-100", "dark:bg-indigo-900/40");
        });

        if (items[idx]) {
            items[idx].setAttribute("aria-selected", "true");
            items[idx].classList.add("bg-indigo-100", "dark:bg-indigo-900/40");
        }
    }

    function chooseItem(item, input, dropdown) {
        input.value = item.textContent;

        var hiddenId = document.getElementById("contributor-id-hidden");
        if (hiddenId) {
            hiddenId.value = item.getAttribute("data-contributor-id") || "";
        }

        hideDropdown(dropdown, input);
    }

    function hideDropdown(dropdown, input) {
        dropdown.innerHTML = "";
        dropdown.classList.add("hidden");
        input.setAttribute("aria-expanded", "false");
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", initContributorAutocomplete);
    } else {
        initContributorAutocomplete();
    }
})();
