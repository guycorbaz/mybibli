# Story 9.15: StatusMessage — empty states (encouraging, role-aware)

Status: in-progress

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As any user (anonymous, librarian, or admin),
I want clear, encouraging empty-state messages on every list / search / dashboard view,
so that an empty result feels like a starting point, not a dead end.

## ⚠️ Existing-code reality check

Before writing a single line, walk the surfaces this story touches and verify the assumptions below — they are LOCKED IN by current main as of 2026-05-07 (post 9-14 close):

- **No `components/status_message.html` exists yet.** This story creates it. Sibling components live in `templates/components/`: `feedback_entry.html` (Foundation Rule pattern — pre-rendered HTML returned by handlers), `loan_status_badge.html`, `cover.html`, `nav_bar.html`, `modal.html` (10/11-param macro from 9-10..9-14). The `modal.html` macro is the closest precedent for the macro signature shape. NEW component — no migration of existing macro.

- **`role` variable is in scope on every page that extends `layouts/base.html`.** Defined in the base context as `String` ("anonymous" / "librarian" / "admin"). All page templates already pass it. The role-gated CTA logic must be evaluated **server-side via Askama `{% if %}`** — NOT JS. Mirror of `home.html:408` (`{% if role == "librarian" || role == "admin" %}`) which is the existing precedent for role-gated CTA on the search-empty-results surface.

- **5 in-scope empty-state surfaces (post validation-driven descoping) — implementation matrix:**

  | # | Surface | File | Current state | Migration scope |
  |---|---------|------|---------------|-----------------|
  | 1 | `/loans` zero active | `templates/pages/loans.html:77-78` | `<p class="text-center py-12 text-stone-500 dark:text-stone-400">{{ empty_state }}</p>` | Migrate to `status_message::status_message` macro call; drop the `empty_state` field from the `LoansTemplate` struct (handler-side wiring) and replace with `empty_heading` + `empty_body` populated via `t!()` calls. |
  | 2 | `/borrowers` | `templates/pages/borrowers.html:51-52` | Same shape as loans. | Same migration. CTA wires to `cta_url = "#add-form"` (the existing fragment-anchor toggle on `borrowers.html:10-49`), NOT `/borrowers/new` (which does not exist as a separate route — verified `src/routes/mod.rs:190` has only `"/borrowers"` GET+POST). |
  | 3 | `/series` | `templates/pages/series_list.html:17-18` | Same shape. | Same migration. CTA wires to `cta_url = "/series/new"` (verified route exists at `src/routes/mod.rs:170-173`). |
  | 4 | `/?filter=...` filtered home with zero matches | `templates/pages/home.html:405-411` (the empty `<div>` starts at line 405, the `<p>` is at 407, the role-gated `{% if %}` is at 408 — verify in Task 1) | Already has the role-aware CTA shape — closest precedent to the new macro. Empty-results div is INSIDE `{% if let Some(paginated) = results %}`, which is `Some` when query OR filter is active. | Migrate to the macro; macro replaces the existing div + SVG + role-gated `<a>`. |
  | 5 | search-no-results (q=...) | Same file as #4. | Same surface — `/?q=...` and `/?filter=...` both flow through the same empty-results path. | Single migration covers both. |

  **Out-of-scope surfaces (originally listed in AC8 but descoped after validation):**
  - `/contributors` — DOES NOT EXIST (no list route; only `/contributor/:id` detail). File at story close as `type:change-request` GH issue: "add `/contributors` list page (with empty-state via `status_message`)".
  - `/catalog` — NOT a list view (catalog is the scan workflow page). The "no titles in DB at all" surface would belong to the home page first-launch path; see below.
  - **Home page first-launch** ("no titles in DB at all") — `src/routes/home.rs:210-228` shows that `results` is `Some(...)` ONLY when `query` is non-empty OR `has_filter` is true. When BOTH are empty, `results = None` and the empty-results branch (`{% if let Some(paginated) = results %}`) is NEVER reached on `/` — the home page renders dashboard widgets (Recent additions, Glance, Stats by genre) instead. Adding a first-launch empty state requires either (a) a NEW outer template branch + a SQL probe for "is DB empty" in `home.rs`, or (b) a redesign of which widgets render when the DB is empty. Both are PRD-level decisions, not 9-15 mechanical scope. **Defer to a follow-up story** (`type:change-request`).
  - `/title/:id` no volumes — `title_detail.html` has NO `volumes.is_empty()` branch (verified). Out of scope.
  - `/borrower/:id` no loan history — `borrower_detail.html:48-49` shows an INLINE `<p class="text-sm text-stone-500 dark:text-stone-400">{{ no_active_loans_label }}</p>` (no `text-center py-12`, no centered macro layout). This is a section-status pattern, not a full-page empty state. Migrating it to the centered macro would change UX visibly (inline → centered with `py-12`). The macro is designed for full-page empty lists; inline sub-section status is a distinct design pattern. **Defer to a follow-up story** if/when a unified inline/centered macro variant is justified.

  **Plus 3 admin reference-data surfaces** that the AC8 contract doesn't list but match the same pattern (out of scope for primary 9-15; documented as a deferred follow-up):
  - `templates/fragments/admin_ref_genres_list.html:3-4`
  - `templates/fragments/admin_ref_node_types_list.html:3-4`
  - `templates/fragments/admin_ref_roles_list.html:3-4`
  - `templates/fragments/admin_ref_volume_states_list.html:3-4`
  - `templates/fragments/admin_users_table.html:2-3`
  - `templates/fragments/admin_trash_panel.html:31-33`

  These admin surfaces are deferred to a follow-up sweep — they don't block the AC8 contract and the story explicitly scopes "list / search / dashboard view" surfaces, not admin internals.

- **Existing `home.html:405-411` empty-search div** (verify exact line range in Task 1 — the `<div>` opens around 405, the `<svg>` is on 406, the `<p>{{ no_results_text }}</p>` is on 407, the `{% if role == "librarian" || role == "admin" %}` is on 408, the CTA `<a>` is on 409) is the **canonical precedent** for the new macro shape. The new macro produces the same visual treatment (UX-DR24 calm stone neutral) when called with the equivalent params.

  **Icon decision** (frozen): v1 macro takes NO icon param — heading + body + optional CTA only. The existing search-empty `<svg>` is preserved by emitting it OUTSIDE the macro call (immediately before, inside the wrapper div). All other surfaces ship without icons, matching their current state. A future polish story can add an `icon_svg` param if usage demand emerges.

- **`home.html:407+409` carry TWO i18n strings** (`no_results_text` + `no_results_create`) populated by `src/routes/home.rs:541-542` (verify in Task 1). Migration drops both in favor of two new field clusters (search sub-case + filter sub-case); see AC6.

- **`is_empty()` empty branches without empty-state markup** (silent UX gaps that this story may OR may NOT close):
  - `templates/pages/contributor_detail.html:26` — `{% if !titles.is_empty() %}<div>...</div>{% endif %}` — when `titles` is empty, NOTHING renders. **AC8 doesn't list `/contributor/:id` as a surface** — out of scope; do NOT add an empty state here in 9-15.
  - `templates/pages/title_detail.html:66` — `{% if !contributors.is_empty() %}` — same shape. Out of scope.
  - `templates/pages/title_detail.html:117` — `{% if !all_series.is_empty() %}` — admin-only series-assignment widget; out of scope.

  These silent-empty-when-empty cases are intentional UI design (a section header without items is more confusing than no header). Not 9-15 scope.

- **Foundation Rule #1 (DRY)** is the primary motivator: 5+ pages currently hand-roll the same `<p class="text-center py-12 text-stone-500 dark:text-stone-400">` shape. Centralizing this into one component is the whole point.

- **CSP compliance (story 7-4)**: the new component must use CSS classes only — no inline `style=`, no `<style>` blocks, no `onclick=`. Tailwind utilities only. The `templates_audit::no_inline_markup_in_templates` test already covers this.

- **Anonymous + role-aware CTA**: the macro receives `cta_role_gate: String` ("" / "librarian" / "admin"). Empty string = show to all (including anonymous); "librarian" = show to librarian/admin only; "admin" = show to admin only. **Anonymous never sees a CTA when `cta_role_gate != ""`.** This is enforced inside the macro via Askama `{% if %}` against the `role` variable, which the caller passes as the 7th param to the macro (sibling of `csrf_token` in the modal pattern).

## Acceptance Criteria

1. **AC1 — NEW macro `templates/components/status_message.html`** with the following signature:
   ```jinja
   {%- macro status_message(variant, heading, body_html, cta_label, cta_url, cta_role_gate, role) -%}
   ```
   - 7 positional params:
     - `variant: &str` — `"empty"` or `"info"` (the `info` variant reserved for future use; v1 ships `empty` only as the styled variant).
     - `heading: &str` — short heading text (e.g. "No active loans").
     - `body_html: &str` — body copy, rendered via `{{ body_html|safe }}`. Caller is responsible for HTML-escaping any user-supplied interpolation BEFORE building the body (mirror of the 9-10..9-14 modal macro's `body_html` contract — same supply-chain risk preserved as deferred GH #137 work).
     - `cta_label: &str` — CTA button text. Empty string = no CTA.
     - `cta_url: &str` — CTA link href. Empty string = no CTA. **Both `cta_label` and `cta_url` must be non-empty for the CTA to render.**
     - `cta_role_gate: &str` — `""` (show to all), `"librarian"` (show to librarian + admin), `"admin"` (show to admin only). Anonymous sees the CTA only when `cta_role_gate == ""`.
     - `role: &str` — the page's current role (`"anonymous"` / `"librarian"` / `"admin"`); used to evaluate the role gate.
   - Renders `<div class="text-center py-12 text-stone-500 dark:text-stone-400" data-status-message data-variant="{{ variant }}">` followed by `<h2 class="text-base font-semibold text-stone-700 dark:text-stone-200">{{ heading }}</h2>`, body via `<p class="mt-2">{{ body_html|safe }}</p>`, and optional CTA `<a href="{{ cta_url }}" class="mt-4 inline-block text-indigo-600 dark:text-indigo-400 hover:underline">{{ cta_label }}</a>`.
   - **CTA render condition** (server-side, single Askama expression): `{% if cta_label != "" && cta_url != "" && (cta_role_gate == "" || cta_role_gate == role || (cta_role_gate == "librarian" && role == "admin")) %}`. The third clause handles the admin-as-librarian relation (admin > librarian, both pass librarian-gated CTAs).
   - Tailwind palette: warm stone neutral (per UX-DR24); NO red, NO amber. Visual treatment matches the existing `home.html:404-411` div for continuity.
   - CSP-clean: no inline `style=`, no `<style>` blocks, no `onclick=`. Verified by `templates_audit::no_inline_markup_in_templates`.
   - The `data-status-message` and `data-variant` attributes are STABLE selectors for AC9 unit tests + AC10 E2E (mirror of the `data-modal-variant` pattern from the modal macro).

2. **AC2 — i18n keys for the 5 in-scope surfaces, naming `empty.<surface>_<part>`**:
   - `empty.loans_heading` + `empty.loans_body` — `/loans` zero active loans. NO CTA in v1 (the librarian's "create loan" path is the scan workflow, not a button).
   - `empty.borrowers_heading` + `empty.borrowers_body` + `empty.borrowers_cta` — `/borrowers` zero borrowers. CTA label "Add a borrower" / "Ajouter un emprunteur" linking to fragment anchor `#add-form` (the existing add-form toggle in `borrowers.html`). Role-gated `librarian`.
   - `empty.series_heading` + `empty.series_body` + `empty.series_cta` — `/series` zero series. CTA "Create a series" / "Créer une série" linking to `/series/new`. Role-gated `librarian`.
   - `empty.search_heading` + `empty.search_body` + `empty.search_cta` — `/?q=…` with zero matches. CTA "Add this title" / "Ajouter ce titre" linking to `/catalog/title/new?title={query}`. Role-gated `librarian`.
   - `empty.filter_heading` + `empty.filter_body` — `/?filter=…` with zero matches. NO CTA in v1 (filter-driven empties don't have a single right action; future polish story can add per-filter CTAs).
   - **Naming convention**: top-level `empty:` block in each locale file, with `<surface>_<part>` keys underneath. The convention is **NEW** (existing locale files use per-domain blocks like `series.empty_state`); choosing a top-level `empty:` block consolidates all empty-state strings in one place, mirroring the centralization the macro itself provides. The 6 existing per-domain `empty_state` keys (loans, borrowers, series, etc.) are dropped in favor of the new keys.
   - **Total: 13 new keys per locale** (2 + 3 + 3 + 3 + 2 = 13). 26 keys across both locales.
   - **CTA URLs are NOT in i18n** — they are template literals passed to the macro call (e.g., `cta_url = "/series/new"` directly in the Askama `{% call %}`). URLs are not user-facing text; routing changes shouldn't require translator review.
   - All copy in EN + FR. Encouraging tone — no "no data", "no results"; use inviting verbs ("Start", "Add", "Try", "Scan"). Gender-neutral FR (per the 9-14 review patch convention — "cette personne", "essayez", etc.).
   - Run `cargo test all_t_keys_have_both_locales` after adding keys.

3. **AC3 — Migrate `templates/pages/loans.html:77-78`** to use the macro:
   - Before:
     ```html
     {% if loans.items.is_empty() %}
     <p class="text-center py-12 text-stone-500 dark:text-stone-400">{{ empty_state }}</p>
     {% endif %}
     ```
   - After:
     ```html
     {% if loans.items.is_empty() %}
     {% call status_message::status_message(
         "empty",
         empty_heading,
         empty_body,
         "",
         "",
         "",
         role,
     ) %}{% endcall %}
     {% endif %}
     ```
   - Drop the `empty_state` field from `LoansTemplate` (Rust struct in `src/routes/loans.rs`); replace with `empty_heading` + `empty_body` populated via `t!("empty.loans_heading")` and `t!("empty.loans_body")`.
   - The `{% import "components/status_message.html" as status_message %}` at the top of `loans.html` is required.

4. **AC4 — Migrate `templates/pages/borrowers.html:51-52`** to the macro (same shape as AC3):
   - CTA wired: `cta_label` from `t!("empty.borrowers_cta")`, `cta_url = "#add-form"` (the existing fragment-anchor toggle on `borrowers.html:10-49` — `/borrowers/new` does NOT exist as a separate route).
   - `cta_role_gate = "librarian"`.
   - Drop the page template's `empty_state` field; add `empty_heading`, `empty_body`, `empty_cta_label`, `empty_cta_url` fields. The `cta_role_gate` is hardcoded in the template call (string literal `"librarian"`), not a struct field.

5. **AC5 — Migrate `templates/pages/series_list.html:17-18`** to the macro (same shape as AC4 with `cta_url = "/series/new"`).

6. **AC6 — Migrate `templates/pages/home.html:405-411`** (the search-no-results + filter-no-results path — 2 sub-cases):
   - This migration preserves the existing SVG icon (emitted OUTSIDE the macro since v1 macro has no icon param) and replaces the role-gated `<a>` block with the macro:
     ```html
     {% if paginated.items.is_empty() %}
     <div class="text-center py-12 text-stone-500 dark:text-stone-400">
         <svg class="mx-auto w-12 h-12 text-stone-300 dark:text-stone-600 mb-3" ...>...</svg>
         {% if !active_filter.is_empty() %}
         {# Filter-driven empty: no CTA, neutral copy #}
         {% call status_message::status_message(
             "empty",
             filter_empty_heading,
             filter_empty_body,
             "",
             "",
             "",
             role,
         ) %}{% endcall %}
         {% else %}
         {# Search-driven empty: librarian/admin sees "Add this title" CTA #}
         {% call status_message::status_message(
             "empty",
             search_empty_heading,
             search_empty_body,
             search_empty_cta,
             search_empty_cta_url,
             "librarian",
             role,
         ) %}{% endcall %}
         {% endif %}
     </div>
     {% endif %}
     ```
   - **Two sub-cases** distinguish themselves by `active_filter` state (the only thing that varies — at this point in the template we already know `paginated.items.is_empty()` AND `results = Some(_)` which means a query OR filter was active):
     - `active_filter` non-empty → filter-driven empty (no CTA)
     - `active_filter` empty (so a query drove the search) → search-driven empty (librarian/admin CTA to "Add this title")
   - **NOTE on `text-center py-12` wrapper**: kept on the outer `<div>` to preserve the icon's centering. The macro itself ALSO emits `text-center py-12` on its inner `<div>`. This DOES double the `py-12` (24 vs 12) in the search/filter case — accept this as a one-off for the icon path; if it's visually too much, the icon path can drop the outer `py-12` once it's confirmed safe (Task 1 visual check).
   - Page template fields added: `search_empty_heading`, `search_empty_body`, `search_empty_cta`, `search_empty_cta_url`, `filter_empty_heading`, `filter_empty_body` (6 new fields). Drop `no_results_text` and `no_results_create` (verify zero callers via grep).
   - **First-launch sub-case** ("/ with no titles in DB at all") is OUT OF SCOPE — see the reality-check out-of-scope notes. The home page's `results = None` branch never reaches the empty-state block today; adding it requires a separate SQL probe + new template branch (PRD-level decision deferred to a follow-up `type:change-request`).

7. **AC7 — Story-level surface contract**: the following 5 surfaces emit `status_message::status_message` on empty (the in-scope subset of the original AC8 contract from epics.md, after validation-driven descoping):
   - ✅ `/loans` zero active — AC3
   - ✅ `/borrowers` zero borrowers — AC4
   - ✅ `/series` zero series — AC5
   - ✅ `/?q=...` no search results — AC6 (search sub-case)
   - ✅ `/?filter=...` filtered no results — AC6 (filter sub-case)
   - ❌ Out of scope (each filed as a deferred GH issue at story close):
     - `/` home no titles at all (first-launch) — `results = None` branch never reaches the empty path today; needs PRD-level redesign + SQL probe.
     - `/borrower/:id` no loan history — uses inline `<p class="text-sm">` styling, not the centered macro pattern; section-status vs full-page-empty design split.
     - `/title/:id` no volumes — no `volumes.is_empty()` branch exists in `title_detail.html`.
     - `/contributors` — no list page route exists.
     - `/catalog` zero titles — catalog is the scan workflow page, not a list view.

8. **AC8 — Templates audit + i18n parity stay green**:
   - `cargo test no_inline_markup_in_templates` passes (the new macro uses CSS classes only).
   - `cargo test all_t_keys_have_both_locales` passes (every new key has EN + FR entries).
   - `cargo test forms_include_csrf_token` not affected (no new forms).
   - `cargo test hx_confirm_matches_allowlist` stays at `&[]` post-9-14.

9. **AC9 — Unit tests** (NEW file `src/routes/status_message_tests.rs`, mirror of `src/routes/modal_tests.rs` from 9-10) — exercise the macro via a tiny test wrapper:
    - **NEW wrapper template** `templates/fragments/status_message_test_wrapper.html` (mirror of `modal_test_wrapper.html`):
      ```jinja
      {% import "components/status_message.html" as status_message %}
      {% call status_message::status_message(
          variant, heading, body_html, cta_label, cta_url, cta_role_gate, role,
      ) %}{% endcall %}
      ```
    - **Test cases** (11 cases):
      1. `empty_variant_renders_heading_and_body` — basic render, no CTA, no role gate.
      2. `cta_renders_when_label_and_url_both_non_empty` — both `cta_label` + `cta_url` non-empty + no role gate → CTA visible.
      3. `cta_omitted_when_label_empty` — `cta_label = ""`, `cta_url = "/foo"` → no `<a>` element.
      4. `cta_omitted_when_url_empty` — `cta_label = "Foo"`, `cta_url = ""` → no `<a>` element.
      5. `cta_role_gate_librarian_hides_for_anonymous` — `cta_role_gate = "librarian"`, `role = "anonymous"` → no `<a>` element.
      6. `cta_role_gate_librarian_shows_for_librarian` — same gate, `role = "librarian"` → CTA visible.
      7. `cta_role_gate_librarian_shows_for_admin` — same gate, `role = "admin"` → CTA visible (admin > librarian).
      8. `cta_role_gate_admin_hides_for_librarian` — `cta_role_gate = "admin"`, `role = "librarian"` → no CTA.
      9. `cta_role_gate_admin_shows_for_admin` — `cta_role_gate = "admin"`, `role = "admin"` → CTA visible.
      10. `body_html_safe_passes_markup_through` — `body_html = "<em>hi</em>"`, assert raw `<em>hi</em>` (NOT entities) appears in output (locks the `|safe` contract).
      11. `data_attributes_are_stable_selectors` — assert `data-status-message` AND `data-variant="empty"` both appear (locks the AC1 stable-selector contract for E2E).
    - All 11 unit tests must pass.

10. **AC10 — E2E test** — NEW spec file `tests/e2e/specs/journeys/empty-states.spec.ts`. The empty-state journey crosses multiple pages and isn't a natural fit inside any existing spec (each existing spec is page-scoped).
    - **Test 1 — anonymous on `/series` (public read) sees empty state WITHOUT CTA**:
      - Blank context (clearCookies). Navigate to `/series` with empty-series DB state (Playwright global setup or fixture).
      - Verify: page renders, the StatusMessage heading + body appear, NO CTA `<a>` is visible.
    - **Test 2 — librarian on `/series` sees empty state WITH CTA**:
      - `loginAs(page, "librarian")`. Navigate to `/series` with empty DB.
      - Verify CTA "Create a series" / "Créer une série" visible AND clicking it navigates to `/series/new`.
    - **Test 3 — search-no-results role-aware CTA**:
      - `loginAs(page, "librarian")`. Navigate to `/?q=ZZZZNonExistentTitle`. Verify the empty state shows + "Add this title" CTA visible (librarian role).
      - `clearCookies()` + navigate to same URL (anonymous). Verify empty state shows but NO CTA.
    - **Test 4 — i18n round-trip**:
      - One test asserts the EN copy. Another asserts the FR copy by setting the `lang` cookie OR the `Accept-Language` header. Use the existing language-toggle pattern from `tests/e2e/specs/journeys/language-toggle.spec.ts` (if it exists; otherwise trace the `lang` cookie path in `src/middleware/locale.rs`).
    - **Flake gate**: `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` MUST stay clean.

11. **AC11 — Foundation Rule #12 LOC discipline**:
    - `templates/components/status_message.html`: NEW file, ~25 LOC.
    - `templates/fragments/status_message_test_wrapper.html`: NEW file, ~5 LOC.
    - `src/routes/status_message_tests.rs`: NEW file, ~250 LOC of unit tests.
    - `tests/e2e/specs/journeys/empty-states.spec.ts`: NEW file, ~120 LOC.
    - `templates/pages/loans.html`, `borrowers.html`, `series_list.html`: each net change ~+5/-2 LOC (replace `<p>...</p>` with macro call + import).
    - `templates/pages/home.html`: net change ~+18/-7 LOC (2 sub-case branching for filter / search).
    - `src/routes/loans.rs`, `src/routes/borrowers.rs`, `src/routes/series.rs`, `src/routes/home.rs`: each net change ~+5/-1 LOC (replace `empty_state` field with `empty_heading` + `empty_body` + optional CTA fields). Current LOC: loans.rs ~600, borrowers.rs ~400, series.rs ~600, home.rs ~700 — all comfortably under 2000.
    - `locales/en.yml` and `locales/fr.yml`: +13 new keys per locale, −6 dropped (existing per-domain `<domain>.empty_state` keys replaced by the centralized `empty.<surface>_<part>` cluster). Net +7 keys per locale.

12. **AC12 — Story-level grep audit**: at story close, run two greps and document the output in Dev Agent Record:
    - `grep -rnE 'class=".*py-12.*text-stone-500' templates/pages` — should return 0 hits in `templates/pages/`. The only remaining occurrence is the new `status_message.html` macro in `templates/components/`, which is correct.
    - `grep -rn 'empty_state' src/ templates/` — should match ZERO struct fields and ZERO template variables (the `empty_state` field name is dropped on `LoansTemplate`, `BorrowersTemplate`, `SeriesListTemplate`; the new fields use `empty_heading` / `empty_body` / etc.). The 6 i18n keys `<domain>.empty_state` in locales should also be gone.

13. **AC13 — Local Testing Before Push (Foundation Rule #13)**: run the full local gate before opening the PR:
    - `SQLX_OFFLINE=true cargo check` — clean
    - `cargo clippy --all-targets -- -D warnings` — clean
    - `cargo test --lib` — green (≥757 lib + the new ~11 status_message_tests cases + existing integration suites)
    - `cargo test no_inline_markup_in_templates` — green
    - `cargo test all_t_keys_have_both_locales` — green
    - Existing 9-14-touched suites (`admin_user_deactivate_modal`, `series_delete_modal`, etc.) — still green; this story doesn't touch the modal foundation.
    - Full E2E via `./scripts/e2e-reset.sh` + `cd tests/e2e && npm test` — green; pay attention to the new `empty-states.spec.ts`.
    - Flake gate: `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` returns nothing.

14. **AC14 — Draft PR + CI gate (Foundation Rule #15 + #18)**: open a draft PR at the first commit and WAIT for CI green before requesting review or merging.

## Tasks / Subtasks

- [ ] **Task 1 — Verify reality-check assumptions (AC: all)**
  - [ ] Read `templates/pages/loans.html:77-78`, `borrowers.html:51-52`, `series_list.html:17-18` to confirm the identical hand-rolled `<p class="text-center py-12 text-stone-500 dark:text-stone-400">{{ empty_state }}</p>` shape (verified pre-spec; re-confirm before edit).
  - [ ] Read `templates/pages/home.html:400-460` to confirm the search/filter empty branching shape — locate the `<div>` at the empty-results path (~line 405), the `<svg>` icon, the `<p>{{ no_results_text }}</p>`, and the `{% if role == "librarian" || role == "admin" %}` precedent. Pin the exact line numbers in Dev Agent Record.
  - [ ] Verify `/series/new` route exists at `src/routes/mod.rs:170-173`. **`/borrowers/new` does NOT exist** (confirmed); the borrowers CTA wires to fragment `#add-form` instead (uses the existing toggle in `borrowers.html:10-49`).
  - [ ] Verify `role` is in scope on every migrated template (`loans.html`, `borrowers.html`, `series_list.html`, `home.html`) — read the corresponding Rust template structs and confirm `role: String` is a field.
  - [ ] Measure current LOC of all affected Rust files (`loans.rs`, `borrowers.rs`, `series.rs`, `home.rs`) and templates via `wc -l`. Project final LOC.
  - [ ] Confirm `cargo test no_inline_markup_in_templates` baseline passes BEFORE any edit.

- [ ] **Task 2 — Create the `status_message` macro (AC: 1, 8, 11)**
  - [ ] Create `templates/components/status_message.html` with the 7-positional-param macro per AC1.
  - [ ] Verify CSP-clean (no inline `style=`, `<style>`, `onclick=`).
  - [ ] Stable selectors: emit `data-status-message data-variant="{{ variant }}"` on the outer `<div>` (mirror of `data-modal-variant` from the modal macro).
  - [ ] Tailwind palette: warm stone neutral. NO red, NO amber.
  - [ ] Run `cargo build` to confirm Askama parses the new macro file without errors.
  - [ ] Run `cargo test no_inline_markup_in_templates` to confirm CSP audit green.

- [ ] **Task 3 — i18n keys (AC: 2, 8)**
  - [ ] Add a new top-level `empty:` block to `locales/en.yml` with **13 keys** for the 5 in-scope surfaces:
    - `loans_heading` + `loans_body` (2 keys, no CTA)
    - `borrowers_heading` + `borrowers_body` + `borrowers_cta` (3 keys; CTA URL is a template literal `#add-form`)
    - `series_heading` + `series_body` + `series_cta` (3 keys; CTA URL is a template literal `/series/new`)
    - `search_heading` + `search_body` + `search_cta` (3 keys; CTA URL is a template literal `/catalog/title/new?title={query}` interpolated server-side via Askama)
    - `filter_heading` + `filter_body` (2 keys, no CTA)
  - [ ] Add the same 13 keys to `locales/fr.yml` with FR copy, encouraging tone, gender-neutral (per the 9-14 review patch precedent — `cette personne` not `il`/`elle`).
  - [ ] **Drop the existing per-domain `empty_state` keys** (6 keys per locale): `loans.empty_state`, `borrower.empty_state`, `series.empty_state`, etc. (verify exact key names in each locale before dropping). Net change per locale: +13 new, −6 dropped = +7 keys.
  - [ ] Encouraging-tone copy guidelines:
    - EN: "No titles yet — start by scanning a barcode." NOT "No data" / "No results".
    - FR: "Aucun titre pour l'instant — commencez par scanner un code-barres."
    - Inviting verbs: "Start", "Add", "Try", "Scan" in EN; "Commencez", "Ajoutez", "Essayez", "Scannez" in FR.
  - [ ] **CTA URLs are NOT in i18n** — they are Askama template literals in the `{% call %}` invocation (e.g., `cta_url = "/series/new"`). Routing changes shouldn't require translator review.
  - [ ] Run `touch src/lib.rs && cargo build` to force rust-i18n proc-macro recompilation.
  - [ ] Run `cargo test all_t_keys_have_both_locales` to confirm parity.

- [ ] **Task 4 — Migrate `/loans` empty state (AC: 3, 7, 8)**
  - [ ] Edit `templates/pages/loans.html:77-78`: replace per AC3's before/after.
  - [ ] At top of `loans.html`: add `{% import "components/status_message.html" as status_message %}` (keep existing imports).
  - [ ] Edit `src/routes/loans.rs`: drop the `empty_state` field on `LoansTemplate`; add `empty_heading` + `empty_body` populated via `t!("empty.loans_heading")` + `t!("empty.loans_body")`.
  - [ ] Run `cargo build` to verify compilation.
  - [ ] Run any existing `tests/loans*.rs` integration suite to confirm no regression.

- [ ] **Task 5 — Migrate `/borrowers` empty state (AC: 4, 7, 8)**
  - [ ] Same shape as Task 4. CTA wired: `cta_label = empty_cta`, `cta_url = "#add-form"` (the existing fragment-anchor toggle on `borrowers.html:10-49` — `/borrowers/new` does NOT exist as a separate route), `cta_role_gate = "librarian"`.
  - [ ] Edit `src/routes/borrowers.rs` template struct: drop `empty_state`, add `empty_heading` + `empty_body` + `empty_cta`. The `cta_url` and `cta_role_gate` are template literals in the `{% call %}` invocation, not struct fields.

- [ ] **Task 6 — Migrate `/series` empty state (AC: 5, 7, 8)**
  - [ ] Same shape as Task 5. CTA `cta_url = "/series/new"` (route confirmed).
  - [ ] Edit `src/routes/series.rs` template struct.

- [ ] **Task 7 — Migrate `/` home empty states — 2 sub-cases (AC: 6, 7, 8)**
  - [ ] Edit `templates/pages/home.html:405-411` (verify exact range in Task 1): replace per AC6's two-sub-case shape (filter / search). Keep the outer `<div class="text-center py-12 ...">` for the SVG icon centering; the macro emits its own `text-center py-12` on its inner `<div>` (the doubled `py-12` is a one-off accepted in v1 — Task 1 visual check confirms or flags).
  - [ ] Edit `src/routes/home.rs` template struct: drop `no_results_text` and `no_results_create` (verify zero callers via grep); add 6 new fields: `search_empty_heading`, `search_empty_body`, `search_empty_cta`, `search_empty_cta_url`, `filter_empty_heading`, `filter_empty_body`.
  - [ ] Preserve the SVG icon outside the macro call.
  - [ ] Run `cargo build`, then any existing `tests/home*.rs` integration suite.

- [ ] **Task 8 — Unit tests (AC: 9)**
  - [ ] Create `templates/fragments/status_message_test_wrapper.html` per AC9.
  - [ ] Create `src/routes/status_message_tests.rs` with the 11 unit-test cases. Mirror the structure of `src/routes/modal_tests.rs`.
  - [ ] Add the module to `src/routes/mod.rs` (or wherever `modal_tests` is currently registered) — likely `mod status_message_tests;` next to `mod modal_tests;`.
  - [ ] Run `cargo test --lib status_message_tests` and confirm 11/11 pass.

- [ ] **Task 9 — E2E test (AC: 10)**
  - [ ] Create `tests/e2e/specs/journeys/empty-states.spec.ts` per AC10. Cover the 4 test scenarios (anonymous-no-CTA, librarian-with-CTA, search-no-results role-aware, i18n round-trip).
  - [ ] Run `cd tests/e2e && npx tsc --noEmit` to verify the spec edits don't break tsc.
  - [ ] Run `./scripts/e2e-reset.sh && cd tests/e2e && npx playwright test specs/journeys/empty-states.spec.ts` (single-spec run for fast feedback) and confirm all tests green.
  - [ ] Run the full E2E lane (`cd tests/e2e && npm test`) and confirm no other spec regressions.

- [ ] **Task 10 — Local gate + push + draft PR (AC: 13, 14)**
  - [ ] `SQLX_OFFLINE=true cargo check` — clean
  - [ ] `cargo clippy --all-targets -- -D warnings` — clean
  - [ ] `cargo test` (full lib + integration) — green
  - [ ] CI flake gate: `grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/` returns nothing
  - [ ] Run AC12 grep audit and document output in Dev Agent Record.
  - [ ] Push branch + open draft PR (Foundation Rule #15)
  - [ ] WAIT for CI green per Foundation Rule #18 before requesting review / merging.

## Dev Notes

### Why a NEW component vs. extending an existing one

`templates/components/feedback_entry.html` is a similar shape (heading + body + optional dismiss button) but is positioned as a TRANSIENT message (post-action feedback like "Loan created"). The empty-state pattern is STRUCTURAL (always rendered when the list is empty, no auto-dismiss). Mixing the two semantics into one component would dilute both. New component is the right call.

### Why role-gating server-side (Askama `{% if %}`) vs. client-side JS

Server-side keeps the rendered HTML clean (no role-conditional DOM that must be hidden by JS) and matches the existing precedent at `home.html:408` (`{% if role == "librarian" || role == "admin" %}`). Anonymous users never receive the CTA HTML, so there's no JS-disabled fallback gap.

The only edge case is the language-toggle / role-toggle in the same session: when a user logs in / out, the page re-renders with the new role (no client-side reactivity needed). This is the existing CSRF + auth flow.

### Why no icon in v1

UX-DR13 mentions an "illustrative icon if any". The existing `home.html:404` carries an SVG magnifying glass. Other surfaces don't have icons today. Adding a per-surface icon param would either:
- Require an `icon_svg: &str` param that callers pass as raw SVG text (couples the template to SVG markup) — UGLY.
- Require an `icon_name: &str` lookup against a sprite sheet (introduces new dependency on a sprite-sheet pattern not yet in the codebase) — SCOPE CREEP.

V1 ships without icon param; the existing search-empty SVG is preserved by emitting it OUTSIDE the macro (in the page template). A future polish story can add the icon param if usage demand emerges. YAGNI in v1.

### Why the home page splits into 2 sub-cases

`/` is overloaded: it's both the home dashboard AND the search/filter results page. The two empty-state contexts that ARE reachable today (per `home.rs:210-228`) are semantically distinct:
1. **Search-no-results** (`?q=...` returns nothing) — invite the user to add the searched title.
2. **Filter-no-results** (`?filter=overdue` returns nothing) — informational only; no CTA because the right action depends on the filter.

Folding these into one generic empty state would force generic copy ("No titles match"), which violates UX-DR13's "encouraging tone" goal. Two sub-cases is the right granularity.

A third sub-case ("First-launch empty: no titles in DB at all") was originally planned but DESCOPED post-validation: the `results = None` branch on `/` never reaches the empty-results template path. Adding it requires PRD-level redesign (SQL probe + new outer template branch) — deferred.

### Out-of-scope sweeps

The following surfaces have hand-rolled empty states but are NOT in the AC8 contract — explicitly OUT OF SCOPE for 9-15:
- `templates/fragments/admin_users_table.html:2-3`
- `templates/fragments/admin_ref_genres_list.html` and 3 sibling files
- `templates/fragments/admin_trash_panel.html:31-33`
- `templates/pages/contributor_detail.html:26` (silent empty — `{% if !titles.is_empty() %}`)
- `templates/pages/title_detail.html:66` (silent empty — `{% if !contributors.is_empty() %}`)

A follow-up "admin empty-state sweep" story can migrate the admin internals later. Adding them to 9-15 would balloon scope by ~5 more file edits without changing the user-facing surface contract.

### `/contributors` list page is missing

AC8 in the epics.md spec mentions `/contributors` as a surface. **Verified**: no such route exists. Only `/contributor/:id` (detail) and `/catalog/contributors/*` (admin assignment endpoints) exist. The AC8 entry is aspirational. Document as deferred GH `type:change-request` issue at story close: "add `/contributors` list page (with empty-state via `status_message`) — needs PRD discussion."

### File-LOC budget

All affected Rust files are well under the 2000 LOC ceiling. The largest is `home.rs` (~700 LOC) which gains ~10 fields on the page template struct + 5-10 LOC of `t!()` calls in the handler. Projected delta is small.

`status_message.html` macro itself is ~25 LOC — comparable to `modal.html` (~40 LOC after the 9-14 11-param extension).

### DEFERRED items inherited from prior stories (no action in 9-15)

- **CSRF rejection retargets to `#feedback-list`** (GH issue from 9-13 review). Cross-cutting; not relevant to 9-15 (no destructive actions added).
- **`body_html` `|safe` supply-chain risk** (GH #137 from 9-13). The new `status_message` macro inherits the same risk pattern (body_html rendered via `|safe`). Documented in AC1; sweep follow-up will cover both modal + status_message at once.
- **HIGH NotFound modal silent no-op + Conflict feedback hidden under modal** (GH issues from 9-14 review). Not relevant to 9-15.
- **Pre-existing E2E flakes** (`similar-titles.spec.ts:105`, `:170`; `home-search.spec.ts:222`) — data pollution under parallel mode on tests using fixed entity names. 9-15 does not regress these.

### NEW deferred items this story will file

5 GitHub Issues to file at story close (`type:code-review-finding` or `type:change-request` per content):
- **`/contributors` list page** (`type:change-request`) — no list route; original AC8 entry was aspirational. PRD discussion needed.
- **Home page first-launch empty state** (`type:change-request`) — `/` with no titles in DB never reaches the empty-results path; needs SQL probe + new template branch + PRD decision on which dashboard widgets render in first-launch.
- **`/borrower/:id` no loan history** (`type:change-request`) — section-status pattern (inline `<p>`) vs full-page-empty pattern (centered macro) — needs unified macro variant or accepted UX shift.
- **`/title/:id` no volumes** (`type:change-request`) — no current empty branch in the template; needs UX design decision.
- **Admin empty-state sweep** (`type:code-review-finding`) — `admin_users_table`, `admin_ref_*_list`, `admin_trash_panel` still hand-roll empty states.

### Project Structure Notes

- `templates/components/status_message.html` mirrors the location convention of `templates/components/modal.html`, `feedback_entry.html`, etc.
- `src/routes/status_message_tests.rs` mirrors `src/routes/modal_tests.rs` (9-10 sibling).
- `tests/e2e/specs/journeys/empty-states.spec.ts` is a NEW E2E spec file (no existing file fits the cross-page empty-state journey).

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-9.15] — story spec verbatim (8 ACs + EN/FR copy)
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#UX-DR13] — empty-state component spec, encouraging tone, role-aware CTA
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#UX-DR24] — calm stone neutral palette for empty/info variants
- [Source: _bmad-output/implementation-artifacts/9-10-modal-foundation-and-borrower-migration.md] — closest precedent for component creation pattern (macro + test wrapper + unit-test file)
- [Source: _bmad-output/implementation-artifacts/9-14-migrate-deactivate-user-to-modal.md] — recent precedent for macro extension (the 11ᵗʰ-param `version`); 9-15 doesn't extend an existing macro but the unit-test pattern is the same
- [Source: CLAUDE.md#Foundation-Rules] — Rules #1 (DRY), #11 (issue tracking), #12 (LOC ceiling), #13 (local testing), #15 (draft PR), #18 (CI gating)
- [Source: src/routes/modal_tests.rs] — the 222-LOC unit-test file pattern (macro test wrapper + variant cases) that `status_message_tests.rs` mirrors
- [Source: templates/components/modal.html] — macro signature pattern (10/11 params, conditional rendering with Askama `{% if %}`)
- [Source: templates/fragments/modal_test_wrapper.html] — test wrapper template that `status_message_test_wrapper.html` mirrors
- [Source: templates/pages/home.html:404-411] — canonical precedent for role-gated CTA empty state (the migration target for AC6 search sub-case)
- [Source: templates/pages/loans.html:77-78, borrowers.html:51-52, series_list.html:17-18] — the hand-rolled empty-state pattern being migrated (AC3-5)
- [Source: templates/pages/borrower_detail.html:48] — `active_loans.is_empty()` empty branch (AC7)
- [Source: src/routes/loans.rs, borrowers.rs, series.rs, home.rs] — page handler files where template struct fields are added/replaced

## Dev Agent Record

### Agent Model Used

claude-opus-4-7[1m]

### Debug Log References

(populated by dev agent)

### Completion Notes List

(populated by dev agent)

### File List

**Modified:**
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — story 9-15 status `ready-for-dev` → `in-progress` → `review`; `last_updated` bump.
- `locales/en.yml` — +13 new keys under top-level `empty:` block; −6 dropped per-domain `<domain>.empty_state` keys.
- `locales/fr.yml` — same shape, FR copy (gender-neutral).
- `src/routes/loans.rs` — `LoansTemplate.empty_state` field replaced with `empty_heading` + `empty_body`.
- `src/routes/borrowers.rs` — same pattern + `empty_cta` for `/borrowers` list (CTA URL `#add-form` is a template literal).
- `src/routes/series.rs` — same pattern + `empty_cta` (CTA URL `/series/new` is a template literal).
- `src/routes/home.rs` — drop `no_results_text` + `no_results_create`; add 6 new fields (3 search + 2 filter + cta_url for search). 2 sub-cases.
- `src/routes/mod.rs` — register `mod status_message_tests;`.
- `templates/pages/loans.html` — empty-state migration to macro.
- `templates/pages/borrowers.html` — empty-state migration to macro.
- `templates/pages/series_list.html` — empty-state migration to macro.
- `templates/pages/home.html:405-411` — empty-state migration to macro (2 sub-cases — search + filter).

**New:**
- `templates/components/status_message.html` — the 7-param `status_message` macro (~25 LOC).
- `templates/fragments/status_message_test_wrapper.html` — test wrapper (~5 LOC).
- `src/routes/status_message_tests.rs` — 11 unit-test cases (~250 LOC).
- `tests/e2e/specs/journeys/empty-states.spec.ts` — 4 E2E scenarios (~120 LOC).

**No change:**
- `templates/components/modal.html`, `static/js/modal.js`, `layouts/base.html`, `src/templates_audit.rs`.

### Change Log

| Date | Change |
| --- | --- |
| 2026-05-07 | Story created (backlog → ready-for-dev). First story in Epic 9 polish-finalize phase (post hx-confirm chain close 9-14). Scope: introduce a NEW `templates/components/status_message.html` macro (7 positional params: variant, heading, body_html, cta_label, cta_url, cta_role_gate, role) and migrate 5 surfaces from hand-rolled `<p class="text-center py-12 ...">` empties to the macro. 11 unit tests + 4 E2E scenarios. Role-gating server-side via Askama `{% if %}`. v1 ships without icon param (YAGNI). Body_html `|safe` supply-chain risk acknowledged (sweep follow-up via GH #137). Encouraging-tone copy in EN + FR, gender-neutral FR. |
| 2026-05-07 | Story validated; 14 improvements applied (6 critical + 5 enhancements + 3 optimizations). **Critical fixes**: (1) `/borrowers/new` does NOT exist — CTA rerouted to fragment `#add-form` (existing toggle in `borrowers.html:10-49`); (2) **first-launch home empty-state DESCOPED** — `home.rs:210-228` shows `results = None` when query+filter both empty, so the `{% if let Some(paginated) = results %}` empty branch is unreachable on `/`; PRD-level redesign (SQL probe + new template branch) deferred to a follow-up `type:change-request`; (3) **`/borrower/:id` no loan history DESCOPED** — actual markup at `borrower_detail.html:48-49` is inline `<p class="text-sm">` (not centered `py-12` with `empty_state`); section-status pattern vs full-page-empty pattern split; deferred to follow-up; (4) i18n key count corrected from "+14" to net +7 per locale (+13 new under `empty:` top-level block, −6 existing `<domain>.empty_state` keys dropped); (5) DRY motivation toned down (only 3 surfaces share the hand-rolled pattern, not 5+); (6) i18n convention choice (`empty:` top-level) explicitly justified vs existing per-domain convention. **Enhancements**: `/title/:id` no volumes confirmed definitively OUT OF SCOPE (no `volumes.is_empty()` branch exists); AC11 Test 1 "Re-decision" prose cleaned; AC10 case count standardized to 11; `/contributors` deferred GH issue label specified (`type:change-request`); top-level `empty:` block convention justified. **Optimizations**: trimmed reality-check duplication (~20 lines); dedupe of out-of-scope notes between reality-check and Dev Notes; AC count reduced from 15 to 14 (consolidation). **Final scope**: 5 in-scope surfaces (`/loans`, `/borrowers`, `/series`, `/?q=`, `/?filter=`); 5 deferred GH issues to file at story close. |
