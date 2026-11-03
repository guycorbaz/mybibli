// Browse mode toggle: list (table) / grid for search results and Recent additions
(function () {
  "use strict";
  var KEY = "mybibli_browse_mode";
  var DEFAULT = "list";

  function getMode() {
    return localStorage.getItem(KEY) || DEFAULT;
  }

  function applyMode(mode) {
    // #577 — every surface that renders both the browse table and the
    // card grid carries `data-browse-mode-container` (#browse-results and
    // the home page's #recent-additions-results); the CSS in browse.css
    // shows one rendering per the `.browse-list` / `.browse-grid` class.
    var containers = document.querySelectorAll("[data-browse-mode-container]");
    if (containers.length === 0) return;
    containers.forEach(function (container) {
      container.classList.remove("browse-list", "browse-grid");
      container.classList.add("browse-" + mode);
    });
    // CR #250 — list mode uses sortable column headers, so the
    // <select> sort dropdown is redundant; hide it via a class toggle
    // (CSP-friendly — strict policy blocks element.style.* writes,
    // see mybibli.js:73 for the same pattern). Grid mode shows it.
    var sortControl = document.getElementById("browse-sort-control");
    if (sortControl) {
      sortControl.classList.toggle("hidden", mode === "list");
    }
    // Update radiogroup
    var radios = document.querySelectorAll("[data-browse-mode]");
    radios.forEach(function (btn) {
      var isActive = btn.dataset.browseMode === mode;
      btn.setAttribute("aria-checked", isActive ? "true" : "false");
      btn.setAttribute("tabindex", isActive ? "0" : "-1");
      if (btn.dataset.browseMode === mode) {
        btn.classList.add(
          "bg-indigo-600",
          "text-white",
          "dark:bg-indigo-500",
        );
        btn.classList.remove(
          "text-stone-500",
          "dark:text-stone-400",
          "hover:bg-stone-100",
          "dark:hover:bg-stone-800",
        );
      } else {
        btn.classList.remove(
          "bg-indigo-600",
          "text-white",
          "dark:bg-indigo-500",
        );
        btn.classList.add(
          "text-stone-500",
          "dark:text-stone-400",
          "hover:bg-stone-100",
          "dark:hover:bg-stone-800",
        );
      }
    });
  }

  window.mybibliSetBrowseMode = function (mode) {
    localStorage.setItem(KEY, mode);
    applyMode(mode);
  };

  // Click-wire the radiogroup buttons. Pre-CSP these had inline onclick
  // attributes calling mybibliSetBrowseMode(...). Delegated at document
  // level so HTMX-injected groups also work.
  document.addEventListener("click", function (e) {
    var btn = e.target.closest && e.target.closest("[data-browse-mode]");
    if (!btn) return;
    window.mybibliSetBrowseMode(btn.dataset.browseMode);
  });

  // Keyboard support for radiogroup
  document.addEventListener("keydown", function (e) {
    var focused = document.activeElement;
    if (!focused || !focused.dataset.browseMode) return;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      // ArrowRight → next (list→grid), ArrowLeft → previous (grid→list)
      var newMode =
        e.key === "ArrowRight" ? "grid" : "list";
      window.mybibliSetBrowseMode(newMode);
      var next = document.querySelector(
        '[data-browse-mode="' + newMode + '"]',
      );
      if (next) next.focus();
    }
  });

  // Sort dropdown: building the "/" URL was previously inline. Read the
  // current `q` and `filter` from data-attributes the template rendered.
  function initSortDropdown() {
    var sel = document.getElementById("browse-sort-select");
    if (!sel || sel.dataset.wired === "true") return;
    sel.dataset.wired = "true";
    sel.addEventListener("change", function () {
      var parts = sel.value.split(":");
      var sort = parts[0] || "title";
      var dir = parts[1] || "asc";
      var q = sel.dataset.baseQ || "";
      var filter = sel.dataset.activeFilter || "";
      var url = "/?q=" + q + "&sort=" + sort + "&dir=" + dir + "&page=1";
      if (filter) url += "&filter=" + filter;
      // Story 13-7 (#586) — keep the "added since" date filter.
      var since = document.getElementById("browse-added-since");
      if (since && since.value) url += "&added_since=" + encodeURIComponent(since.value);
      window.location = url;
    });
  }

  // Story 13-7 (#586) — the "added since" GET form carries the browse
  // params of the page load as hidden inputs; a live search or an HTMX
  // sort since then changed them (and pushed them into the URL), so the
  // form re-reads them from the current URL just before it submits.
  function initAddedSinceForm() {
    var form = document.getElementById("browse-added-since-form");
    if (!form || form.dataset.wired === "true") return;
    form.dataset.wired = "true";
    // Indicator filters are left out: the date replaces an indicator view
    // (single-active-filter contract, the server would drop the date).
    var indicators = (form.dataset.indicatorFilters || "").split(" ");
    form.addEventListener("submit", function () {
      var params = new URLSearchParams(window.location.search);
      ["q", "filter", "sort", "dir"].forEach(function (name) {
        var old = form.querySelector('input[type="hidden"][name="' + name + '"]');
        if (old) old.remove();
        // The URL wins when it names the param; else the page-load value.
        var value = params.has(name) ? params.get(name) : old ? old.value : "";
        if (name === "filter" && (value === "" || indicators.indexOf(value) !== -1)) return;
        var input = document.createElement("input");
        input.type = "hidden";
        input.name = name;
        input.value = value;
        form.appendChild(input);
      });
    });
  }

  // Apply on load and after HTMX swaps
  function init() {
    applyMode(getMode());
    initSortDropdown();
    initAddedSinceForm();
  }
  document.addEventListener("DOMContentLoaded", init);
  document.addEventListener("htmx:afterSettle", init);
})();
