// Browse mode toggle: list/grid for search results
(function () {
  "use strict";
  var KEY = "mybibli_browse_mode";
  var DEFAULT = "list";

  function getMode() {
    return localStorage.getItem(KEY) || DEFAULT;
  }

  function applyMode(mode) {
    var container = document.getElementById("browse-results");
    if (!container) return;
    container.classList.remove("browse-list", "browse-grid");
    container.classList.add("browse-" + mode);
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
      window.location = url;
    });
  }

  // Apply on load and after HTMX swaps
  function init() {
    applyMode(getMode());
    initSortDropdown();
  }
  document.addEventListener("DOMContentLoaded", init);
  document.addEventListener("htmx:afterSettle", init);
})();
