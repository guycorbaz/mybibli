// inline-form.js — UX-DR21 InlineForm component (story 8-4).
//
// Powers the Reference Data tab's four CRUD sub-sections (genres, volume
// states, contributor roles, location node types). Pure delegated event
// handlers, CSP-clean (no inline handlers, no inline styles, no globals).
// All server I/O routes through htmx.ajax so the existing CSRF + OOB-swap
// plumbing applies unchanged.
//
// Handlers (matched by `data-action` attributes):
//   - inline-form-add-toggle    → reveal/hide the Add form, focus its input.
//   - inline-form-add-cancel    → close the Add form, reset its input.
//   - inline-form-edit          → swap a row's name <span> for a text input.
//   - inline-form-edit-cancel   → revert the input back to a span (no roundtrip).
//
// Delete + loanable-toggle use straight HTMX attributes on the buttons
// themselves (hx-get for the modal, hx-post for the toggle); no JS handler
// needed beyond the framework's own.
(function () {
    "use strict";

    if (window.__mybibliInlineFormWired) return;
    window.__mybibliInlineFormWired = true;

    // Story 8-4 P37 (D6-b): refuse concurrent modal opens. The
    // `#admin-modal-slot` is a single global slot; opening modal B while A
    // is open silently destroyed A's state (incl. typed delete-confirmation
    // text). We block the second request at htmx:beforeRequest level and
    // surface a localized feedback message. Localized strings follow the
    // CLAUDE.md "read <html lang> and use embedded string map" idiom.
    var MODAL_BUSY_MESSAGES = {
        en: "Please close the current dialog before opening another.",
        fr: "Veuillez fermer la fenêtre en cours avant d'en ouvrir une autre.",
    };
    function getModalBusyMessage() {
        var lang = (document.documentElement.lang || "en").toLowerCase();
        return MODAL_BUSY_MESSAGES[lang] || MODAL_BUSY_MESSAGES.en;
    }
    function modalSlotIsOccupied() {
        var slot = document.getElementById("admin-modal-slot");
        if (!slot) return false;
        // A `<dialog open>` element in the slot indicates an active modal.
        return !!slot.querySelector("dialog[open]");
    }
    document.body.addEventListener("htmx:beforeRequest", function (evt) {
        var elt = evt.detail && evt.detail.elt;
        if (!elt || !elt.getAttribute) return;
        if (elt.getAttribute("hx-target") !== "#admin-modal-slot") return;
        if (!modalSlotIsOccupied()) return;
        evt.preventDefault();
        var fbList = document.getElementById("feedback-list");
        if (!fbList) return;
        var entry = document.createElement("div");
        entry.className = "feedback-entry p-3 border-l-4 border-amber-500 bg-amber-50 dark:bg-amber-900/20 rounded-r";
        entry.setAttribute("role", "status");
        entry.setAttribute("data-feedback-variant", "warning");
        var msg = document.createElement("p");
        msg.className = "text-stone-700 dark:text-stone-300";
        msg.textContent = getModalBusyMessage();
        entry.appendChild(msg);
        fbList.appendChild(entry);
        setTimeout(function () {
            if (entry.parentNode) entry.parentNode.removeChild(entry);
        }, 5000);
    }, false);

    document.body.addEventListener("click", function (evt) {
        var target = evt.target;
        if (!target) return;

        var action = target.getAttribute && target.getAttribute("data-action");
        if (!action) {
            // Allow event delegation from a child icon by walking up until we
            // hit an element carrying data-action (or until we leave the body).
            var node = target;
            while (node && node !== document.body) {
                if (node.getAttribute && node.getAttribute("data-action")) {
                    target = node;
                    action = node.getAttribute("data-action");
                    break;
                }
                node = node.parentElement;
            }
        }
        if (!action) return;

        if (action === "inline-form-add-toggle") {
            evt.preventDefault();
            var slot = document.getElementById(target.getAttribute("data-slot"));
            if (!slot) return;
            slot.classList.toggle("hidden");
            if (!slot.classList.contains("hidden")) {
                var input = slot.querySelector('input[type="text"], input:not([type])');
                if (input) input.focus();
            }
            return;
        }

        if (action === "inline-form-add-cancel") {
            evt.preventDefault();
            var slotId = target.getAttribute("data-slot");
            var addSlot = slotId ? document.getElementById(slotId) : null;
            if (!addSlot) return;
            var nameInput = addSlot.querySelector('input[name="name"]');
            if (nameInput) nameInput.value = "";
            addSlot.classList.add("hidden");
            return;
        }

        if (action === "inline-form-edit") {
            evt.preventDefault();
            startInlineEdit(target);
            return;
        }

        if (action === "inline-form-edit-cancel") {
            evt.preventDefault();
            var rowId = target.getAttribute("data-row-id");
            if (rowId) cancelInlineEdit(document.getElementById(rowId));
            return;
        }

        if (action === "admin-modal-close") {
            evt.preventDefault();
            var slot = document.getElementById("admin-modal-slot");
            if (slot) slot.innerHTML = "";
            return;
        }

        if (action === "admin-modal-close-revert-row") {
            evt.preventDefault();
            // Story 8-4 P14: close modal AFTER the revert HTMX call resolves
            // (or rejects) so the user does not see the row in its flicker
            // state during the refetch. Disable the button while in-flight to
            // prevent double-clicks. If the revert fails, still close the
            // modal so the user is not stranded.
            var revertTarget = target.getAttribute("data-row-revert-target");
            var revertEndpoint = target.getAttribute("data-row-revert-endpoint");
            var slot2 = document.getElementById("admin-modal-slot");
            var closeModal = function () {
                if (slot2) slot2.innerHTML = "";
            };
            if (revertEndpoint && revertTarget && window.htmx) {
                target.disabled = true;
                var p = window.htmx.ajax("GET", revertEndpoint, {
                    target: revertTarget,
                    swap: "outerHTML",
                });
                if (p && typeof p.then === "function") {
                    p.then(closeModal, closeModal);
                } else {
                    closeModal();
                }
            } else {
                closeModal();
            }
            return;
        }
    }, false);

    // Story 8-4 P15: <dialog open aria-modal="true"> does NOT trap Escape
    // automatically (only <dialog>.showModal() does). Add a document-level
    // listener so Escape closes the active admin modal — and, when the
    // close button carries a `revert-row` data-action, triggers the
    // associated revert by synthesizing a click instead of duplicating the
    // close-with-revert logic here.
    document.body.addEventListener("keydown", function (evt) {
        if (evt.key !== "Escape") return;
        var slot = document.getElementById("admin-modal-slot");
        if (!slot) return;
        var dialog = slot.querySelector("dialog[open]");
        if (!dialog) return;
        var closeBtn = dialog.querySelector(
            '[data-action="admin-modal-close-revert-row"], [data-action="admin-modal-close"]'
        );
        if (closeBtn) {
            evt.preventDefault();
            closeBtn.click();
        } else {
            evt.preventDefault();
            slot.innerHTML = "";
        }
    }, false);

    // Story 8-4 P22: WAI-ARIA contract for `role="button"` requires BOTH
    // Enter AND Space to activate. Without Space, the row span is keyboard-
    // unreachable for users following the standard "tab to focus, Space to
    // activate" pattern.
    document.body.addEventListener("keydown", function (evt) {
        var t = evt.target;
        if (!t || !t.getAttribute) return;
        var action = t.getAttribute("data-action");
        if (action === "inline-form-edit" && (evt.key === "Enter" || evt.key === " ")) {
            evt.preventDefault();
            startInlineEdit(t);
        }
    }, false);

    function startInlineEdit(span) {
        if (!span) return;
        if (span.dataset.editing === "1") return;
        // Story 8-4 P8: guard against detached / re-swapped DOM. If the row
        // was OOB-replaced (e.g., after a successful rename) but our click
        // handler still has a stale reference to the old span, calling
        // span.parentNode.insertBefore() below would throw. Bail out
        // silently — the user can click again on the new row.
        if (!span.parentNode || !span.isConnected) return;
        var rowId = span.getAttribute("data-row-id");
        var endpoint = span.getAttribute("data-rename-endpoint");
        var version = span.getAttribute("data-version");
        var currentName = span.textContent || "";
        if (!rowId || !endpoint || version === null) return;

        span.dataset.editing = "1";

        var form = document.createElement("form");
        form.setAttribute("data-inline-edit-form", rowId);
        form.className = "flex gap-2 items-center";

        var input = document.createElement("input");
        input.type = "text";
        input.name = "name";
        input.value = currentName.trim();
        input.required = true;
        input.maxLength = 255;
        input.className = "px-2 py-1 border border-stone-300 dark:border-stone-700 rounded-md dark:bg-stone-800";
        form.appendChild(input);

        var versionInput = document.createElement("input");
        versionInput.type = "hidden";
        versionInput.name = "version";
        versionInput.value = version;
        form.appendChild(versionInput);

        // CSRF token from the meta tag — same idiom as csrf.js.
        var meta = document.querySelector('meta[name="csrf-token"]');
        var csrfInput = document.createElement("input");
        csrfInput.type = "hidden";
        csrfInput.name = "_csrf_token";
        csrfInput.value = meta ? (meta.getAttribute("content") || "") : "";
        form.appendChild(csrfInput);

        var saveBtn = document.createElement("button");
        saveBtn.type = "submit";
        saveBtn.className = "px-2 py-1 bg-blue-600 text-white rounded-md text-sm";
        saveBtn.textContent = (input.getAttribute("data-save-label") || "Save");
        form.appendChild(saveBtn);

        var cancelBtn = document.createElement("button");
        cancelBtn.type = "button";
        cancelBtn.setAttribute("data-action", "inline-form-edit-cancel");
        cancelBtn.setAttribute("data-row-id", rowId);
        cancelBtn.className = "px-2 py-1 bg-stone-300 dark:bg-stone-700 rounded-md text-sm";
        cancelBtn.textContent = (input.getAttribute("data-cancel-label") || "Cancel");
        form.appendChild(cancelBtn);

        // Stash original markup so cancel can restore it.
        span.dataset.originalText = currentName;
        span.style.display = "none";
        span.parentNode.insertBefore(form, span.nextSibling);

        input.focus();
        input.select();

        input.addEventListener("keydown", function (e) {
            if (e.key === "Escape") {
                e.preventDefault();
                cancelInlineEdit(span);
            }
        });

        form.addEventListener("submit", function (e) {
            e.preventDefault();
            if (!window.htmx) return;
            window.htmx.ajax("POST", endpoint, {
                source: form,
                target: "#" + rowId,
                swap: "outerHTML",
                values: {
                    name: input.value,
                    version: version,
                    _csrf_token: csrfInput.value,
                },
            });
        });
    }

    function cancelInlineEdit(span) {
        if (!span) return;
        var rowId = span.getAttribute("data-row-id");
        var form = document.querySelector('[data-inline-edit-form="' + cssEscape(rowId) + '"]');
        if (form && form.parentNode) form.parentNode.removeChild(form);
        span.style.display = "";
        delete span.dataset.editing;
    }

    // Story 8-4 P7: prefer the standard CSS.escape (widely supported since
    // 2017 — Chrome 46, Firefox 31, Safari 10). Falls back to a hand-rolled
    // escape only on legacy browsers where CSS.escape is missing. The old
    // hand-rolled escape produced invalid backslash sequences for hyphens
    // and was a foot-gun for any future ID scheme drift.
    function cssEscape(value) {
        if (!value) return "";
        if (typeof CSS !== "undefined" && typeof CSS.escape === "function") {
            return CSS.escape(value);
        }
        return String(value).replace(/[^a-zA-Z0-9_-]/g, function (ch) {
            return "\\" + ch;
        });
    }
})();
