# Story 20.1: Remove an entry from the wish list (closes #714)

Status: ready-for-dev

## Story

As a librarian,
I want to remove a wish list entry I no longer want, without saying I bought it,
so that the list holds only books I still want, and its history says what really happened.

## Context

Usage feedback of 2026-11-25 (#714). Covers FR183. First story of Epic 20 (released with Epic 19 as v1.29.0).

What exists (read on `main` at `a608f56`):
- **Routes** — `src/routes/wishlist.rs` (677 lines). The list `wishlist_page`, the entry page `wishlist_detail`, the modal `wishlist_delete_modal` (`GET /wishlist/{id}/delete-modal`, HTMX only, else 405) and the soft-delete `wishlist_delete` (`DELETE /wishlist/{id}`), registered in `src/routes/mod.rs:452-457`. `wishlist_delete` reads the item (`find_by_id`, `None` → `AppError::NotFound(error.not_found)`), calls `WishlistItemModel::soft_delete(pool, id, version)` with the version it just read (`0` rows → `AppError::Conflict(error.conflict)`), writes the `admin_audit` row `wishlist_bought` (best effort, `{"title", "isbn"}`), then answers `200` + `HX-Redirect: /wishlist`.
- **One action only.** Every surface opens the same "bought" modal: the table row and the mobile card in `templates/pages/wishlist.html` (`✓ {{ label_action_bought }}`, `hx-get="/wishlist/{{ item.id }}/delete-modal"`, `hx-target="#modal-slot"`), the entry page `templates/pages/wishlist_detail.html`, and the scan banner built in Rust by `wishlist::autolink_feedback_html` (button `wishlist.autolink_remove`, "Remove from wish list").
- **Modal** — `templates/fragments/wishlist_delete_modal.html` calls the UX-DR8 macro `components/modal.html` with variant `"delete"`, method `"DELETE"`, `hx_target="#wishlist-feedback"`, version `0`. A failed Confirm lands in the modal's `[data-modal-error]` (`static/js/modal.js` `htmx:afterRequest`, Path A; `ModalConfirmRetargetGuard` strips `HX-Retarget` from the error answer).
- **False Trash claim.** `wishlist.delete_modal_body` says the entry can be restored from the Trash for 30 days (all four locales). `wishlist_items` is in neither `services::soft_delete::ALLOWED_TABLES` nor `PURGE_DELETION_ORDER`: it cannot.
- **Model** — `src/models/wishlist.rs`: `find_by_id` (live only), `soft_delete` (`UPDATE wishlist_items SET deleted_at = NOW(), version = version + 1 WHERE id = ? AND version = ? AND deleted_at IS NULL`, returns rows affected; test `soft_delete_with_wrong_version_returns_zero`). Wish list items are never edited: their version moves only through `soft_delete`.
- **Locales** — `wishlist.action_remove` already exists in the four files ("Remove" / "Retirer" / "Entfernen" / "Rimuovi") and is unused: reuse it, do not add a duplicate.
- **E2E** — `tests/e2e/specs/journeys/wishlist.spec.ts` clicks `getByRole("button", { name: /Bought|Acheté/i })`; the new accessible names keep that word ("Mark “…” as bought" / "… comme acheté"), so it still matches — check it, do not edit it beyond what breaks.

## Decisions

- **D1 — a separate "Remove" next to "Bought".** *(Approved by the project lead, 2026-11-26.)* Each action has its own modal copy and its own audit action: `wishlist_removed` (Remove) and `wishlist_bought` (Bought, unchanged). The scan banner keeps the "bought" modal: a scanned ISBN means the book is in hand.
- **D2 — fix the false Trash text.** *(Approved by the project lead, 2026-11-26.)* Neither modal claims a restore. Putting wish list items in the Trash (and purging removed rows) is a change request of its own, if ever wanted — not this story.
- **D3 — keep the full reload.** *(Approved by the project lead, 2026-11-26.)* After Confirm, `HX-Redirect: /wishlist`, for both actions and from both pages.
- **D4 — copy (EN / FR).** *(Approved by the project lead, 2026-11-26, epic 20 D11.)* DE and IT are translated in this story (drafts below) and reviewed like the others.

  | Key | EN | FR |
  |---|---|---|
  | `wishlist.action_remove` (exists) | Remove | Retirer |
  | `wishlist.action_remove_named` (aria-label) | Remove “%{title}” from the wish list | Retirer « %{title} » de la wish list |
  | `wishlist.action_bought_named` (aria-label) | Mark “%{title}” as bought | Marquer « %{title} » comme acheté |
  | `wishlist.remove_modal_title` | Remove “%{title}” from the wish list? | Retirer « %{title} » de la wish list ? |
  | `wishlist.remove_modal_body` | The entry leaves the wish list. It cannot be restored: add it again if you change your mind. | L’entrée quitte la wish list. Elle ne peut pas être restaurée : ajoutez-la à nouveau si vous changez d’avis. |
  | `wishlist.delete_modal_body` (fixed, "bought") | The entry leaves the wish list. It cannot be restored afterwards. | L’entrée quitte la wish list. Elle ne pourra pas être restaurée ensuite. |
  | `wishlist.already_removed` | This entry is no longer on the wish list. It was probably removed in another tab or by another librarian. Reload the list to see it as it is now. | Cette entrée n’est plus dans la wish list. Elle a sans doute été retirée dans un autre onglet ou par un autre bibliothécaire. Rechargez la liste pour la voir à jour. |

  DE / IT drafts (to review in the PR):
  - `action_remove_named`: DE "„%{title}“ von der Wunschliste entfernen" · IT "Rimuovi «%{title}» dalla lista dei desideri"
  - `action_bought_named`: DE "„%{title}“ als gekauft markieren" · IT "Segna «%{title}» come acquistato"
  - `remove_modal_title`: DE "„%{title}“ von der Wunschliste entfernen?" · IT "Rimuovere «%{title}» dalla lista dei desideri?"
  - `remove_modal_body`: DE "Der Eintrag verlässt die Wunschliste. Er kann nicht wiederhergestellt werden: Fügen Sie ihn erneut hinzu, wenn Sie es sich anders überlegen." · IT "La voce lascia la lista dei desideri. Non può essere ripristinata: aggiungila di nuovo se cambi idea."
  - `delete_modal_body`: DE "Der Eintrag verlässt die Wunschliste. Er kann danach nicht wiederhergestellt werden." · IT "La voce lascia la lista dei desideri. Non potrà essere ripristinata in seguito."
  - `already_removed`: DE "Dieser Eintrag steht nicht mehr auf der Wunschliste. Er wurde wahrscheinlich in einem anderen Tab oder von einer anderen Person entfernt. Laden Sie die Liste neu, um den aktuellen Stand zu sehen." · IT "Questa voce non è più nella lista dei desideri. Probabilmente è stata rimossa in un'altra scheda o da un altro bibliotecario. Ricarica la lista per vederla aggiornata."
- **D5 — the action parameter.** *(From the epic, approved 2026-11-26.)* `GET /wishlist/{id}/delete-modal?action=remove|bought`, default `bought`. The parser accepts exactly `remove` and `bought` (Rule #20, positive shape); anything else — absent, empty, `REMOVE`, `delete` — is `bought`. The modal's `action_url` carries the same value (`/wishlist/{id}?action=remove`), and `wishlist_delete` reads it through the same parser (DRY: one `WishlistAction::from_query`), so the audit action matches the modal the librarian confirmed. No route is added or renamed.
- **D6 — what "Remove" looks like.** *(Approved by the project lead, 2026-11-26: option A.)* The epic names the action, not its look.
  - **A (recommended):** on the list row and the mobile card, "Remove" is a text button right after "✓ Bought", same size, in red (`text-red-600 dark:text-red-400`, no icon), separated by a gap; on the entry page, a bordered secondary button after "✓ Bought" (`border-red-300 text-red-700`, same height). The remove modal uses the macro variant `"delete"` (red Confirm) with the confirm label `wishlist.action_remove` ("Remove" / "Retirer"). The "bought" modal keeps variant `"delete"` and its "Bought" confirm label (unchanged).
  - B: "Remove" in neutral stone, modal variant `"remove"` (amber). Softer, but the removal cannot be undone (D2), which is what red says elsewhere in mybibli.
- **D7 — a modal asked for an entry already gone.** *(Approved by the project lead, 2026-11-26: option A.)* When "Bought" or "Remove" is clicked on a list that is out of date (the entry was removed in another tab), `GET …/delete-modal` finds nothing. Today it answers the generic `error.not_found` ("We couldn't find what you were looking for — it may have been moved or deleted.") in `#feedback-list`.
  - **A (recommended):** answer `wishlist.already_removed` there instead (no new copy; the same sentence as the Confirm refusal, AC5). Still a 404 through `AppError::NotFound`, so the `htmx:responseError` path inserts it (story 18-5).
  - B: keep the generic message.
- **D8 — quotes in the "bought" modal title.** *(Approved by the project lead, 2026-11-26: option A.)* The approved remove title quotes the entry ("Remove “%{title}” …?"); the "bought" title today does not ("Mark %{title} as bought?").
  - **A (recommended):** align it — EN "Mark “%{title}” as bought?", FR « Marquer « %{title} » comme acheté ? », DE "„%{title}“ als gekauft markieren?", IT "Segnare «%{title}» come acquistato?".
  - B: leave `wishlist.delete_modal_title` as it is.

## Acceptance Criteria

1. **AC1 — two actions everywhere an entry is shown to a Librarian or Admin.** The `/wishlist` table row, the mobile card and `/wishlist/{id}` each show "✓ Bought" and "Remove". Each button's accessible name includes the entry's title: `aria-label` = `wishlist.action_bought_named` / `wishlist.action_remove_named` (WCAG 2.5.3: the visible word is part of the name). "Bought" opens `…/delete-modal?action=bought` (or no parameter), "Remove" opens `…/delete-modal?action=remove`, both `hx-target="#modal-slot" hx-swap="innerHTML"`.
2. **AC2 — the remove modal.** It opens in `#modal-slot` through the UX-DR8 macro (Pattern A; scanner-guard protects it; no `hx-confirm=`), titled `remove_modal_title`, body `remove_modal_body`, confirm label `action_remove`, Cancel first and default focus (macro invariant).
3. **AC3 — Confirm.** The entry is soft-deleted (`WishlistItemModel::soft_delete`, version-guarded); one `admin_audit` row is written: `wishlist_removed` after "Remove", `wishlist_bought` after "Bought" (both `entity_type = "wishlist_items"`, details `{"title", "isbn"}`); the answer is `HX-Redirect: /wishlist` (D3); the entry is gone from the list, its siblings stay.
4. **AC4 — no Trash claim.** Neither modal's text mentions the Trash or a restore window, in any of the four locales (`delete_modal_body` replaced per D4; the remove copy says it cannot be restored).
5. **AC5 — already removed.** If the entry is gone when Confirm is sent (removed in another tab or by another librarian: `find_by_id` → `None`, or `soft_delete` → 0 rows), nothing is written (no row change, no audit row) and the answer is `wishlist.already_removed`, shown inside the modal's `[data-modal-error]`; the modal stays open. Both former messages (`error.not_found`, `error.conflict`) are gone from this handler. With D7-A, the modal opener answers the same sentence in `#feedback-list`.
6. **AC6 — roles.** Anonymous sees neither button (the list stays browsable); `GET …/delete-modal` keeps `require_role_with_return(Role::Librarian, "/wishlist", …)`; `DELETE /wishlist/{id}` keeps `require_role(Role::Librarian, …)` and the CSRF check (state-changing method).
7. **AC7 — the scan banner is unchanged.** `autolink_feedback_html` still opens the "bought" modal (`/wishlist/{id}/delete-modal`, no parameter or `?action=bought`); its button text stays `wishlist.autolink_remove`.
8. **AC8 — the parameter (D5).** `remove` → remove; `bought`, absent or any other value → bought; the same parser on the modal and the delete routes. Unknown query fields (htmx 2 appends `_csrf_token` to a DELETE URL) are ignored, not refused.
9. **AC9 — tests.**
   - Unit (`src/routes/wishlist.rs` test module): the parser's accepted shapes and fallbacks; the modal copy per action (title, body, confirm label) in the four locales; no locale's `delete_modal_body` / `remove_modal_body` contains the locale's Trash word or "30" (EN "Trash", FR "Corbeille", DE "Papierkorb", IT "Cestino").
   - DB integration, new `tests/wishlist_remove.rs` declared in `tests/integration.rs` (`crate::common`, `common::web::Browser`): Remove writes `wishlist_removed`, Bought writes `wishlist_bought`, each removes only its entry; a second DELETE on the same entry answers `already_removed` and writes no audit row; a delete whose `soft_delete` matches 0 rows (version bumped between read and write, driven through the model) changes nothing; `action=REMOVE` / `action=x` record `wishlist_bought`; Anonymous gets no button in the list HTML and is bounced from the modal; a Librarian sees both buttons with their named labels.
   - E2E, new spec `tests/e2e/specs/journeys/wishlist-remove.spec.ts` (spec id `WR` for `specIsbn` / `VCODE_BLOCKS` if used; titles unique per run, free-form entries): from a blank browser `loginAs(page, "librarian")`, create three entries; remove one from the list (table at desktop width), check it is gone and a sibling stays (Rule #23); remove one from its entry page; refusal case: open the Remove modal on page A, remove the same entry on page B (same context), Confirm on A → `[data-modal-error]` shows `already_removed` (EN/FR regex), modal still open (Rule #3); "Bought" still opens the bought modal; axe on `/wishlist` (`helpers/accessibility.ts`). No `waitForTimeout`; `expectHtmxProcessed` before clicking inside a swapped modal.
10. **AC10 — gates.** `cargo clippy -- -D warnings` (and `--all-targets`), unit + integration tests (`wishlist_remove::`, `locale_parity::`, the templates audit), `npx tsc --noEmit`, the `waitForTimeout` grep, `vcode-literals.sh`, the new spec and `wishlist.spec.ts` green under `CI=true npx playwright test --workers=2`.

## Tasks / Subtasks

- [ ] T1 — `WishlistAction { Bought, Remove }` in `src/routes/wishlist.rs` with `from_query(Option<&str>)`, `audit_action()` (`"wishlist_bought"` / `"wishlist_removed"`), `query_value()`; a `#[derive(Deserialize)] struct WishlistActionQuery { action: Option<String> }` (no `deny_unknown_fields`) (D5, AC8).
- [ ] T2 — `wishlist_delete_modal`: take `Query<WishlistActionQuery>`; pick title / body / confirm label per action; `action_url = format!("/wishlist/{id}?action={}", action.query_value())`; D7 (if approved): `None` → `NotFound(wishlist.already_removed)` (AC2, AC5).
- [ ] T3 — `wishlist_delete`: take the same query; `None` or `0` rows → `AppError::NotFound(wishlist.already_removed)` (404 → body into `[data-modal-error]`), audit with `action.audit_action()`; the redirect unchanged (AC3, AC5).
- [ ] T4 — Templates: list row + card + entry page get the "Remove" button and both `aria-label`s (D6). The labels depend on the title: build them in Rust — e.g. a `WishlistRow { item, bought_label, remove_label }` view for the list (`items: Vec<WishlistRow>`), two `String` fields on `WishlistDetailTemplate` — never `t!()` in the template. Update the module doc's surface map and the modal fragment's header comment (no "Mark as bought" only) (AC1).
- [ ] T5 — Locales ×4, `wishlist:` block only (D4, D8 if approved); `touch src/lib.rs && cargo build`; `cargo test --test integration locale_parity::` (AC4).
- [ ] T6 — Unit tests, `tests/wishlist_remove.rs` (+ `mod wishlist_remove;` in `tests/integration.rs`, alphabetical), E2E `wishlist-remove.spec.ts` (AC9).
- [ ] T7 — Gates (AC10); commit per workflow step (Rule #10).

## Dev Notes

### Guardrails

- **Do not touch** `services::soft_delete::ALLOWED_TABLES` / `PURGE_DELETION_ORDER` (D2: no Trash for wish list items in this story).
- **No `hx-confirm=`** (templates audit, allowlist empty); the modal is the UX-DR8 macro, Pattern A in `#modal-slot`. Do not add a new modal fragment: one `wishlist_delete_modal.html` takes both actions (its fields already are per-call strings).
- **CSP / templates audit:** no inline `style=` / `on…=`; the `aria-label` values come from Rust strings built with `t!()` in the request locale and are escaped by Askama (`{{ … }}`, never `|safe`). The scan banner (`autolink_feedback_html`, Rust-built markup) is unchanged — do not add a literal `aria-label` there (`templates_audit` reads Rust markup since 19-1).
- **Escaping:** the entry title goes into the modal title (an Askama field, escaped) and into `t!()` parameters; `body_html` stays `<p>{html_escape(text)}</p>`.
- **Rule #20:** the action parser accepts two exact strings; everything else is the existing behaviour ("bought").
- **Rule #21:** the buttons open a modal through `#modal-slot`; `modal.js` restores focus on Cancel — do not send focus anywhere by hand. After Confirm the page reloads (D3).
- **AppError:** propagate with `?`; the 404 body is a feedback entry by `AppError::IntoResponse`; inside the modal `ModalConfirmRetargetGuard` strips the retarget so `modal.js` shows it in `[data-modal-error]`. Do not wrap it by hand.
- **Audit stays best effort** (`.ok()`), after the soft-delete, as today; no audit row on any refusal.
- **E2E data:** free-form titles unique per run (`WR-…-${Date.now()}`-style), no ISBN shared with 20.2's dedup, no title + note pair 20.2 could match; assert only on the spec's own rows; the desktop table and the mobile card are both in the DOM (`hidden md:block` / `md:hidden`) — scope locators to the visible one (or set the viewport) so a click never lands on the hidden copy.
- Heavy commands under `flock -w 10800 /tmp/station-heavy.lock mem-guard …`; never run `cargo fmt`.

### Couplings (Rule #24)

- **20.2** adds `WishlistItemModel::create_in` in `src/models/wishlist.rs` and reuses `wishlist::audit_wishlist_add`; 20.1 does not touch `models/wishlist.rs` beyond reading it (no model change needed), nor `audit_wishlist_add`, nor `src/routes/series_expected_tomes.rs`. Same file `src/routes/wishlist.rs` for 20.2 only if it moves the shared dedup there — then different functions; whoever merges second rebases.
- **Locales:** this story adds keys only under `wishlist:` (20.2 adds under `expected_tome:`). Rebase → re-run `locale_parity::`.
- **20.6 smoke** will remove its gap entry through "Remove": it relies on the accessible name `action_remove_named` and on `?action=remove`; keep both stable.
- **20.7 documentation** describes "Remove" versus "Bought" in `03-usage.tex` — no manual change here.
- **`sprint-status.yaml`:** this story's line + `last_updated` only (Rule #16).

### References

- #714; `_bmad-output/planning-artifacts/epics.md` "Epic 20" (decisions D1–D3, D11, copy table) and "Story 20.1"; FR183.
- `src/routes/wishlist.rs` (`wishlist_page`, `wishlist_detail`, `wishlist_delete_modal`, `wishlist_delete`, `autolink_feedback_html`), `src/models/wishlist.rs`, `templates/pages/wishlist.html`, `templates/pages/wishlist_detail.html`, `templates/fragments/wishlist_delete_modal.html`, `templates/components/modal.html`, `static/js/modal.js`, `src/middleware/modal_confirm_retarget_guard.rs`, `src/models/admin_audit.rs::AdminAuditModel::create`, `tests/borrower_delete_modal.rs` (modal route test shape), `tests/e2e/specs/journeys/wishlist.spec.ts`, `tests/e2e/specs/journeys/admin-modal-lifecycle.spec.ts` (`[data-modal-error]` assertions), `docs/error-message-style.md`.

## Dev Agent Record

### Agent Model Used

### Completion Notes List

### File List
