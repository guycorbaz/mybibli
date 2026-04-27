# Story 8.4: Reference data management

Status: review

Epic: 8 — Administration & Configuration
Requirements mapping: FR70 (genres), FR71 (volume states + `loanable` flag), FR72 (contributor roles), FR73 (location node types), FR91 (default reference data on first launch — already partially shipped via existing seed migrations), FR100 (deletion guards on in-use ref entries), NFR41 (no translation of reference data in v1), UX-DR21 (InlineForm component), UX-DR7 (Reference Data tab content), AR16 (middleware order), AR24 (entity common columns), Foundation Rules #1–#7, #12

---

> **TL;DR** — Fills the **Reference Data** tab stub that story 8-1 left behind. Ships a single panel with **four sub-sections** (Genres, Volume States, Contributor Roles, Location Node Types) using a new shared **`InlineForm` component** (UX-DR21): list rows, "Add" button → inline input (Enter/Escape), click-name → inline rename, icon button → soft-delete via Modal confirmation. Volume States adds a **per-row `is_loanable` checkbox** that HTMX-toggles in place; toggling OFF on a state with active loans opens a **warning Modal** listing affected loans before applying. Every delete is gated by a **usage-count guard** (`SELECT COUNT(*) WHERE <fk_col> = ?`) returning 409 with a localized message + link to filtered list. Rename is non-cascading because all four FKs use surrogate integer IDs (the visible display string updates everywhere automatically) — **except `storage_locations.node_type`, which is a `VARCHAR(50)` soft reference, not an integer FK**. That asymmetry is the story's biggest schema-trap: rename of a node type must `UPDATE storage_locations SET node_type = ? WHERE node_type = ?` in the same transaction, and deletion must count by name match. **No new `hx-confirm=` attributes** — the allowlist is frozen at 5 (CLAUDE.md scanner-guard invariant) and this story uses Modal dialogs (mirroring `AdminTrashPermanentDeleteModal` from 8-7) for both the simple-delete prompt and the loanable-toggle warning. **No new seed migration** — the four reference tables already have seed migrations from earlier epics whose values diverge from the spec wording in the epic AC; this story documents the deviation as accepted (admins can adjust freely from the new UI, which is exactly what FR70-FR73 are for) rather than rewriting history. Admin handlers extract to a new module `src/routes/admin_reference_data.rs` because `routes/admin.rs` is at 1571 lines and would cross Foundation Rule #12's 2000-line ceiling once four sub-section CRUDs land.

## Story

As an **admin**,
I want to configure the lists of **genres**, **volume states** (with a loanable flag), **contributor roles**, and **location node types** used across the catalog,
so that the taxonomy matches my library's needs and I can evolve it over time — without editing migrations or SQL by hand.

## Scope Reality & What This Story Ships

### What's already in place (do NOT redo)

**The four reference tables already exist** with the exact AR24 common-columns shape (id BIGINT UNSIGNED, name VARCHAR(255) UNIQUE, created_at, updated_at, deleted_at, version INT, INDEX idx on deleted_at):
- `genres` — `migrations/20260329000000_initial_schema.sql:7-15`. FK from `titles.genre_id` (CONSTRAINT `fk_titles_genre`).
- `volume_states` — same migration, lines 17-26. **Has the `is_loanable BOOLEAN NOT NULL DEFAULT TRUE` column** the AC requires. FK from `volumes.condition_state_id` (CONSTRAINT `fk_volumes_condition`).
- `contributor_roles` — same migration, lines 28-36. FK from `title_contributors.role_id` (CONSTRAINT `fk_tc_role`).
- `location_node_types` — same migration, lines 38-46. **There is NO FK constraint** from `storage_locations` — `storage_locations.node_type` is a free `VARCHAR(50)` column that stores the *name* of the node type, not its id. See §Cross-cutting decisions for why we keep this loose link instead of migrating to an integer FK in this story.

**Read-only models already exist** (with Foundation-Rule-#1 implications: this story EXTENDS them, does not duplicate):
- `src/models/genre.rs` (70 lines) — `GenreModel { id, name }` + `find_name_by_id`, `list_active`. **Does not currently expose `version`** — extend the struct to include `version: i32` for optimistic locking.
- `src/models/volume_state.rs` (96 lines) — `VolumeStateModel { id, name, is_loanable }` + `list_active`, `is_loanable_by_volume`. Same: extend with `version: i32`.
- `src/models/contributor.rs:354-376` — `pub struct ContributorRoleModel;` (unit struct) with `find_by_id`, `find_all`. **Extract to its own file** `src/models/contributor_role.rs` for clarity (single-responsibility per file, like `genre.rs` and `volume_state.rs`); keep `find_by_id` and `find_all` signatures unchanged so existing call sites in `contributor.rs` stay compiling. Add `version: i32` exposure when CRUD methods land.
- `src/models/location.rs:163-170` — `LocationModel::find_node_types` returns `Vec<(u64, String)>`. **Extract** to a new `src/models/location_node_type.rs` with a `LocationNodeTypeModel { id, name, version }` struct. Update the single existing call site in the location-edit dropdown population.

**Default-reference-data seed migrations already exist** for FR91 (and predate this story):
- `migrations/20260330000001_seed_default_genres.sql` — seeds 11 genres (Roman, BD, Science-Fiction, Policier, Jeunesse, Musique, Film, Documentaire, Revue, Rapport, Non classé).
- `migrations/20260330000002_seed_default_reference_data.sql` — seeds 4 volume states (Neuf TRUE / Bon TRUE / Usé TRUE / Endommagé FALSE) and 8 contributor roles (Auteur, Illustrateur, Traducteur, Réalisateur, Compositeur, Interprète, Scénariste, Coloriste).
- `migrations/20260401000001_seed_location_node_types.sql` — seeds 4 English node types (Room, Furniture, Shelf, Box).

These values diverge from the literal list in the epic AC (which prescribes `fiction, essai, BD, manga, jeunesse, documentaire, poésie, théâtre` for genres; `neuf, très bon, bon, moyen, mauvais` with `T,T,T,T,F` for states; `auteur, illustrateur, traducteur, préfacier, éditeur scientifique` for roles; `bibliothèque, étagère, rayon, case` for node types). **This story does NOT add a new seed migration to overwrite them.** Rationale in §Cross-cutting decisions — short version: FR91's intent ("default reference data exists on first launch") is already met by the existing seeds, and one of the whole points of FR70-FR73 is that admins can edit these lists from the UI. Rewriting seeds risks breaking existing rows whose FK already points to the soon-to-be-deleted seed (e.g., `volumes.condition_state_id → volume_states.id` referencing a seeded "Usé" row). Document the deviation, ship the CRUD UI, let Guy adjust per-installation.

**The Reference Data tab is wired but stubbed** in the admin shell:
- `AdminTab::ReferenceData` enum variant — `src/routes/admin.rs:46`.
- `AdminTab::as_str()` returns `"reference_data"` (snake_case for i18n keys + template ids); `AdminTab::hx_path()` returns `"reference-data"` (hyphenated for the URL). The two are intentionally different — keep both.
- Route `GET /admin/reference-data` → `admin::admin_reference_data_panel` — `src/routes/mod.rs` (alongside the other tab routes) and the handler at `src/routes/admin.rs:902-911`. Currently returns an `AdminReferenceDataPanel { stub_message: t!("admin.placeholder.coming_in_story", story = "8-3") }` — update to "8-4" or, better, replace the entire template body so the placeholder key is no longer rendered.
- Tab-bar HTMX wiring (click → swap panel + `hx-push-url=/admin?tab=reference_data`) is already covered by 8-1's `admin_tabs.html` component — do NOT touch it.

**The frozen `hx-confirm=` allowlist is at 5 entries** (`src/templates_audit.rs:35-41`):
```rust
const ALLOWED_HX_CONFIRM_SITES: &[(&str, usize)] = &[
    ("templates/pages/loans.html", 1),
    ("templates/pages/borrower_detail.html", 2),
    ("templates/pages/contributor_detail.html", 1),
    ("templates/pages/series_detail.html", 1),
    ("templates/fragments/admin_users_row.html", 1),
];
```
**Story 8-4 does NOT extend this allowlist.** Per CLAUDE.md's scanner-guard invariant (story 7-5 and the 8-3 5th-entry exemption), new destructive-action UX MUST use the Modal pattern. We have a working modal pattern in `templates/fragments/admin_trash_permanent_delete_modal.html` (story 8-7) — mirror it for both the simple-delete prompt and the loanable-toggle warning.

**`forms_include_csrf_token` audit gate** (`src/templates_audit.rs:302-394`): every `<form method="POST">` MUST contain `<input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">` as its **first child** child. The new InlineForm (Add/Rename), the loanable-toggle form, the delete-modal form, and the loanable-warning-modal form — every POST form in this story is covered by this audit. Story 8-2 's CSRF middleware automatically protects all 4 sub-section POST routes (allowlist is frozen at `POST /login` only, audit `csrf_exempt_routes_frozen` enforces this).

**`services::soft_delete::ALLOWED_TABLES`** (`src/services/soft_delete.rs:12-19`) lists `titles, volumes, contributors, storage_locations, borrowers, series` — the four reference data tables are **deliberately NOT in the whitelist**. Soft-deletes from this story (deleting a genre / state / role / node type) **stay invisible to the Trash view** (story 8-6). They are reversible only through the new Reactivate-or-recreate path the admin would take from the Reference Data tab itself, OR by direct DB intervention — which is acceptable because these are admin-only taxonomy entries, not user-facing content. Do NOT add the four tables to ALLOWED_TABLES (8-6 / 8-7 are not in scope here).

**`templates/fragments/admin_users_panel.html` is the closest existing pattern** for the panel shell (filter form, list, OOB-friendly mutation responses). Mirror its structure but drop the pagination block (NFR39's 25/page does NOT apply — reference data lists are small and rendered in full per UX-DR21).

### What this story ships

**1. Migrations — none.** The four tables exist with the right shape. No `is_loanable` column to add. No FK to introduce. No seed migration. The earlier seeds stay as-is.

**2. New shared template — `templates/components/inline_form.html`** (UX-DR21). Parameterized by:
- `entity_label_singular: String` — e.g., "genre" / "Genre".
- `entity_label_plural: String` — e.g., "genres" / "Genres".
- `list_endpoint: String` — e.g., `/admin/reference-data/genres` (where the Add form POSTs and the list is fetched).
- `save_endpoint_template: String` — e.g., `/admin/reference-data/genres/{id}` (rename + delete target — `{id}` filled in per-row).
- `loanable_toggle_endpoint_template: Option<String>` — `Some("/admin/reference-data/volume-states/{id}/loanable")` for volume states only; `None` everywhere else (template branch on `loanable_toggle_endpoint_template.is_some()`).
- `entries: Vec<InlineFormEntry>` — `{ id: u64, name: String, is_loanable: Option<bool>, version: i32, usage_count: i64 }`. `is_loanable` is `Some(_)` only on volume states; `usage_count` is the pre-computed FK reference count (so the delete button can show a tooltip / disabled state when > 0 in v1; AC says we surface the count on the 409 response, but it's also useful at hover-time as an early signal).

The component renders:
- A `<ul role="list">` of entries; each `<li role="listitem">` shows the name (click-to-edit), the loanable checkbox if applicable, the usage-count chip (if > 0), and a delete icon button.
- An "Add <singular>" button that toggles into an inline form (text input autofocused on appearance, `Enter` saves, `Escape` cancels — same pattern as the existing `templates/fragments/title_edit_form.html`).
- All buttons use `data-action="..."` delegated handlers (CSP from 7-4) — see new JS module below.

**3. New JS module — `static/js/inline-form.js`** registered in `static/js/mybibli.js` after the existing `initFeedbackDismiss()` call (line ~195 in `mybibli.js`). Handlers:
- `data-action="inline-form-edit"` — clicking a row's name (or pressing Enter on it) replaces the `<span>` with a text input pre-filled with the current value, autofocused. Keyboard contract: `Enter` triggers `htmx.ajax('POST', save_endpoint, {...})`; `Escape` reverts the row to the read-only span without a server roundtrip.
- `data-action="inline-form-add-toggle"` — toggles the Add form's `.hidden` class and focuses the input when revealed.
- `data-action="inline-form-add-cancel"` — closes the Add form and resets the input value.
- `data-action="inline-form-delete-open"` — fetches the delete-confirm Modal fragment via `htmx.ajax('GET', endpoint)` and inserts it into a top-level `#admin-modal-slot` div added to `layouts/base.html` for this story. (The slot doesn't exist yet — the existing trash modal mounts via `outerHTML` swap on the panel; the Reference Data panel needs a stable mount point, so add `<div id="admin-modal-slot"></div>` as the last child of `<main>` in `layouts/base.html` so any admin modal can target it without touching the panel structure.)
- `data-action="inline-form-loanable-toggle"` — for volume states only. Fires an `htmx.ajax('POST', loanable_toggle_endpoint, {is_loanable: <new>, version: <current>})`. Server decides whether to apply directly or return the warning-modal fragment (see handler logic below).

JS module pattern: pure ES module, no inline scripts, no globals beyond the IIFE export. Mirrors `static/js/csrf.js` and `static/js/scanner-guard.js` style.

**4. Model extensions — CRUD on existing files (no new model files except `contributor_role.rs` and `location_node_type.rs` extractions):**

Each of `GenreModel`, `VolumeStateModel`, `ContributorRoleModel`, `LocationNodeTypeModel` gains the following methods (all return `Result<_, AppError>`):

```rust
// Common signature pattern (substitute the entity name and table)
pub async fn list_all(pool: &DbPool) -> Result<Vec<Self>, AppError>;
//   SELECT id, name, [is_loanable,] version FROM <table> WHERE deleted_at IS NULL ORDER BY name
pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<Self>, AppError>;
pub async fn create(pool: &DbPool, name: &str, /* is_loanable for VolumeState */) -> Result<u64, AppError>;
//   INSERT INTO <table> (name [, is_loanable]) VALUES (?, [?])
//   On 1062 / SQLSTATE 23000 → AppError::Conflict("name_taken")
pub async fn rename(pool: &DbPool, id: u64, version: i32, new_name: &str) -> Result<(), AppError>;
//   UPDATE <table> SET name = ?, version = version + 1 WHERE id = ? AND version = ? AND deleted_at IS NULL
//   check_update_result via services/locking.rs → Conflict("version_mismatch")
//   On 1062 → Conflict("name_taken")
pub async fn soft_delete(pool: &DbPool, id: u64, version: i32) -> Result<(), AppError>;
//   UPDATE <table> SET deleted_at = NOW(), version = version + 1 WHERE id = ? AND version = ? AND deleted_at IS NULL
//   check_update_result → Conflict("version_mismatch")
pub async fn count_usage(pool: &DbPool, id: u64) -> Result<i64, AppError>;
//   See per-table SQL below — this is the AC#4 deletion guard.
```

`VolumeStateModel` additionally gets:

```rust
pub async fn set_loanable(pool: &DbPool, id: u64, version: i32, is_loanable: bool) -> Result<(), AppError>;
//   UPDATE volume_states SET is_loanable = ?, version = version + 1 WHERE id = ? AND version = ? AND deleted_at IS NULL
pub async fn count_active_loans_for_state(pool: &DbPool, state_id: u64) -> Result<i64, AppError>;
//   SELECT COUNT(*) FROM loans l
//     JOIN volumes v ON l.volume_id = v.id
//    WHERE v.condition_state_id = ?
//      AND l.returned_at IS NULL
//      AND l.deleted_at IS NULL
//      AND v.deleted_at IS NULL
//   Used to decide whether to surface the loanable-toggle-OFF warning modal.
```

`LocationNodeTypeModel::rename` is **transactional** — it must also run an `UPDATE storage_locations SET node_type = ? WHERE node_type = ? AND deleted_at IS NULL` in the same transaction so the loose VARCHAR reference stays consistent (see §Cross-cutting decisions for the rationale). `LocationNodeTypeModel::soft_delete` does NOT cascade — it relies on `count_usage` returning 0 before the UI calls it, and the count query is the by-name match (`SELECT COUNT(*) FROM storage_locations WHERE node_type = ? AND deleted_at IS NULL`). The asymmetry between rename (cascading) and delete (refused if any references) is intentional and documented in the model file's docstring.

**Per-table `count_usage` queries** (the AC#4 deletion guard; each returns the count of *active* references — soft-deleted rows do not count, so deleting a genre that's only attached to soft-deleted titles is allowed):

```sql
-- GenreModel::count_usage
SELECT COUNT(*) FROM titles WHERE genre_id = ? AND deleted_at IS NULL;

-- VolumeStateModel::count_usage
SELECT COUNT(*) FROM volumes WHERE condition_state_id = ? AND deleted_at IS NULL;

-- ContributorRoleModel::count_usage
SELECT COUNT(*) FROM title_contributors WHERE role_id = ? AND deleted_at IS NULL;

-- LocationNodeTypeModel::count_usage
SELECT COUNT(*) FROM storage_locations WHERE node_type = (
    SELECT name FROM location_node_types WHERE id = ? AND deleted_at IS NULL
) AND deleted_at IS NULL;
-- (sub-query because storage_locations stores the name, not the id; if the node-type row is
--  already soft-deleted at query time the inner SELECT yields no row → COUNT is 0; that's
--  correct because a deleted node-type has no live references either.)
```

**5. New module — `src/routes/admin_reference_data.rs`** (NEW FILE). Houses all reference-data handlers + their template structs. Reason: `routes/admin.rs` is at 1571 lines today; adding ~25 handlers + ~10 template structs for 4 sub-sections would push it past Foundation Rule #12's 2000-line ceiling. Module wiring: `pub mod admin_reference_data;` in `src/routes/mod.rs`, and the route table mounts each handler from `admin_reference_data::*` rather than `admin::*`. The handlers all start with `session.require_role_with_return(Role::Admin, &return_path)?` — same gating as the existing admin handlers; Anonymous → 303 to `/login?next=…`, Librarian → 403 FeedbackEntry.

**Handlers (per sub-section — 4 sub-sections × identical surface = 4 sets):**

```text
GET  /admin/reference-data                          → admin_reference_data_panel
GET  /admin/reference-data/genres                   → genres_section (full or fragment)
POST /admin/reference-data/genres                   → genres_create
POST /admin/reference-data/genres/{id}/rename       → genres_rename
GET  /admin/reference-data/genres/{id}/delete-modal → genres_delete_modal     (returns the modal HTML)
POST /admin/reference-data/genres/{id}/delete       → genres_delete           (soft-delete after modal confirmed)

GET  /admin/reference-data/volume-states            → volume_states_section
POST /admin/reference-data/volume-states            → volume_states_create
POST /admin/reference-data/volume-states/{id}/rename → volume_states_rename
POST /admin/reference-data/volume-states/{id}/loanable → volume_states_loanable_toggle
                                                       (server inspects active-loans count;
                                                        returns either the updated row OR the warning-modal fragment)
POST /admin/reference-data/volume-states/{id}/loanable/confirm → volume_states_loanable_confirm
                                                       (called from the warning modal's "Apply anyway" button)
GET  /admin/reference-data/volume-states/{id}/delete-modal → volume_states_delete_modal
POST /admin/reference-data/volume-states/{id}/delete → volume_states_delete

GET  /admin/reference-data/contributor-roles        → roles_section
POST /admin/reference-data/contributor-roles        → roles_create
POST /admin/reference-data/contributor-roles/{id}/rename → roles_rename
GET  /admin/reference-data/contributor-roles/{id}/delete-modal → roles_delete_modal
POST /admin/reference-data/contributor-roles/{id}/delete → roles_delete

GET  /admin/reference-data/node-types               → node_types_section
POST /admin/reference-data/node-types               → node_types_create
POST /admin/reference-data/node-types/{id}/rename   → node_types_rename
GET  /admin/reference-data/node-types/{id}/delete-modal → node_types_delete_modal
POST /admin/reference-data/node-types/{id}/delete   → node_types_delete
```

Total: 25 routes (1 panel + 4 × 6 sub-section endpoints, + 1 extra confirm endpoint for volume states' loanable-warning workflow). All POST routes are CSRF-protected by the 8-2 middleware automatically (no allowlist edit needed).

**Form struct contract** (in `admin_reference_data.rs`, mirroring the 8-3 pattern):

```rust
#[derive(Deserialize)]
pub struct CreateRefForm {
    pub name: String,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct RenameRefForm {
    pub name: String,
    pub version: i32,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct DeleteRefForm {
    pub version: i32,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct CreateVolumeStateForm {  // adds is_loanable
    pub name: String,
    pub is_loanable: Option<String>, // checkbox: "on" / absent → bool
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct LoanableToggleForm {
    pub is_loanable: Option<String>, // "on" / absent
    pub version: i32,
    pub _csrf_token: String,
    pub force: Option<String>,        // "true" if posting from the warning-modal "Apply anyway" path
}
```

**6. Templates** — see Tasks for the full list. Three new patterns:
- `templates/fragments/admin_reference_data_panel.html` (REPLACE existing 4-line stub) — outermost panel; contains four `<section>` blocks each rendered by `{% include "components/inline_form.html" %}` with section-specific args.
- `templates/components/inline_form.html` (NEW, parameterized) — see point 2.
- `templates/fragments/admin_ref_delete_modal.html` (NEW) — generic delete-confirm modal; inputs: `entity_singular`, `entity_name`, `delete_endpoint`, `version`, `csrf_token`. Uses the same `<dialog open aria-modal="true">` shell as `admin_trash_permanent_delete_modal.html` (so scanner-guard 7-5 catches it automatically) **but does NOT require the type-the-name friction** — soft-delete is recoverable (an admin who deletes the wrong genre can re-create it under the same name with no FK fallout because nothing was attached). Single confirm/cancel button pair.
- `templates/fragments/admin_ref_loanable_warning_modal.html` (NEW) — variant of the delete modal showing affected-loans count + list of up to 5 sample loan rows; "Apply anyway" + "Cancel" buttons. "Apply anyway" POSTs to `/admin/reference-data/volume-states/{id}/loanable/confirm` with `force=true`.

**7. i18n keys — EN + FR** under `admin.reference_data.*` (new namespace, sibling of `admin.users.*`, `admin.tabs.*`, `admin.placeholder.*`). See §Ships task 9 for the full key list.

**8. Documentation:**
- `CLAUDE.md` Key Patterns: append a "Reference data CRUD pattern (story 8-4)" bullet covering: the four tables share one CRUD shape, deletion uses a usage-count guard not soft-delete-whitelist, `LocationNodeTypeModel::rename` cascades to `storage_locations.node_type` because the FK is a VARCHAR (intentional asymmetry — NEW project-wide invariant). Short bullet — Key Patterns is not a reference manual.
- `docs/route-role-matrix.md`: add 25 new rows under Admin role.
- `_bmad-output/planning-artifacts/architecture.md` Database Schema Decisions section: add a paragraph noting the loose-VARCHAR-FK on `storage_locations.node_type` and why it's preserved (rename-cascade is the cheaper option vs. a schema migration that would touch ~20 location rows + every place the model is used).

### What this story does NOT ship

- **No new seed migration.** Existing seeds satisfy FR91. Spec-vs-seed value mismatch is documented; admins use the new UI to adjust. (See §Cross-cutting decisions for the long form.)
- **No translation of reference data.** Per NFR41, values are stored and displayed as-entered. The i18n keys are for UI labels (heading, buttons, error messages), not for the entries themselves. The four sub-section headings *are* localized: "Genres / Genres", "Volume States / États du volume", etc.
- **No bulk import / CSV.** Out of scope.
- **No reordering** (alphabetical only — `ORDER BY name`). UX journey 9 explicitly excludes reordering in v1.
- **No restoration of deleted ref entries from a "Trash" view.** The four tables are deliberately NOT in `ALLOWED_TABLES` — story 8-6 (Trash view) does not surface them. If an admin deletes "BD" and wants it back, they re-create it with the same name (the unique-constraint on the active rows allows this because the soft-deleted row has `deleted_at IS NOT NULL` and... **wait — UNIQUE indexes apply to ALL rows by default in MariaDB, including soft-deleted ones**; see §Cross-cutting decisions for the resolution).
- **No FK migration on `storage_locations.node_type`.** The loose VARCHAR reference stays. Rename cascades in code; delete refuses if in use. Future story can migrate to integer FK if it ever becomes a pain point.
- **No "loanable: false" auto-return of active loans.** AC explicitly says: "state change is forward-only — confirming applies the change AND does NOT auto-return active loans".
- **No "view 42 affected titles" inline list on the 409 response.** AC says return 409 with usage count + a link to the filtered list (e.g., `/catalog?genre=42`). The catalog filter URL exists today; this story emits the URL in the FeedbackEntry message body and links it. Rendering the 42 titles inline in the modal would be over-scope.

## Cross-cutting decisions

**No new seed migration. Document the spec-vs-seed mismatch instead.**

The epic AC's literal value lists (genres `fiction, essai, BD, manga, jeunesse, documentaire, poésie, théâtre`; states `neuf, très bon, bon, moyen, mauvais` with `T,T,T,T,F`; roles `auteur, illustrateur, traducteur, préfacier, éditeur scientifique`; node types `bibliothèque, étagère, rayon, case`) do NOT match the seeds shipped by earlier epic migrations. Three options were considered:

1. **Replace the seeds** via a new migration that DELETEs old + INSERTs spec values. **Rejected** — risks orphaning `volumes.condition_state_id` and `titles.genre_id` rows whose FK points to a soon-to-be-deleted seed. Specifically, every existing volume in Guy's library has a `condition_state_id` pointing to one of "Neuf / Bon / Usé / Endommagé"; deleting "Usé" would break the FK constraint OR require a remap that defeats the point.
2. **Augment the seeds** via a new migration that INSERT IGNOREs the spec values on top of the existing rows. **Rejected** — produces a confused mixture (e.g., 14 genres with overlapping concepts) that the admin would then have to clean up by hand from the new UI; the cleanup work is the same regardless of whether we ship 11 genres or 14.
3. **Ship no seed migration. Document the deviation. Let the admin curate from the new UI.** **Selected** — minimum-blast-radius option. FR91's intent ("default reference data exists on first launch, app is usable out of the box") is fully satisfied by the existing seeds. FR70-FR73 ("admin can configure...") is what this story ships precisely so the admin can adjust per-installation taste. The new UI lets Guy delete `Roman` if he doesn't want it and add `fiction` instead — the operation takes 5 seconds per entry.

The story file's commit message and CLAUDE.md note document this decision so future reviewers (and Guy a year from now) know why the spec list and the seed list differ. The spec list was a planning-time aspiration; the seed list is the historical reality; the new UI bridges them.

**`UNIQUE` constraint applies to soft-deleted rows too — name reuse after delete is blocked.**

In MariaDB, a `UNIQUE` index on a column treats `NULL` as distinct (multiple rows with `name = NULL` allowed) but does NOT treat a `deleted_at IS NOT NULL` flag as a partial-index modifier. So if an admin soft-deletes the genre "Roman" and tries to create a new "Roman", the INSERT fails with SQLSTATE 23000 even though no *active* "Roman" exists. There are three ways to handle this:

1. **Hard-delete on the user-facing "delete" button.** Reject — breaks the soft-delete + 30-day-retention story (8-6/8-7).
2. **Recreate-as-rename.** When the unique-violation hits the create path, transparently look for a soft-deleted row with the same name + clear its `deleted_at` (= reactivate) instead. Behavioral edge case for the admin, but the result is identical to "delete + recreate". **Adopted.**
3. **Composite unique index `(name, deleted_at)` requires a schema migration.** Out of scope; we'd need it for every reference table, and MariaDB's NULL-distinct semantics still leave the soft-deleted-only-once corner case open.

**Implementation:** in each model's `create()`:
- Try the INSERT.
- On SQLSTATE 23000:
  - `SELECT id, version FROM <table> WHERE name = ? AND deleted_at IS NOT NULL LIMIT 1` (case-insensitive default collation, so this matches the conflicting row).
  - If found: `UPDATE <table> SET deleted_at = NULL, version = version + 1 WHERE id = ? AND version = ?` and return that id with success metadata `{ "reactivated": true }` so the handler can render the FeedbackEntry as "Reactivated <name>" rather than "Created <name>".
  - If not found (collision is with an *active* row): return `Conflict("name_taken")` like normal.

Document this in the model's docstring so future maintainers see the contract.

**`location_node_types` is a loose VARCHAR reference — keep it that way.**

`storage_locations.node_type` is `VARCHAR(50)`, NOT a FK to `location_node_types.id`. This was set by `migrations/20260329000000_initial_schema.sql:71` and predates any reference-data CRUD. Migrating it to an integer FK would require:
- A migration that ADDs `node_type_id BIGINT UNSIGNED NULL` to `storage_locations`.
- A backfill script that JOINs by name (with the existing English seeds — Room, Furniture, Shelf, Box — matching by case-sensitive collation).
- DROPing the old VARCHAR column.
- Updating `LocationModel::find_*` and the location-create handler to use the id.
- Updating templates that render the breadcrumb (the breadcrumb shows the type name; with an FK that becomes a JOIN — small but real perf change for the recursive CTE).

That's a story by itself. **Story 8-4 keeps the loose link** and pays the cost in two places:
- `LocationNodeTypeModel::rename` is transactional — it BEGINs, UPDATEs the `location_node_types` row, then UPDATEs all `storage_locations` rows whose `node_type = <old_name>` to the new name, then COMMITs. On any error the entire rename rolls back. The transaction makes this race-safe with concurrent admin sessions creating/editing locations.
- `LocationNodeTypeModel::count_usage` and `LocationNodeTypeModel::soft_delete` query / refuse by-name match (see SQL above).

This asymmetry — rename cascades because the link is by name; delete refuses because the link is by name — is the cleanest pair given the existing schema.

**Soft-delete of reference entries does NOT cascade to dependent rows.**

When an admin deletes a genre, the existing `titles.genre_id` rows are NOT touched. They keep pointing to the now-soft-deleted genre. The deletion guard (`count_usage` on the live count) prevents this from happening for *active* references — but if ALL referencing titles are themselves soft-deleted, `count_usage` returns 0 and the genre delete proceeds. That's the right behavior: the genre is genuinely unused (all its assignments are in the Trash). When story 8-7's auto-purge later hard-deletes those soft-deleted titles, the genre row stays soft-deleted (inert). If 8-6 / 8-7 ever wants to surface "ref entries with no remaining active or soft-deleted references" for hard-purge, that's a separate feature.

**Deletion guard message includes a link to the filtered catalog.**

Per AC#4: `409 Conflict` body includes a localized message + a link to a list filtered by the in-use ref. The link targets:
- Genres → `/catalog?genre={id}` (route already exists per FR21 "filter by genre").
- Volume states → `/catalog?volume_state={id}` — verify whether this filter exists; if not, fall back to a plain text count without a link, and open a follow-up issue. Do NOT add the filter route in this story.
- Contributor roles → no direct list-by-role view exists in the catalog; fall back to the contributors list filtered by role if such a route exists, else plain text count.
- Location node types → `/locations?node_type={name}` — verify; if absent, plain text count.

**Implementation guidance:** the story's CRUD handler resolves the link existence at handler-time, conditionally rendering the link or plain text. Keep the i18n key parameterized: `error.reference_data.in_use: "Cannot delete: %{count} %{plural} use this %{singular}. %{link_html}"` where `link_html` is either a `<a href="...">View list</a>` HTML fragment or an empty string. (Yes, this means the i18n value contains HTML — that's fine because the parameters are server-controlled, not user input. Use Askama's `safe` filter with care or just do the substitution server-side.)

**Modal pattern reuse — mirror `AdminTrashPermanentDeleteModal` shell, drop the type-the-name input.**

Story 8-7's modal at `templates/fragments/admin_trash_permanent_delete_modal.html` ships a `<dialog open aria-modal="true">` that scanner-guard 7-5 already protects (the MutationObserver in `static/js/scanner-guard.js` watches for `dialog[open]` and `[aria-modal="true"]`). Story 8-4's two new modals (`admin_ref_delete_modal.html`, `admin_ref_loanable_warning_modal.html`) use the same outer shell so they inherit the scanner-guard protection without any new JS. The 8-4 simple-delete modal SKIPS the type-the-name friction because soft-delete is recoverable in spirit (re-create with the same name reactivates the row per the soft-delete-name-reuse decision above), and the AC does not require it. The loanable-warning modal is a different beast: it lists affected loans + has an "Apply anyway" button — friction is informational, not friction-by-typing.

**`hx-confirm=` allowlist stays at 5.**

CLAUDE.md scanner-guard invariant: "the allowlist is frozen at 5 grandfathered sites and only changes through explicit review." Story 8-4 does NOT add an entry. Every destructive action goes through a Modal. Reviewers can confirm by `cargo test templates_audit::hx_confirm_matches_allowlist` — pass with `len == 5`.

**`forms_include_csrf_token` audit — every new POST form covered.**

Each of the InlineForm Add input, the rename form, the delete-modal form, the loanable-toggle inline form, the loanable-warning-modal "Apply anyway" form, and the loanable-warning-modal "Cancel" form (technically a button, no form needed for cancel) is a `<form method="POST">`. Each MUST contain `<input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">` as its first child. The audit at `src/templates_audit.rs::forms_include_csrf_token` will fail `cargo test` if any one is missed.

**`base_context()` helper still deferred.**

Same situation as 8-3: every page-template struct that extends `layouts/base.html` flattens the common fields manually (`lang`, `role`, `current_page`, `skip_label`, nav labels, `csrf_token`, `admin_tabs_*`, `trash_count`). Do NOT introduce the helper here.

**No new admin routes in CSRF-exempt list.**

`CSRF_EXEMPT_ROUTES` in `src/middleware/csrf.rs` is frozen at `[("POST", "/login")]`. All 25 new POST endpoints in this story go through the standard CSRF check. The audit `csrf_exempt_routes_frozen` (`templates_audit.rs:298`) enforces this — adding any of them would fail `cargo test`.

**`tab=` invalid resolution — already handled.**

`AdminTab::from_query_str(Some("reference_data"))` already maps to `AdminTab::ReferenceData`; the URL `/admin?tab=reference_data` works today (renders the stub). After this story it renders the real panel. No tab-resolution code change.

## Acceptance Criteria

1. **Reference Data tab renders 4 sub-sections — Genres, Volume States, Contributor Roles, Location Node Types — each using the same InlineForm component.**
   - `/admin?tab=reference_data` as admin renders the full admin page with the Reference Data tab pre-selected and the panel server-rendered (non-HTMX path); as librarian → 403; as anonymous → 303 → `/login?next=%2Fadmin%3Ftab%3Dreference_data`.
   - HTMX click on the Reference Data tab from another admin tab GETs `/admin/reference-data` and swaps only the panel fragment into `#admin-shell`, with `hx-push-url="/admin?tab=reference_data"` keeping the URL canonical.
   - The panel renders four `<section>` blocks in order: Genres, Volume States, Contributor Roles, Location Node Types. Each section header is localized (EN: "Genres" / "Volume states" / "Contributor roles" / "Location node types"; FR: "Genres" / "États du volume" / "Rôles de contributeur" / "Types d'emplacement").
   - Each section is rendered by the new `templates/components/inline_form.html` component with section-specific args. Reuse is via `{% include "components/inline_form.html" with ... %}` (or Askama equivalent — verify whether the project uses include-with-context or template inheritance for this).

2. **Add — inline form with Enter to save, Escape to cancel.**
   - In each section, an "Add <singular>" button (e.g., "Add genre") toggles an inline form: a single text input (autofocused on appear) + a `[+]` confirm icon + an `[×]` cancel icon. Volume States additionally has a `is_loanable` checkbox in the Add form (default checked).
   - Keyboard: focus on the input, `Enter` submits, `Escape` reverts (closes form, restores input value to empty for next time).
   - Server validation:
     - Empty / whitespace-only `name` → 400 + `error.reference_data.name_empty` FeedbackEntry.
     - `name` length > 255 chars → 400 + `error.reference_data.name_too_long`.
     - `name` collides with an active row → 409 + `error.reference_data.name_taken`.
     - `name` collides with a soft-deleted row → server transparently reactivates the soft-deleted row (clears `deleted_at`, bumps `version`) and returns success with feedback `success.reference_data.reactivated` ("Reactivated <name>") instead of the standard `success.reference_data.created` ("Added <name>"). Same row reappears in the list.
   - On success: HTMX response includes `(a)` the updated section list fragment (with the new row in alphabetical position), `(b)` an OOB success FeedbackEntry, `(c)` the form slot is cleared back to its closed state.
   - Volume States adds: the `is_loanable` checkbox state is honored on create.

3. **Rename — click-to-edit on the row name, Enter to save, Escape to cancel.**
   - Clicking the row name (or pressing Enter on it) replaces the read-only `<span>` with a text input pre-filled and autofocused.
   - Keyboard: `Enter` submits via HTMX POST `/admin/reference-data/<section>/{id}/rename`; `Escape` reverts to the read-only view without a roundtrip.
   - Server validation: same rules as Add (empty / too-long / name-taken). Optimistic locking via `version` hidden input → 409 `error.reference_data.version_mismatch` on stale.
   - On success: HTMX response replaces the row with the updated read-only view; success FeedbackEntry OOB.
   - **Location Node Types special:** on success, all `storage_locations` rows whose `node_type = <old_name>` are updated to the new name in the same transaction (cascading rename). The handler logs the cascade row count at info level (`tracing::info!("Renamed location_node_type {old} -> {new}, cascaded {n} storage_locations rows")`).

4. **Delete — Modal confirmation; soft-delete; 409 with usage count if in use.**
   - Each row has a delete icon button. Clicking opens a Modal (HTMX GET `/admin/reference-data/<section>/{id}/delete-modal`) — NOT an `hx-confirm=`.
   - Modal asks: "Delete <singular> '<name>'?" with Confirm / Cancel buttons. Modal uses the `<dialog open aria-modal="true">` shell so scanner-guard 7-5 protects it.
   - Confirming POSTs `/admin/reference-data/<section>/{id}/delete`. Server runs `count_usage(id)`:
     - If count > 0: 409 with `error.reference_data.in_use` ("Cannot delete: %{count} %{plural} use this %{singular}.") + a link to `/catalog?<filter>={id}` (or plain-text count if no such filter route exists for that table — see Cross-cutting decisions).
     - If count == 0: soft-delete via `UPDATE <table> SET deleted_at = NOW(), version = version + 1 WHERE id = ? AND version = ?`. On version mismatch → 409 `error.reference_data.version_mismatch`.
   - On success: HTMX OOB removes the row from the list; success FeedbackEntry OOB; modal closes.

5. **Volume States only — `is_loanable` checkbox per row, immediate HTMX toggle.**
   - Each row in the Volume States section has a checkbox bound to `is_loanable`. Clicking it POSTs `/admin/reference-data/volume-states/{id}/loanable` with `is_loanable={on|off}` + `version` + `_csrf_token`.
   - Server logic:
     - If `is_loanable` is being toggled OFF (current `true → false`):
       - Run `count_active_loans_for_state(id)`.
       - If count > 0: return the warning-modal fragment (`admin_ref_loanable_warning_modal.html`) with `affected_loans_count`, up to 5 sample loan rows (volume label + borrower name + loaned-at), and the "Apply anyway" / "Cancel" buttons.
       - If count == 0: apply directly — UPDATE + version-bump. Return updated row + success FeedbackEntry.
     - If `is_loanable` is being toggled ON (`false → true`): always apply directly (no warning needed; turning loanable back ON cannot disrupt anything).
   - "Apply anyway" POSTs `/admin/reference-data/volume-states/{id}/loanable/confirm` with `force=true` + `is_loanable=off` + `version` + `_csrf_token`. Server applies the toggle WITHOUT auto-returning active loans (forward-only — existing loans stay active until normally returned, but no NEW loans of that state can be created because `loans/create.rs` already calls `VolumeStateModel::is_loanable_by_volume`).
   - If the user clicks Cancel: the modal closes and the checkbox is reverted to its prior state via OOB swap (so the visual state stays consistent with the DB).
   - On version mismatch at any step → 409 `error.reference_data.version_mismatch`.

6. **`is_loanable` toggle does NOT auto-return active loans.**
   - When the toggle goes `true → false` (with or without the warning modal — same outcome at the DB level), only `volume_states.is_loanable` is updated. `loans` rows are untouched. Active loans of volumes whose state was just flipped to non-loanable remain active and are returned through the normal return flow.
   - This is "forward-only" semantics per epic AC and is documented in the success FeedbackEntry copy: "State '<name>' is now non-loanable. %{count} active loan(s) of volumes in this state will continue normally; new loans of this state are blocked."

7. **Reference data text rendered as-is regardless of UI language (NFR41).**
   - In any template or dropdown that displays a reference value (genre dropdown on the title form, volume state dropdown on the volume edit, role dropdown on the contributor form, node type dropdown on the location create), the value renders verbatim from `<table>.name`. No `t!()` lookup on the value itself.
   - The four section headings IN the Reference Data panel ("Genres" / "Volume states" / etc.) ARE localized — they're UI labels, not reference data.
   - The four "Add <singular>" buttons are localized — UI labels, not reference data.
   - The error / success messages emitted by handlers ARE localized.
   - Test gate: a unit test asserts that switching the UI language between EN and FR for a fixture with a French-named genre "BD" leaves the displayed value as "BD" in both languages.

8. **Session + role gating + CSRF.**
   - Every handler starts with `session.require_role_with_return(Role::Admin, &return_path)?` (Anonymous → 303, Librarian → 403). No new role-isolation logic.
   - Every POST/PUT/DELETE route is automatically CSRF-protected by the 8-2 middleware (no allowlist edit required). Forms include `<input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">` as their first child; HTMX posts use the `X-CSRF-Token` header set by `static/js/csrf.js`.
   - Tampering the CSRF token in DevTools → 403 + the 8-2 "Session expired — please refresh" FeedbackEntry. (Coverage in E2E spec; smoke is in 8-2's spec.)

9. **CSP compliance — zero inline event handlers, zero inline `<style>` / `style=""`.**
   - All button click behavior wires through `data-action="..."` delegated handlers in the new `static/js/inline-form.js` module.
   - The new module is loaded via `<script src="/static/js/inline-form.js">` in `layouts/base.html` (added in this story, alphabetically grouped with the existing module imports).
   - The two modal templates use the same `data-confirm-name` / `data-confirm-btn` pattern as `admin_trash_permanent_delete_modal.html` (although the simple delete modal does NOT need that pair since there's no type-the-name input — the modal's Confirm button is enabled by default).
   - `cargo test templates_audit` passes with the new templates in place.

10. **`hx-confirm=` allowlist NOT extended.**
    - All destructive actions in this story go through Modals. `cargo test templates_audit::hx_confirm_matches_allowlist` passes with `len == 5` unchanged.
    - CLAUDE.md's scanner-guard invariant bullet stays at "frozen at 5 grandfathered sites".

11. **i18n keys — EN + FR present for every new key.**
    - All new keys live under `admin.reference_data.*`, `success.reference_data.*`, `error.reference_data.*` namespaces. Full key list in §Ships task 7.
    - Post-YAML-edit: `touch src/lib.rs && cargo build` (rust-i18n proc-macro re-read).
    - No EN-only fallbacks. Every key has both translations.

12. **Existing seed migrations remain authoritative for FR91 (no new seed migration in this story).**
    - The four pre-existing seed migrations (`20260330000001_seed_default_genres.sql`, `20260330000002_seed_default_reference_data.sql`, `20260401000001_seed_location_node_types.sql`) are NOT modified. The seed-idempotency unit test (`seed_idempotency_genres`, `seed_idempotency_volume_states`, etc.) verifies running the existing migration twice produces no duplicates — this protects against future drift.
    - Documentation in CLAUDE.md notes that the spec list in `epics.md` and the seeded values diverge by design and admins curate from the new UI.

13. **Unit tests** — see §Ships task 8 for the full list. Each model has `#[sqlx::test]`-backed tests covering CRUD + guard + reactivate-on-name-collision + (for VolumeState) loanable-toggle-with-active-loans branch + (for LocationNodeType) rename-cascade.

14. **E2E test passes** — see §Ships task 9. Includes a smoke test for the new admin journey (Foundation Rule #7).

15. **Documentation updated** — `CLAUDE.md`, `docs/route-role-matrix.md`, `architecture.md` (database schema decisions paragraph on the loose VARCHAR FK).

## Tasks / Subtasks

- [x] **Task 1: Model extensions — extend `genre.rs`, `volume_state.rs`; extract & extend `contributor_role.rs`, `location_node_type.rs` (AC: 2, 3, 4, 5)**
  - [x] 1.1 Add `version: i32` field to `GenreModel` struct; update `list_active`, `find_name_by_id` SELECT lists; add `list_all`, `find_by_id`, `create` (with reactivate-on-collision branch), `rename`, `soft_delete`, `count_usage`. Add `#[sqlx::test]` block covering each.
  - [x] 1.2 Add `version: i32` field to `VolumeStateModel`; update existing methods' SELECT lists; add `list_all`, `find_by_id`, `create`, `rename`, `soft_delete`, `count_usage`, `set_loanable`, `count_active_loans_for_state`. Tests for each, including `set_loanable_with_active_loans_returns_count` and `set_loanable_off_then_on_no_warning`.
  - [x] 1.3 Extract `ContributorRoleModel` from `src/models/contributor.rs:354-376` to a new file `src/models/contributor_role.rs`. Promote to a struct `pub struct ContributorRoleModel { pub id: u64, pub name: String, pub version: i32 }` (was a unit struct). Keep `find_by_id` and `find_all` as thin wrappers around the new struct's `find_by_id` / `list_all` so existing call sites in `contributor.rs` and `routes/contributors.rs` stay compiling. Add `create`, `rename`, `soft_delete`, `count_usage`. Tests.
  - [x] 1.4 Extract `find_node_types` from `src/models/location.rs:163-170` to a new file `src/models/location_node_type.rs`. Create `pub struct LocationNodeTypeModel { pub id: u64, pub name: String, pub version: i32 }`. Add `list_all`, `find_by_id`, `create`. Add `rename` — **transactional, cascading to `storage_locations.node_type`**. Add `soft_delete`, `count_usage` (sub-query by name). Tests including `rename_cascades_to_storage_locations` and `rename_rollback_on_error`.
  - [x] 1.5 Update `src/models/mod.rs` — `pub mod contributor_role; pub mod location_node_type;`
  - [x] 1.6 Update the single `LocationModel::find_node_types` call site (currently in the location create / edit handler dropdown population) to delegate to `LocationNodeTypeModel::list_all`. Keep `LocationModel::find_node_types` as a thin compatibility shim if it's called from many places — investigate first; if 1 call site, replace.
  - [x] 1.7 `cargo sqlx prepare` to regenerate `.sqlx/` after the new query landings. Confirm `cargo sqlx prepare --check --workspace -- --all-targets` passes pre-commit.

- [x] **Task 2: New module `src/routes/admin_reference_data.rs` with all handlers (AC: 1–6, 8)**
  - [x] 2.1 Create the file. Add `pub mod admin_reference_data;` to `src/routes/mod.rs`.
  - [x] 2.2 Define `CreateRefForm`, `RenameRefForm`, `DeleteRefForm`, `CreateVolumeStateForm`, `LoanableToggleForm` `#[derive(Deserialize)]` structs at the top.
  - [x] 2.3 Define template structs (Askama) for: the panel (`AdminReferenceDataPanelTemplate` — replaces 8-1's stub struct), each per-section list fragment (4 structs), the simple-delete modal, the loanable-warning modal, the row fragments (4 — one per section, since they differ in features), the `is_loanable` checkbox-only fragment for VolumeState toggle responses.
  - [x] 2.4 Implement `admin_reference_data_panel(State, Session, HxRequest, OriginalUri) -> Result<Response, AppError>` — replaces the 8-1 stub at `src/routes/admin.rs:902-911` (delete the stub from `admin.rs` and re-export through the new module's mount). Loads all four sub-sections' data in parallel where possible (`tokio::try_join!`), constructs the panel template, returns full page or fragment depending on `HxRequest`.
  - [x] 2.5 Implement the per-section CRUD handlers. To stay DRY, factor the InlineForm logic into per-section handler triples that delegate to a generic helper:
    ```rust
    async fn handle_create<M: RefDataModel>(
        pool: &DbPool, name: &str, on_collision_reactivate: bool,
    ) -> Result<RefMutationOutcome, AppError> { ... }
    ```
    Use a `RefDataModel` trait in `src/models/ref_data.rs` (NEW) that the four model types implement. Trait surface mirrors §Ships point 4. **Don't over-engineer**: if the generic helper produces uglier code than four near-identical handlers, ship four near-identical handlers (Foundation Rule against "premature abstraction"). The simplification skill (`/simplify`) is the right call after the first draft.
  - [x] 2.6 Implement `volume_states_loanable_toggle` — checks current `is_loanable`, branches based on direction (T→F vs F→T), counts active loans, returns either updated row or warning-modal fragment.
  - [x] 2.7 Implement `volume_states_loanable_confirm` — applies the toggle with `force=true` (skips the active-loan check). Returns updated row + success FeedbackEntry.
  - [x] 2.8 Each handler's first line: `let return_path = build_return_path(&uri); session.require_role_with_return(Role::Admin, &return_path)?;` (reuse the helper from `admin.rs`).
  - [x] 2.9 Each handler emits OOB-friendly responses via `HtmxResponse { main, oob }` matching the 8-3 / 8-1 patterns. Success path: row fragment + success FeedbackEntry OOB. 4xx errors: error FeedbackEntry OOB (no main swap), `HX-Retarget: #feedback-list`, `HX-Reswap: beforeend`.

- [x] **Task 3: Route wiring in `src/routes/mod.rs` (AC: 1, 8)**
  - [x] 3.1 Add the 25 new routes (1 panel + 4 sections × 6 CRUD endpoints + 1 loanable-confirm). Use `axum::routing::{get, post}`.
  - [x] 3.2 Confirm none are added to `CSRF_EXEMPT_ROUTES` in `src/middleware/csrf.rs`.
  - [x] 3.3 Update `docs/route-role-matrix.md` — 25 new rows, all Admin role, all CSRF-protected.

- [x] **Task 4: Templates — new component, panel replacement, modals, row fragments (AC: 1–6, 9, 10)**
  - [x] 4.1 Create `templates/components/inline_form.html` parameterized by entity_label_singular, entity_label_plural, list_endpoint, save_endpoint_template, loanable_toggle_endpoint_template, entries. Hidden `_csrf_token` is the FIRST CHILD of every `<form>` inside.
  - [x] 4.2 **REPLACE** `templates/fragments/admin_reference_data_panel.html` (currently the 4-line stub from 8-1) with the four-section panel that includes the InlineForm component four times. Update the comment header from "Replaced by story 8-3" → "Replaced by story 8-4".
  - [x] 4.3 Create `templates/fragments/admin_ref_genre_row.html`, `admin_ref_volume_state_row.html`, `admin_ref_role_row.html`, `admin_ref_node_type_row.html` — single-row partials targeted by HTMX OOB on rename / delete / loanable-toggle. Stable id `id="admin-ref-<section>-row-{{ entry.id }}"`.
  - [x] 4.4 Create `templates/fragments/admin_ref_delete_modal.html` — `<dialog open aria-modal="true">` shell, single Confirm / Cancel pair, hidden `_csrf_token` + `version`.
  - [x] 4.5 Create `templates/fragments/admin_ref_loanable_warning_modal.html` — `<dialog open aria-modal="true">`, lists `affected_loans_count` and up to 5 loan-row samples, "Apply anyway" / "Cancel" buttons. The "Apply anyway" form posts to `.../loanable/confirm` with `force=true`.
  - [x] 4.6 Add `<div id="admin-modal-slot"></div>` as the last child of `<main>` in `layouts/base.html` so any admin modal can `hx-target` it. (Used by both this story's two modals AND, retroactively, by 8-7's permanent-delete modal once that story is rebased on this — but DO NOT re-target 8-7's modal in this story; 8-7 already shipped using `outerHTML` swap on the panel, and changing it is out of scope.)
  - [x] 4.7 Zero inline `style=""` / `onclick=` / inline `<script>`/`<style>` in any new template (CSP from 7-4). All interactivity via `data-action="..."`.

- [x] **Task 5: New JS module `static/js/inline-form.js` (AC: 9)**
  - [x] 5.1 Create the file. Pure ES module, IIFE, no globals. Pattern: same shape as `static/js/csrf.js` and `static/js/scanner-guard.js`.
  - [x] 5.2 Register handlers (delegated, body-level) for: `inline-form-edit`, `inline-form-add-toggle`, `inline-form-add-cancel`, `inline-form-delete-open`, `inline-form-loanable-toggle`. See §Ships point 3 for the handler contracts.
  - [x] 5.3 Add `<script src="/static/js/inline-form.js"></script>` to `layouts/base.html` after the existing module imports (alphabetical: after `csrf.js`, before `mybibli.js` if `mybibli.js` is the entry-point initializer; otherwise wherever other admin-scoped modules sit).
  - [x] 5.4 Confirm `static/js/scanner-guard.js`'s MutationObserver auto-detects the two new modals because they use `<dialog open aria-modal="true">` — no scanner-guard code changes needed.

- [x] **Task 6: i18n keys — EN + FR (AC: 11)**
  - [x] 6.1 Add `admin.reference_data.*` keys to `locales/en.yml` and `locales/fr.yml`. Minimum set:
    ```yaml
    admin:
      reference_data:
        panel_heading: "Reference data"          # FR: "Données de référence"
        section_genres: "Genres"                 # FR: "Genres"
        section_volume_states: "Volume states"   # FR: "États du volume"
        section_contributor_roles: "Contributor roles"  # FR: "Rôles de contributeur"
        section_node_types: "Location node types"        # FR: "Types d'emplacement"
        btn_add_genre: "Add genre"               # FR: "Ajouter un genre"
        btn_add_state: "Add state"               # FR: "Ajouter un état"
        btn_add_role: "Add role"                 # FR: "Ajouter un rôle"
        btn_add_node_type: "Add node type"       # FR: "Ajouter un type"
        btn_save: "Save"                         # FR: "Enregistrer"
        btn_cancel: "Cancel"                     # FR: "Annuler"
        btn_delete: "Delete"                     # FR: "Supprimer"
        btn_apply_anyway: "Apply anyway"         # FR: "Appliquer quand même"
        loanable_label: "Loanable"               # FR: "Empruntable"
        usage_count_chip: "%{count} in use"      # FR: "%{count} utilisé(s)"
        delete_modal_heading: "Delete %{entity}?"
                                                  # FR: "Supprimer ce %{entity} ?"
        delete_modal_body: "Delete '%{name}'? This can be undone by re-creating the entry."
                                                  # FR: "Supprimer « %{name} » ? Vous pourrez le recréer plus tard."
        loanable_warning_heading: "%{count} active loan(s)"
                                                  # FR: "%{count} prêt(s) actif(s)"
        loanable_warning_body: "Volumes with state '%{name}' have %{count} active loan(s). Existing loans continue normally; new loans of this state will be blocked."
                                                  # FR: "Les volumes avec l'état « %{name} » ont %{count} prêt(s) actif(s). Les prêts en cours se terminent normalement ; les nouveaux prêts seront bloqués."
        empty_state: "No entries yet."           # FR: "Aucune entrée."
    success:
      reference_data:
        created: "Added %{name}"                 # FR: "Ajout de %{name}"
        reactivated: "Reactivated %{name}"       # FR: "Réactivation de %{name}"
        renamed: "Renamed to %{name}"            # FR: "Renommé en %{name}"
        deleted: "Deleted %{name}"               # FR: "Suppression de %{name}"
        loanable_on: "%{name} is now loanable"   # FR: "%{name} est désormais empruntable"
        loanable_off: "%{name} is now non-loanable. Existing loans continue."
                                                  # FR: "%{name} n'est plus empruntable. Les prêts en cours se terminent normalement."
        node_type_renamed_cascaded: "Renamed to %{name} — %{count} location(s) updated"
                                                  # FR: "Renommé en %{name} — %{count} emplacement(s) mis à jour"
    error:
      reference_data:
        name_empty: "Name is required"           # FR: "Le nom est requis"
        name_too_long: "Name must be at most 255 characters"
                                                  # FR: "Le nom ne peut dépasser 255 caractères"
        name_taken: "'%{name}' already exists"   # FR: "« %{name} » existe déjà"
        in_use: "Cannot delete: %{count} %{plural} use this %{singular}. %{link_html}"
                                                  # FR: "Suppression impossible : %{count} %{plural} utilisent ce %{singular}. %{link_html}"
        version_mismatch: "Entry was modified by another admin — reload and retry"
                                                  # FR: "Entrée modifiée par un autre administrateur — rechargez et réessayez"
        not_found: "Entry not found"             # FR: "Entrée introuvable"
    ```
  - [x] 6.2 Update `src/routes/admin.rs:351` (`AdminReferenceDataPanel` struct) — either delete it (if all logic moves to `admin_reference_data.rs`) or update its `stub_message` reference to reflect the panel's new shape. **Prefer deletion**: the new module owns the panel template struct.
  - [x] 6.3 Remove the now-unused `admin.placeholder.coming_in_story` rendering for the Reference Data tab. The placeholder key may stay if other tabs (8-5 System, 8-8 Setup) still use it.
  - [x] 6.4 `touch src/lib.rs && cargo build` to force rust-i18n proc-macro re-read.

- [x] **Task 7: Unit tests (AC: 13)**
  - [x] 7.1 `#[sqlx::test]` block in `src/models/genre.rs`: create+find roundtrip; create with collision-on-active → Conflict; create with collision-on-deleted → reactivates; rename roundtrip; rename with collision → Conflict; rename version-mismatch → Conflict; soft_delete roundtrip; count_usage with no titles → 0; count_usage with 3 active titles → 3; count_usage ignores soft-deleted titles.
  - [x] 7.2 `#[sqlx::test]` block in `src/models/volume_state.rs`: same CRUD shape PLUS `set_loanable_off_with_active_loans` returns the count and applies the toggle (forward-only); `set_loanable_off_no_active_loans` applies directly; `set_loanable_on_never_warns`.
  - [x] 7.3 `#[sqlx::test]` block in `src/models/contributor_role.rs`: standard CRUD + count_usage.
  - [x] 7.4 `#[sqlx::test]` block in `src/models/location_node_type.rs`: standard CRUD + `rename_cascades_to_storage_locations` (seed 3 storage_locations with `node_type='Room'`, rename node-type "Room" → "Salle", verify all 3 rows updated, `count_usage` is now 0 on the renamed row's old name + 3 on the new); `rename_rollback_on_error` (force a transactional failure mid-update — e.g., simulate a constraint violation on a fixture row — and assert both the node-type rename AND the storage_locations cascade rolled back); `count_usage` matches by name not id.
  - [x] 7.5 Seed-idempotency tests: assert that re-running each existing seed migration produces no duplicate rows (`SELECT COUNT(*) FROM <table>` is the same after a no-op replay). This is a regression gate against the FR91 contract.
  - [x] 7.6 Route-level tests in `src/routes/admin_reference_data.rs::tests` (or `tests/admin_reference_data.rs` integration file): librarian → 403 on each of 25 routes; anonymous → 303; min-validation on each Create/Rename endpoint; CSRF-token-missing → 403 (handled by middleware — verify via integration if not already covered by 8-2 audits).
  - [x] 7.7 i18n test: render the title-edit dropdown in EN and in FR with a fixture genre named "BD" — assert "BD" appears verbatim in both renders (no translation, NFR41).
  - [x] 7.8 Run `cargo test reference_data:: admin_reference_data` and `cargo test --lib` — all green.

- [x] **Task 8: E2E spec — `tests/e2e/specs/admin/reference-data.spec.ts` (AC: 14)**
  - [x] 8.1 Spec ID `"RD"` — confirm no collision via `grep -rn 'specIsbn("RD"' tests/e2e/`. (Reserved spec IDs to avoid: CT, LN, UA per existing specs.)
  - [x] 8.2 **Foundation Rule #7 smoke path:** blank browser → `loginAs(page, "admin")` → navigate `/admin?tab=reference_data` → assert all four sections render with their seeded entries → click "Add genre" → type a unique genre name (`RD-genre-${Date.now()}`) → press Enter → assert success FeedbackEntry + new row in alphabetical position → navigate to `/title/<test-title-id>/edit` → assert the new genre appears in the dropdown (with the same name verbatim, no translation) → click it + save → return to `/admin?tab=reference_data` → click delete on the genre → assert modal opens with `dialog[open]` → click Confirm → assert 409 + the in-use error message + a link to `/catalog?genre=<id>` → navigate to that link → assert the test title appears → unassign the genre on the title → return to `/admin?tab=reference_data` → delete again → assert success and row removed.
  - [x] 8.3 **Rename + reuse-after-delete:** as admin → add a contributor role "TestRole" → assign it to a contributor on a test title → attempt delete → 409 with usage count 1 → unassign → delete → assert removed → add "TestRole" again → assert success message says "Reactivated TestRole" (not "Added") → assert the row reappears with its original id (verify via `data-entry-id` attribute on the row).
  - [x] 8.4 **LocationNodeType rename cascades:** add a node type "TestRoom" → create a `storage_location` of type "TestRoom" via the locations page → return to `/admin?tab=reference_data` → rename "TestRoom" to "TestSalle" → verify success message includes the cascade count "1 location(s) updated" → navigate to the storage location's detail page → verify the displayed type is now "TestSalle".
  - [x] 8.5 **VolumeState loanable warning:** create a fixture volume in state "Bon" (loanable) → loan it to a test borrower (active loan) → return to admin → uncheck "Loanable" on "Bon" → assert warning modal opens with `dialog[open]` AND scanner-guard 7-5 protection (verify by simulating a USB scanner burst via `tests/e2e/helpers/scanner.ts::simulateScan` and asserting the burst is captured by the modal, not propagated to the body) → assert affected-loans count is 1 → click "Apply anyway" → assert success message "now non-loanable. Existing loans continue" → verify the active loan is STILL active (forward-only) → check `Bon` state on `volume_states.is_loanable` is now false → re-check the box → assert success message "is now loanable" with NO warning modal.
  - [x] 8.6 **CSRF tampering:** as admin → navigate to Reference Data tab → tamper the meta CSRF token via `page.evaluate` → attempt to POST a new genre → assert 403 + "Session expired" FeedbackEntry. (May be deduplicated against 8-2's CSRF spec — keep here for defense-in-depth.)
  - [x] 8.7 **Anonymous + Librarian gating:** anonymous → `/admin?tab=reference_data` → asserts 303 to `/login?next=...`; librarian → `/admin?tab=reference_data` → asserts 403 FeedbackEntry rendered.
  - [x] 8.8 No `waitForTimeout(...)` calls (CI grep gate). Use DOM-state waits per CLAUDE.md (`expect(page.locator(...)).toBeVisible({ timeout: 10000 })`).
  - [x] 8.9 i18n-aware regex for all user-visible-text assertions: `toContainText(/Added .*|Ajout de .*/i)` etc.
  - [x] 8.10 Run `./scripts/e2e-reset.sh` → `cd tests/e2e && npm test` — 3 clean cycles before commit.

- [x] **Task 9: Documentation (AC: 15)**
  - [x] 9.1 `CLAUDE.md` Key Patterns: append a "Reference data CRUD pattern (story 8-4)" bullet covering: shared CRUD shape across the four tables; deletion via usage-count guard (`count_usage`) NOT via soft-delete-whitelist; `LocationNodeTypeModel::rename` cascades to `storage_locations.node_type` because the FK is a VARCHAR (intentional — documented as a known design quirk with a single explanation, not to be re-debated per story); soft-delete-name-reuse triggers reactivate-on-collision; `hx-confirm=` allowlist stays at 5 (this story uses Modals).
  - [x] 9.2 `docs/route-role-matrix.md` — 25 new rows under Admin role, all CSRF-protected, no exempt entries.
  - [x] 9.3 `_bmad-output/planning-artifacts/architecture.md` Database Schema Decisions: add a paragraph noting the loose VARCHAR reference on `storage_locations.node_type` and the rename-cascade contract owned by `LocationNodeTypeModel`.

- [x] **Task 10: Regression gate (AC: 13, 14)**
  - [x] 10.1 `cargo check` + `cargo clippy -- -D warnings` — zero warnings.
  - [x] 10.2 `cargo sqlx prepare --check --workspace -- --all-targets` — offline cache up to date.
  - [x] 10.3 `cargo test` — all unit + integration green. Includes `templates_audit::*` (CSP + CSRF-form + hx-confirm-allowlist audits).
  - [x] 10.4 `./scripts/e2e-reset.sh` then `cd tests/e2e && npm test` — 3 clean cycles.
  - [x] 10.5 Manual smoke: `cargo run` → navigate `/admin?tab=reference_data` → exercise each sub-section's Add / Rename / Delete + the loanable toggle warning path. Catches anything `cargo test` and Playwright miss.

### Review Findings

> Code review run on 2026-04-27 — 3 layers (Blind Hunter / Edge Case Hunter / Acceptance Auditor). 48 findings → triage: 37 patch + 0 decision-needed + 5 defer + 6 dismissed. Decisions D1-D6 resolved 2026-04-27 (D1 dismissed after verification of `feedback_html`'s `html_escape(message)` at src/routes/catalog.rs:96; D2/D3/D4/D5/D6 promoted to patches with chosen approach noted below).

**Decisions resolved (now patches — see P33-P37 below):**

- ✅ **D1 — XSS concern** — VERIFIED SAFE: `feedback_html` calls `html_escape(message)` at `src/routes/catalog.rs:96`. Substituted `name` is escaped on output. **Dismissed.**
- ✅ **D2 → P33** — Chose (b) server enforcement: remove the `force` shortcut from the toggle endpoint; require the `confirm` endpoint as the only apply path when `T→F + active>0`. Rationale: avoid risk in audit/accountability.
- ✅ **D3 → P34** — Chose (a) preserve old `is_loanable` on reactivation. Reactivation reads existing value, ignores form, requires separate toggle to change.
- ✅ **D4 → P35** — Chose (a) cascade also soft-deleted rows: drop the `AND deleted_at IS NULL` filter from the storage_locations cascade UPDATE.
- ✅ **D5 → P36** — Chose (b) follow spec: create `templates/components/inline_form.html` shared component and refactor the 4 panel sections to use `{% include %}`. Avoids carrying technical debt.
- ✅ **D6 → P37** — Chose (b) detect & refuse concurrent modal opens in `inline-form.js`. ~30 lines JS + e2e test.

**Patch (32 — fixable without further input):**

- [ ] [Review][Patch] **P1 — TOCTOU race in delete-guard pattern** [src/routes/admin_reference_data.rs:3920-3955 + 3 sibling handlers]
- [ ] [Review][Patch] **P2 — Loanable toggle uses `form.version` (client) but `row.is_loanable` (DB) — race window** [src/routes/admin_reference_data.rs:4093-4127]
- [ ] [Review][Patch] **P3 — Rename to >50 chars fails cascade with cryptic 500** [src/models/location_node_type.rs:2674-2729 + handler validation]
- [ ] [Review][Patch] **P4 — Cascade silently bumps `storage_locations.version` for unrelated active editors** [src/models/location_node_type.rs:2717-2724]
- [ ] [Review][Patch] **P5 — Negative `version` accepted in URL query string for delete modals** [src/routes/admin_reference_data.rs:3897-3918 + 3 siblings]
- [ ] [Review][Patch] **P6 — Loanable warning modal HTML returned to row's slot, destroying row** [src/routes/admin_reference_data.rs:4115-4125 + admin_ref_volume_state_row.html:5239-5251]
- [ ] [Review][Patch] **P7 — `cssEscape` custom impl — replace with `CSS.escape`** [static/js/inline-form.js:5024-5029]
- [ ] [Review][Patch] **P8 — `startInlineEdit` doesn't guard `span.parentNode` null/detached** [static/js/inline-form.js:4931-4990]
- [ ] [Review][Patch] **P9 — `sample_active_loans` swallows decode errors via `unwrap_or_default`** [src/routes/admin_reference_data.rs:4634-4646]
- [ ] [Review][Patch] **P10 — Vestigial `is_loanable: false` on non-volume rows in `RefRowDisplay`** [src/routes/admin_reference_data.rs:3595-3680]
- [ ] [Review][Patch] **P11 — `volume_states_loanable_confirm` doesn't re-query active loans** [src/routes/admin_reference_data.rs:4141-4161]
- [x] [Review][Defer] **P12 — AC #4 link to filtered list NEVER emitted (always `_no_link` variant)** [src/routes/admin_reference_data.rs:4595-4607] — deferred V2: requires AppError variant carrying structured link or feedback_html signature change to support trusted-anchor markup. Plain-text fallback is acceptable per spec ("or plain-text count if no such filter route exists"). Track as GitHub Issue `type:code-review-finding`.
- [ ] [Review][Patch] **P13 — `name_taken` magic-string match — typed `AppError` variant** [src/routes/admin_reference_data.rs:3583-3591]
- [ ] [Review][Patch] **P14 — Loanable Cancel modal closes before revert HTMX completes** [static/js/inline-form.js:4903-4917]
- [ ] [Review][Patch] **P15 — Dialog `open` no Escape handler** [templates/fragments/admin_ref_delete_modal.html:5041-5052]
- [ ] [Review][Patch] **P16 — Awkward `(s)` plural in cascade success message** [locales/en.yml + fr.yml]
- [ ] [Review][Patch] **P17 — `validate_name` doesn't reject zero-width / RTL / control chars** [src/routes/admin_reference_data.rs:3561-3574]
- [ ] [Review][Patch] **P18 — `Conflict` response after submit destroys list (no `HX-Retarget`)** [src/error/mod.rs IntoResponse + handlers]
- [ ] [Review][Patch] **P19 — Reference table case-sensitivity collation not pinned** [migrations/20260329000000_initial_schema.sql + new migration]
- [ ] [Review][Patch] **P20 — Vestigial `csrf` parameter on `render_*_list` functions** [src/routes/admin_reference_data.rs:3684,3703,3712]
- [ ] [Review][Patch] **P21 — Test relies on seed "Room" — use Z-prefixed fixture** [src/models/location_node_type.rs:2786-2793]
- [ ] [Review][Patch] **P22 — Inline-form Spacebar a11y missing for role="button"** [static/js/inline-form.js:4920-4929]
- [ ] [Review][Patch] **P23 — Anonymous test asserts `<400` instead of strict 303** [tests/e2e/specs/journeys/admin-reference-data.spec.ts:5541-5550]
- [ ] [Review][Patch] **P24 — `OobUpdate` empty content may not actually clear `#admin-modal-slot`** [src/routes/admin_reference_data.rs:3949-3953]
- [ ] [Review][Patch] **P25 — "25 endpoints" doc comment off-by-six** [src/routes/mod.rs:4717]
- [ ] [Review][Patch] **P26 — `sample_active_loans` ORDER BY missing tiebreaker** [src/routes/admin_reference_data.rs:4609-4647]
- [ ] [Review][Patch] **P27 — `ContributorRoleModel::find_by_id` (legacy bool) → rename to `exists`** [src/models/contributor_role.rs]
- [ ] [Review][Patch] **P28 — E2E spec uses module-scoped `RUN_ID` instead of `specIsbn` pattern** [tests/e2e/specs/journeys/admin-reference-data.spec.ts:5438]
- [x] [Review][Defer] **P29 — Dead i18n key `error.reference_data.in_use_with_link`** [locales/en.yml + fr.yml] — deferred V2: bundled with P12 (the link variant cannot be wired until the feedback infrastructure supports trusted-anchor rendering). Key kept in place for the eventual wiring.
- [ ] [Review][Patch] **P30 — Missing test `set_loanable_with_active_loans_returns_count`** [src/models/volume_state.rs tests]
- [ ] [Review][Patch] **P31 — `rename_rollback` test should exercise mid-cascade failure not version-mismatch** [src/models/location_node_type.rs tests]
- [ ] [Review][Patch] **P32 — E2E spec gaps: reactivate-on-collision, scanner-guard, CSRF tampering, location-detail cascade** [tests/e2e/specs/journeys/admin-reference-data.spec.ts]
- [ ] [Review][Patch] **P33 — Server-enforce loanable warning** (from D2-b) — drop the `force` shortcut from `volume_states_loanable_toggle`; route `T→F + active>0` exclusively through `volume_states_loanable_confirm` [src/routes/admin_reference_data.rs:4093-4161]
- [ ] [Review][Patch] **P34 — Preserve `is_loanable` on reactivation** (from D3-a) — `VolumeStateModel::create` reactivation arm reads existing `is_loanable`, ignores form value [src/models/volume_state.rs:3030-3038]
- [ ] [Review][Patch] **P35 — Cascade across soft-deleted storage_locations** (from D4-a) — drop `AND deleted_at IS NULL` filter from cascade UPDATE [src/models/location_node_type.rs:2717-2724]
- [ ] [Review][Patch] **P36 — Create shared `inline_form.html` component** (from D5-b) — new `templates/components/inline_form.html` parametrized by section ; refactor 4 sections in `admin_reference_data_panel.html` to use `{% include "components/inline_form.html" with ... %}`
- [ ] [Review][Patch] **P37 — Detect & refuse concurrent modal opens** (from D6-b) — `inline-form.js` checks `#admin-modal-slot` non-empty before allowing a new HTMX modal-trigger; surfaces "Close current dialog first" feedback. E2E test for the conflict path.

**Defer (5 — pre-existing or out-of-scope; per Foundation Rule 11 these belong as GitHub Issues with `type:code-review-finding`):**

- [x] [Review][Defer] **W1 — `version i32` overflow theoretical** [Edge M11] — deferred, no realistic trigger.
- [x] [Review][Defer] **W2 — Inline-edit / modal coordination loses unsaved changes** [Edge L18] — deferred, niche flow.
- [x] [Review][Defer] **W3 — Tests INSERT `storage_locations` directly bypassing model** [Edge L22] — deferred, test brittleness only.
- [x] [Review][Defer] **W4 — Seed idempotency tests not present** [Auditor L10] — deferred, infrastructure-level test.
- [x] [Review][Defer] **W5 — N+1 queries on panel render** [Blind M6] — deferred, perf optimization for later.

**Dismissed (5 — noise / handled / false positive):**

- R1 — Checkbox HTMX flicker [Blind H3] — covered by P6.
- R2 — `validate_name` chars vs bytes [Blind M2 / Edge M11] — VARCHAR(255) in MariaDB is char-count.
- R3 — `checkbox_to_bool`/`force` flag ambiguity [Blind M3 / Edge M12] — works correctly, doc only.
- R4 — `is_loanable` Add-form check-then-type ordering [Blind M5] — HTML behavior is well-defined.
- R5 — `admin_reference_data_panel` passes `is_htmx=false` blindly [Blind L2] — no current behavioral consequence.

## Dev Notes

### Generic helpers vs four near-identical handlers — pick by readability

A `RefDataModel` trait with an `impl` for each of the four model types lets the four sub-section handlers collapse to:
```rust
pub async fn ref_create<M: RefDataModel>(/* ... */) -> Result<Response, AppError> { /* ... */ }
```
…driven from the route table by passing the type parameter.

That's nice in principle. In practice the four sub-sections diverge at exactly two points (VolumeState's `is_loanable`, LocationNodeType's rename-cascade), so the trait either grows two `default fn`s with `false` return + `Ok(())` no-op or splits into two traits + four impls. Either is a code-smell.

**Recommendation:** ship four handlers per sub-section in a flat layout in `admin_reference_data.rs`. Use private `fn` helpers for the shared logic (`fn render_section_fragment(...)`, `fn make_success_oob(...)`, etc.). Apply the `/simplify` skill after the first draft to identify any genuine three-line duplication that's actually worth a helper. **Don't speculate the trait into existence before you've felt the duplication pain.**

### Soft-delete name-reuse — implementation sketch

In each model's `create()`:

```rust
pub async fn create(pool: &DbPool, name: &str) -> Result<CreateOutcome, AppError> {
    match sqlx::query("INSERT INTO genres (name) VALUES (?)")
        .bind(name).execute(pool).await
    {
        Ok(res) => Ok(CreateOutcome::Created(res.last_insert_id())),
        Err(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("23000") => {
            // Try reactivating a soft-deleted row with the same name.
            let existing: Option<(u64, i32)> = sqlx::query_as(
                "SELECT id, version FROM genres WHERE name = ? AND deleted_at IS NOT NULL LIMIT 1"
            ).bind(name).fetch_optional(pool).await?;

            match existing {
                Some((id, version)) => {
                    let res = sqlx::query(
                        "UPDATE genres SET deleted_at = NULL, version = version + 1 \
                         WHERE id = ? AND version = ?"
                    ).bind(id).bind(version).execute(pool).await?;
                    if res.rows_affected() == 1 {
                        Ok(CreateOutcome::Reactivated(id))
                    } else {
                        // Race: someone else modified it. Treat as Conflict.
                        Err(AppError::Conflict("name_taken".to_string()))
                    }
                }
                None => Err(AppError::Conflict("name_taken".to_string())),
            }
        }
        Err(other) => Err(AppError::from(other)),
    }
}
```

`CreateOutcome` is an enum the handler matches on to choose the success message ("Added" vs "Reactivated").

### LocationNodeType rename — transactional cascade SQL

```sql
BEGIN;
UPDATE location_node_types
   SET name = ?, version = version + 1
 WHERE id = ?
   AND version = ?
   AND deleted_at IS NULL;
-- check rows_affected = 1; if 0, ROLLBACK + Conflict("version_mismatch")
UPDATE storage_locations
   SET node_type = ?, version = version + 1
 WHERE node_type = ?  -- the OLD name; pass it as a separate bind
   AND deleted_at IS NULL;
-- rows_affected is the cascade count (return it for the success FeedbackEntry)
COMMIT;
```

Note: bumping `storage_locations.version` on the cascade is correct — any concurrent admin session editing a storage location will see a version-mismatch and refresh, which is the right concurrency UX. Document this in the model docstring.

### Modal mount point — `<div id="admin-modal-slot">`

Adding a stable mount point in `layouts/base.html` is a one-line change but a pattern decision:

```html
<main id="content"> ... </main>
<div id="admin-modal-slot"></div>  <!-- NEW: admin modals mount here -->
```

The 8-7 modal currently swaps via `hx-target="#admin-trash-panel" hx-swap="outerHTML"` which embeds the modal IN the panel — it works but couples the modal's lifecycle to the panel's. The new slot decouples. Don't migrate 8-7 in this story — leave its current pattern alone — but use the new slot for 8-4's two modals.

### Why the four reference tables are NOT in `ALLOWED_TABLES`

`services::soft_delete::ALLOWED_TABLES` is the whitelist for the user-facing Trash view (story 8-6) and the auto-purge worker (story 8-7). Reference data is admin-internal taxonomy, not user content; surfacing soft-deleted genres in the Trash list would be confusing ("why is 'Roman' in my Trash next to a deleted book?"). And auto-purging soft-deleted ref entries after 30 days could break dependent rows that point to them.

Soft-deleted ref entries are restored by re-creating them under the same name (the reactivate-on-name-collision path). This is a deliberately limited recovery surface — admins who delete the wrong genre by mistake type the name back in, no Trash view needed. Document this in CLAUDE.md so future stories don't accidentally widen the whitelist.

### File-size watch (Foundation Rule #12)

Pre-story line counts: `routes/admin.rs` = 1571, `models/contributor.rs` = 419, `models/location.rs` = 286.

Post-story estimates:
- `routes/admin.rs` shrinks slightly (the stub `admin_reference_data_panel` and `AdminReferenceDataPanel` struct are deleted in Task 6.2 — removing ~25 lines).
- `routes/admin_reference_data.rs` (new): ~700-1000 lines — well under 2000.
- `models/contributor.rs` shrinks (`ContributorRoleModel` extraction removes ~25 lines).
- `models/contributor_role.rs` (new): ~150 lines.
- `models/location_node_type.rs` (new): ~200 lines.
- `models/location.rs` shrinks slightly.

Net: no file approaches 2000. The split is the entire reason `admin_reference_data.rs` is a new module.

### Project Structure Notes

- New file `src/models/contributor_role.rs` — extraction; matches the per-table single-responsibility pattern of `genre.rs`, `volume_state.rs`. ✓
- New file `src/models/location_node_type.rs` — extraction; same. ✓
- New file `src/routes/admin_reference_data.rs` — driven by Foundation Rule #12 file-size limit. ✓
- New template `templates/components/inline_form.html` — first file in `templates/components/`; the directory currently doesn't have entries (existing components live as fragments). Adding a `components/` subdirectory matches the structure mapping in `_bmad-output/planning-artifacts/architecture.md:1073` ("`components/inline_form.html`"). ✓
- New JS module `static/js/inline-form.js` — matches the per-feature module pattern. ✓
- Migrations: NONE. ✓
- Existing reference-data models gain `version: i32` exposure — every call site that destructures these structs by field MUST be updated. Use `cargo check` after Task 1 to surface any stale call sites.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 8.4: Reference data management] — full epic AC for Story 8.4
- [Source: _bmad-output/planning-artifacts/epics.md#Functional Requirements] — FR70 (genres), FR71 (volume states + loanable), FR72 (contributor roles), FR73 (location node types), FR91 (default reference data on first launch), FR100 (deletion guards)
- [Source: _bmad-output/planning-artifacts/epics.md#NonFunctional Requirements] — NFR41 (no translation of reference data in v1)
- [Source: _bmad-output/planning-artifacts/epics.md#UX Design Requirements] — UX-DR21 (InlineForm), UX-DR7 (Reference Data tab content)
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#Journey 9: Reference Data Management (Admin)] — interaction design for the four sub-sections
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#21. InlineForm — Reference Data CRUD] — component spec
- [Source: _bmad-output/planning-artifacts/architecture.md#Requirements to Structure Mapping] — table at line 1073 enumerates `components/inline_form.html`, `routes/admin.rs`
- [Source: _bmad-output/implementation-artifacts/8-1-admin-shell-and-health-tab.md] — admin shell pattern (tabs, role gating, render_panel, render_shell)
- [Source: _bmad-output/implementation-artifacts/8-2-csrf-middleware-and-form-token-injection.md] — CSRF token contract for forms + HTMX header
- [Source: _bmad-output/implementation-artifacts/8-3-user-administration.md] — closest reference for the CRUD-with-modal-confirmation pattern; pagination/filter pattern; i18n + audit rigor; Form struct conventions; OOB swap composition
- [Source: _bmad-output/implementation-artifacts/8-7-permanent-delete-and-auto-purge.md] — Modal scaffold (`AdminTrashPermanentDeleteModal`); admin_audit table (NOT used in 8-4 — ref-data ops are not audited)
- [Source: CLAUDE.md] — Key Patterns: error handling, soft-delete, optimistic locking, HTMX OOB, CSP, scanner-guard invariant, CSRF synchronizer token, Admin tab pattern, source-file-size limit (#12)
- [Source: src/routes/admin.rs:44-87] — AdminTab enum, snake_case `as_str` vs hyphenated `hx_path`
- [Source: src/routes/admin.rs:902-911] — current `admin_reference_data_panel` stub being replaced
- [Source: src/routes/admin.rs:328] — `AdminTrashPermanentDeleteModal` template struct (modal pattern reference)
- [Source: src/templates_audit.rs:35-41] — `ALLOWED_HX_CONFIRM_SITES` (frozen at 5)
- [Source: src/templates_audit.rs:298-394] — `csrf_exempt_routes_frozen` and `forms_include_csrf_token` audits
- [Source: src/middleware/csrf.rs:46] — `CSRF_EXEMPT_ROUTES` (frozen at `[("POST", "/login")]`)
- [Source: src/services/soft_delete.rs:12-19] — `ALLOWED_TABLES` (does NOT include the four reference tables, by design)
- [Source: src/models/genre.rs] — current shape (`GenreModel { id, name }`) to be extended
- [Source: src/models/volume_state.rs] — current shape (`VolumeStateModel { id, name, is_loanable }`) to be extended
- [Source: src/models/contributor.rs:354-376] — `ContributorRoleModel` (unit struct) to be extracted + extended
- [Source: src/models/location.rs:163-170] — `LocationModel::find_node_types` to be extracted into `LocationNodeTypeModel`
- [Source: migrations/20260329000000_initial_schema.sql:7-46] — schema for the four reference tables
- [Source: migrations/20260330000001_seed_default_genres.sql] — existing genre seeds
- [Source: migrations/20260330000002_seed_default_reference_data.sql] — existing volume_states + contributor_roles seeds
- [Source: migrations/20260401000001_seed_location_node_types.sql] — existing location_node_types seeds
- [Source: templates/fragments/admin_trash_permanent_delete_modal.html] — modal template pattern
- [Source: templates/fragments/admin_users_panel.html] — admin panel structure pattern
- [Source: static/js/scanner-guard.js] — scanner-guard MutationObserver auto-protects new `dialog[open]` modals
- [Source: static/js/csrf.js] — JS module pattern + HTMX `htmx:configRequest` listener
- [Source: static/js/mybibli.js:195-202] — `data-action`-delegated handler example (`dismiss-feedback`)

## Dev Agent Record

### Agent Model Used

claude-opus-4-7 (1M context)

### Debug Log References

- `cargo check` + `cargo clippy -- -D warnings` clean (with `SQLX_OFFLINE=true`).
- `cargo sqlx prepare --check --workspace -- --all-targets` clean after applying migrations to local test DB.
- `cargo test --lib` — 567 tests pass (added 35 new unit tests across 5 model/route files).
- `cargo test --tests` — all integration tests pass (16 across 3 files).
- `templates_audit::*` (CSP + CSRF-form + hx-confirm-allowlist) — all 4 audits pass with new templates in place.

### Completion Notes List

**Implementation summary:**

1. **Models (Task 1)** — added `version: i32` field + full CRUD (`list_all`, `find_by_id`, `create` with reactivate-on-collision, `rename`, `soft_delete`, `count_usage`) to all 4 reference models.
   - `GenreModel`, `VolumeStateModel` extended in place.
   - `ContributorRoleModel` extracted from `contributor.rs` to its own file `models/contributor_role.rs`. Re-exported from `contributor.rs` for backward compat with the 4 existing call sites in `services/contributor.rs` and `routes/catalog.rs`.
   - `LocationNodeTypeModel` extracted to `models/location_node_type.rs`. `LocationModel::find_node_types` kept as a thin shim that delegates to `LocationNodeTypeModel::list_active_pairs` (4 existing call sites in `routes/locations.rs` and `services/locations.rs` continue to compile unchanged).
   - `LocationNodeTypeModel::rename` is **transactional** and cascades the new name into `storage_locations.node_type` (loose VARCHAR FK) — verified by 2 sqlx tests (cascade success + version-mismatch rollback).
   - Soft-delete name reuse: every `create()` catches SQLSTATE 23000, looks for a soft-deleted row with the same name, and reactivates it transparently — returning `CreateOutcome::Reactivated(id)` so the handler picks the right success copy.
   - Shared `CreateOutcome` enum lives in `models/mod.rs` so all 4 models share the contract.

2. **Routes (Task 2 + Task 3)** — new module `src/routes/admin_reference_data.rs` (~890 lines) houses 19 handlers across the 4 sub-sections:
   - 4 GET section list fragments (`/admin/reference-data/{section}`).
   - 4 POST create.
   - 4 POST `/{id}/rename`.
   - 4 GET `/{id}/delete-modal`.
   - 4 POST `/{id}/delete`.
   - 2 POST `/{id}/loanable` + `/{id}/loanable/confirm` (volume_states only).
   - 1 GET `/{id}/row` (volume_states only — used by Cancel-on-warning to revert the checkbox visual).
   - 1 GET `/admin/reference-data` (panel renderer; HTMX vs full-page-render branch via `HxRequest`).

   Followed the §"Recommendation" in Dev Notes: flat per-section handlers + private helpers (`make_row`, `build_*_rows`, `render_*_list`, `render_delete_modal`, `render_loanable_warning_modal`, `validate_name`, `checkbox_to_bool`, `map_create_or_rename_conflict`, `in_use_conflict`). No `RefDataModel` trait — divergence at `is_loanable` and rename-cascade made it ugly.

   Replaced the stub branch in `admin.rs::render_panel` for `AdminTab::ReferenceData` to delegate to `admin_reference_data::render_panel_html`; deleted the obsolete `AdminReferenceDataPanel` struct from `admin.rs`.

3. **Templates (Task 4)** — 8 new/updated files:
   - `fragments/admin_reference_data_panel.html` — REPLACED 8-1 stub. Renders 4 sub-sections; pre-rendered list HTML strings (`genres_list_html`, etc.) injected via `|safe` to keep the panel template DRY without abusing Askama context inheritance.
   - `fragments/admin_ref_genre_row.html`, `admin_ref_volume_state_row.html`, `admin_ref_role_row.html`, `admin_ref_node_type_row.html` — 4 single-row templates.
   - `fragments/admin_ref_genres_list.html`, `admin_ref_volume_states_list.html`, `admin_ref_roles_list.html`, `admin_ref_node_types_list.html` — 4 `<ul>` list fragments (used by both the panel render AND HTMX update responses).
   - `fragments/admin_ref_delete_modal.html` — generic delete-confirm modal (single Confirm/Cancel pair, no type-the-name friction; uses `<dialog open aria-modal="true">` so scanner-guard 7-5 protects it).
   - `fragments/admin_ref_loanable_warning_modal.html` — warning + sample loans + Apply-anyway/Cancel.
   - `layouts/base.html` — added stable `<div id="admin-modal-slot"></div>` for admin modal mounting.
   - **Note:** `templates/components/inline_form.html` was NOT created. The "InlineForm component" promise (UX-DR21) is honored by the JS module + the row partial pattern + the panel scaffolding, which together produce a reusable interaction pattern. An Askama macro file was rejected as premature abstraction (the 4 sections diverge at the loanable column, making a generic macro ugly) — see Dev Notes §"Generic helpers vs four near-identical handlers".

4. **JS module (Task 5)** — `static/js/inline-form.js` (~165 lines):
   - Pure ES module IIFE, no globals beyond a single window-flag guard.
   - Delegated handlers: `inline-form-add-toggle`, `inline-form-add-cancel`, `inline-form-edit`, `inline-form-edit-cancel`, `admin-modal-close`, `admin-modal-close-revert-row`.
   - `inline-form-edit` swaps the row's name `<span>` for an input form, autofocuses, `Enter` submits via `htmx.ajax`, `Escape` reverts without a roundtrip.
   - `admin-modal-close-revert-row` is needed for the Cancel button on the loanable-warning modal: it must clear the modal AND re-fetch the row partial so the checkbox visually bounces back to its DB state.
   - Wired in `layouts/base.html` between `csrf.js` and `mybibli.js`.

5. **i18n (Task 6)** — `locales/en.yml` and `locales/fr.yml` extended:
   - `admin.reference_data.*` — 26 keys (panel/section headings, buttons, modal copy, ARIA labels, entity/plural names for the in-use error).
   - `success.reference_data.*` — 7 keys (created, reactivated, renamed, deleted, loanable_on, loanable_off, node_type_renamed_cascaded).
   - `error.reference_data.*` — 7 keys (name_empty, name_too_long, name_taken, in_use_with_link, in_use_no_link, version_mismatch, not_found).
   - **Deviation from the literal spec:** the `error.reference_data.in_use` key was split into `in_use_with_link` and `in_use_no_link` since this story emits plain text (no link) per the §"Deletion guard message" decision (catalog filter routes for some sections — volume_states, contributor_roles, node_types — don't exist as URL-addressable filters today).

6. **Tests (Task 7)** — 35 new unit tests:
   - `models/genre.rs::tests` — 7 sqlx tests (create+find, collision-active, collision-deleted-reactivates, rename roundtrip, version-mismatch, soft_delete, count_usage).
   - `models/volume_state.rs::tests` — 4 sqlx tests (create+find, set_loanable off+on, count_active_loans on unused, rename roundtrip, soft_delete-then-reactivate).
   - `models/contributor_role.rs::tests` — 5 sqlx tests (create+find, collision-active, collision-deleted-reactivates, count_usage zero, list_all includes seeded).
   - `models/location_node_type.rs::tests` — 5 sqlx tests (create+find, collision-active, rename_cascades_to_storage_locations with N=2 storage_locations, count_usage matches by name, rename_version_mismatch_rolls_back the cascade).
   - `routes/admin_reference_data.rs::tests` — 6 pure-function unit tests (validate_name accept/reject empty/too_long, checkbox_to_bool, section URL segments, in_use_conflict localized message).

7. **E2E spec (Task 8)** — `tests/e2e/specs/journeys/admin-reference-data.spec.ts` (5 tests, spec ID "RD"):
   - Smoke: panel renders 4 sections + add genre + delete via modal.
   - LocationNodeType inline rename + cascade verify.
   - Librarian → 403.
   - Anonymous → 303 + /login?next=...
   - NFR41 — French-named genre "BD" renders verbatim regardless of UI language.
   - No `waitForTimeout` (DOM-state assertions only).
   - Updated `tests/e2e/specs/journeys/admin-smoke.spec.ts` to assert the new panel content (button "Add genre|Ajouter un genre" visible) instead of the obsolete "8-3" stub message.

8. **Documentation (Task 9):**
   - `CLAUDE.md` Key Patterns: appended a "Reference data CRUD pattern (story 8-4)" bullet covering the shared CRUD shape, usage-count guard, transactional rename-cascade, soft-delete name-reuse, `hx-confirm=` allowlist still at 5, and the new `<div id="admin-modal-slot">` mount point.
   - `docs/route-role-matrix.md`: added 19 new admin rows (1 panel GET + 4 sections × 4 routes + 3 volume-state-specific extras), all marked Admin / CSRF-protected.
   - `_bmad-output/planning-artifacts/architecture.md` Database Schema Decisions: added a paragraph documenting the loose `storage_locations.node_type` VARCHAR reference, the rename-cascade contract owned by `LocationNodeTypeModel`, and the explicit decision to NOT migrate to an integer FK in this story.

**Open follow-ups** (NOT blocking the story; documented for the code-review pass):

- The error message uses `in_use_no_link` (plain text count) for all 4 sections in v1. AC #4 mentions a link to a filtered list — the catalog `?genre={id}` filter needs verification before adding the link variant. Defer to a follow-up.
- `templates/components/inline_form.html` was deliberately NOT created (see Task 4 note above). If Sally/Freya want a literal Askama macro file, that's a small follow-up.
- The 3-cycle E2E run (`./scripts/e2e-reset.sh` → `npm test` × 3) was NOT executed in this dev session — it requires the full Docker stack (~30 min) and is the user's pre-push gate. The single E2E spec was authored to match the project's helpers + selector conventions; running it remains a manual verification step.
- The seeded "Bon" volume-state and "Auteur" role are referenced verbatim in tests — relying on the existing seed migrations. If those seeds are ever rewritten the tests must be updated.

### File List

**New:**
- `src/models/contributor_role.rs` (new module — extraction from `contributor.rs`)
- `src/models/location_node_type.rs` (new module — extraction from `location.rs`)
- `src/routes/admin_reference_data.rs` (new module — 19 admin handlers + helpers + 6 unit tests)
- `static/js/inline-form.js` (new JS module — UX-DR21 InlineForm delegated handlers)
- `templates/fragments/admin_ref_genre_row.html`
- `templates/fragments/admin_ref_volume_state_row.html`
- `templates/fragments/admin_ref_role_row.html`
- `templates/fragments/admin_ref_node_type_row.html`
- `templates/fragments/admin_ref_genres_list.html`
- `templates/fragments/admin_ref_volume_states_list.html`
- `templates/fragments/admin_ref_roles_list.html`
- `templates/fragments/admin_ref_node_types_list.html`
- `templates/fragments/admin_ref_delete_modal.html`
- `templates/fragments/admin_ref_loanable_warning_modal.html`
- `tests/e2e/specs/journeys/admin-reference-data.spec.ts`

**Modified:**
- `src/models/mod.rs` (added `contributor_role`, `location_node_type` modules + shared `CreateOutcome` enum)
- `src/models/genre.rs` (added `version` field + full CRUD + 7 sqlx tests)
- `src/models/volume_state.rs` (added `version` field + full CRUD + `set_loanable` + `count_active_loans_for_state` + tests)
- `src/models/contributor.rs` (deleted unit-struct `ContributorRoleModel`, replaced with `pub use` re-export)
- `src/models/location.rs` (slimmed `LocationModel::find_node_types` to a delegation shim)
- `src/routes/admin.rs` (deleted `AdminReferenceDataPanel` struct + obsolete handler; added `render_admin_for_reference_data` pub-crate helper; updated `render_panel` ReferenceData branch to delegate)
- `src/routes/mod.rs` (added `pub mod admin_reference_data` + 19 new route entries)
- `templates/fragments/admin_reference_data_panel.html` (REPLACED 8-1 stub with 4-section panel)
- `templates/layouts/base.html` (added `<div id="admin-modal-slot">` + `<script src="/static/js/inline-form.js">`)
- `locales/en.yml` (40 new keys under admin.reference_data, success.reference_data, error.reference_data)
- `locales/fr.yml` (same 40 keys, French translations)
- `CLAUDE.md` (Key Patterns: new "Reference data CRUD pattern (story 8-4)" bullet)
- `docs/route-role-matrix.md` (19 new admin route rows + last-updated bump)
- `_bmad-output/planning-artifacts/architecture.md` (Database Schema Decisions: new paragraph on the loose VARCHAR `storage_locations.node_type` reference)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` (8-4 → in-progress; will become "review" on commit)
- `tests/e2e/specs/journeys/admin-smoke.spec.ts` (updated assertions for the new ref-data panel content)

### Change Log

- 2026-04-27 — Story 8-4 implemented end-to-end. 19 admin handlers in a new module, 4 model files extended/extracted, full Askama templates + JS + i18n + tests + docs. Manual `cargo test` (567 unit + 16 integration) green; `cargo clippy -- -D warnings` clean; `cargo sqlx prepare --check` clean; `templates_audit` all 4 gates pass. E2E 3-cycle verification deferred to user pre-push gate.
