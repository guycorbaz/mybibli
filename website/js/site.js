// mybibli site — theme toggle + reveal-on-scroll + mobile nav.

(function () {
  // ---------- Theme toggle ----------
  const root = document.documentElement;
  const stored = localStorage.getItem("mybibli-site-theme");
  const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const initial = stored || (prefersDark ? "dark" : "light");
  root.classList.toggle("dark", initial === "dark");

  function setTheme(mode) {
    root.classList.toggle("dark", mode === "dark");
    localStorage.setItem("mybibli-site-theme", mode);
    document.querySelectorAll("[data-theme-toggle]").forEach((btn) => {
      btn.setAttribute("aria-pressed", mode === "dark" ? "true" : "false");
    });
  }

  document.addEventListener("click", (e) => {
    const btn = e.target.closest("[data-theme-toggle]");
    if (!btn) return;
    setTheme(root.classList.contains("dark") ? "light" : "dark");
  });

  // Initialize aria-pressed on all theme toggles after DOM is ready.
  document.addEventListener("DOMContentLoaded", () => {
    document.querySelectorAll("[data-theme-toggle]").forEach((btn) => {
      btn.setAttribute(
        "aria-pressed",
        root.classList.contains("dark") ? "true" : "false",
      );
    });
  });

  // ---------- Mobile nav ----------
  document.addEventListener("click", (e) => {
    const trigger = e.target.closest("[data-mobile-nav-toggle]");
    if (!trigger) return;
    const target = document.getElementById(trigger.getAttribute("aria-controls"));
    if (!target) return;
    const open = target.classList.toggle("hidden");
    trigger.setAttribute("aria-expanded", open ? "false" : "true");
  });

  // ---------- Reveal on scroll ----------
  if ("IntersectionObserver" in window) {
    const io = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.classList.add("in");
            io.unobserve(entry.target);
          }
        });
      },
      { threshold: 0.1 },
    );
    document.addEventListener("DOMContentLoaded", () => {
      document.querySelectorAll(".reveal").forEach((el) => io.observe(el));
    });
  } else {
    document.addEventListener("DOMContentLoaded", () => {
      document.querySelectorAll(".reveal").forEach((el) => el.classList.add("in"));
    });
  }
})();
