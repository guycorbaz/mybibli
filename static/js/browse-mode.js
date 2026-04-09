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
      btn.setAttribute(
        "aria-checked",
        btn.dataset.browseMode === mode ? "true" : "false",
      );
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

  // Keyboard support for radiogroup
  document.addEventListener("keydown", function (e) {
    var focused = document.activeElement;
    if (!focused || !focused.dataset.browseMode) return;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      var newMode = focused.dataset.browseMode === "list" ? "grid" : "list";
      window.mybibliSetBrowseMode(newMode);
      var next = document.querySelector(
        '[data-browse-mode="' + newMode + '"]',
      );
      if (next) next.focus();
    }
  });

  // Apply on load and after HTMX swaps
  function init() {
    applyMode(getMode());
  }
  document.addEventListener("DOMContentLoaded", init);
  document.addEventListener("htmx:afterSettle", init);
})();
