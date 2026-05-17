// Fix #200 — fold / unfold support for the /locations tree.
//
// The Rust render (`src/routes/locations.rs::render_node_at_depth`)
// emits, for every parent node, a `<button class="tree-toggle">` with
// `data-tree-target="tree-children-<id>"` pointing at the wrapper
// `<div class="tree-children" id="tree-children-<id>">` that holds the
// node's children. This script does three things:
//
//   1. Click delegation: a single document-level listener flips the
//      target's hidden state, mirrors that into `aria-expanded` on the
//      button + flips the ▼/▶ icon, and persists the new state.
//   2. Persistence: the collapsed-node IDs are stored as a JSON array
//      under `mybibli-locations-collapsed`, scoped to localStorage so
//      it survives a reload but stays per-browser. The set is bounded
//      by the catalog's location count — no eviction logic needed.
//   3. Restore on page load: every persisted ID gets its container
//      hidden + button updated before the first paint after DOM ready.
//
// CSP-safe: pure addEventListener, no inline handlers, no eval, served
// from /static/js/ under `script-src 'self'`.

(function () {
    "use strict";

    const STORAGE_KEY = "mybibli-locations-collapsed";
    const ICON_EXPANDED = "▼";
    const ICON_COLLAPSED = "▶";

    function readCollapsedSet() {
        try {
            const raw = window.localStorage.getItem(STORAGE_KEY);
            if (!raw) return new Set();
            const parsed = JSON.parse(raw);
            if (!Array.isArray(parsed)) return new Set();
            return new Set(parsed.map(String));
        } catch (_) {
            // localStorage unavailable / parse error — fall back to a
            // fresh empty set; toggles still work for the current page,
            // just don't survive a reload.
            return new Set();
        }
    }

    function writeCollapsedSet(set) {
        try {
            window.localStorage.setItem(
                STORAGE_KEY,
                JSON.stringify(Array.from(set))
            );
        } catch (_) {
            // Quota or private mode — ignore. State stays correct in DOM.
        }
    }

    function findIconSpan(toggle) {
        return toggle.querySelector(".tree-toggle-icon");
    }

    function collapse(toggle, target) {
        target.classList.add("hidden");
        toggle.setAttribute("aria-expanded", "false");
        const icon = findIconSpan(toggle);
        if (icon) icon.textContent = ICON_COLLAPSED;
    }

    function expand(toggle, target) {
        target.classList.remove("hidden");
        toggle.setAttribute("aria-expanded", "true");
        const icon = findIconSpan(toggle);
        if (icon) icon.textContent = ICON_EXPANDED;
    }

    function nodeIdFromTargetId(targetId) {
        // `tree-children-<id>` → `<id>`. Returns null on shape mismatch.
        const prefix = "tree-children-";
        if (!targetId || !targetId.startsWith(prefix)) return null;
        return targetId.slice(prefix.length);
    }

    function applyPersistedState() {
        const collapsed = readCollapsedSet();
        if (collapsed.size === 0) return;
        for (const id of collapsed) {
            const target = document.getElementById("tree-children-" + id);
            if (!target) continue;
            const toggle = document.querySelector(
                `.tree-toggle[data-tree-target="tree-children-${CSS.escape(id)}"]`
            );
            if (!toggle) continue;
            collapse(toggle, target);
        }
    }

    function handleClick(event) {
        const toggle = event.target.closest(".tree-toggle");
        if (!toggle) return;
        event.preventDefault();
        const targetId = toggle.getAttribute("data-tree-target");
        if (!targetId) return;
        const target = document.getElementById(targetId);
        if (!target) return;

        const nodeId = nodeIdFromTargetId(targetId);
        const collapsed = readCollapsedSet();

        if (target.classList.contains("hidden")) {
            expand(toggle, target);
            if (nodeId) collapsed.delete(nodeId);
        } else {
            collapse(toggle, target);
            if (nodeId) collapsed.add(nodeId);
        }
        writeCollapsedSet(collapsed);
    }

    function init() {
        applyPersistedState();
        document.addEventListener("click", handleClick);
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
