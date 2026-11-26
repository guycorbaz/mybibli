# Story 20.6: Epic 20 smoke test (Foundation Rule #7)

Status: ready-for-dev

No GitHub issue. Covers: Foundation Rule #7. Epic 20; ships in v1.29.0 (with Epic 19). Sprint-status key: `20-6-epic-20-smoke-test`.

## Story

As the project,
I want one end-to-end journey from a blank browser over what the epic built,
so that the epic is proven done.

## Acceptance Criteria

1. **AC1 — blank browser.** The test starts with `context.clearCookies()` and a real login through `loginAs(page, "librarian")` (`tests/e2e/helpers/auth.ts`); no `DEV_SESSION_COOKIE`, no injected cookie.
2. **AC2 — the series and the wish list (20-2, 20-1).** The spec creates a closed series of total 4 and one title of its own (scanned on `/catalog`, assigned at position 1). From the series grid, gap 3 is sent to the wish list ("Add to wish list"); the form opens in wish list mode with the position and the pre-filled title; after Save the series page shows the success entry with its link in `#series-feedback`, and gap 3 shows the expected tome. The entry is found on `/wishlist` (by the link's `href`), then removed through "Remove" (not "Bought") and its modal; the browser is back on `/wishlist` and the entry is gone. The series page still shows the expected tome at position 3 (removing the entry does not touch the tome).
3. **AC3 — the total rule (20-3).** On the series edit form, the total lowered to 2 is refused next to the total field (`#series-total-error`, naming position 3), the typed value kept; reopening the form shows the stored total 4.
4. **AC4 — the location description (20-4).** The spec creates a root location and a child of it; the root is given a description on its edit form, and `/location/{id}` shows it in `#location-description`.
5. **AC5 — the tree by keyboard (20-5).** On `/locations`, the root's disclosure button is reached with the keyboard (Shift+Tab from the root's link), has the accessible name "Locations inside {name}" (EN / FR), and Enter collapses it (`aria-expanded="false"`, child list hidden), Enter again expands it (`"true"`, child visible); focus stays on the button.
6. **AC6 — hygiene.** Assertions only on the spec's own rows (Rule #23); no `waitForTimeout`; i18n-aware matchers (EN | FR at least); `npx tsc --noEmit` green; green under `CI=true npx playwright test --workers=2`. If this test fails, the epic is not done. File `tests/e2e/specs/journeys/epic20-smoke.spec.ts`, specIsbn id `"E0"`.

## Decisions

- **D1 — Librarian, not Admin.** Every action of the journey is Librarian+ (series, expected tomes, wish list, location create/edit; only a location *delete* is Admin, and the journey deletes nothing). The epic is daily use: the smoke logs in as a librarian.
- **D2 — spec id `"E0"`.** Epic smokes use `E3`…`E8` (epic 13…18); `E9` is left for 19-10. `"E0"` is used by no spec (grep of every 2-character id under `tests/e2e/`); its ISBN prefix `9786948…` is shared by no other id ('E' is the only printable ASCII character with code ≡ 69 mod 100, '0' the only one ≡ 48). No V-code is created, so no `VCODE_BLOCKS` entry.
- **D3 — ISBN derived from the run and the slot.** `specIsbn("E0", ((RUN * 4 + vcodeSlot()) % 9000) * 10 + 1)` with `RUN = Math.floor(Date.now() / 1000) % 20000`, as `series-total-tomes.spec.ts` (20-3) does: a retry or a re-run on a non-reset stack never scans an ISBN it already catalogued (`titleIdFromSkeleton` needs a fresh skeleton).
- **D4 — names run-stamped.** Series `E0-Series-${RUN}-${vcodeSlot()}`, locations `E0-Root-…` / `E0-Child-…`, so the wish list dedup (title + note, 20-2) never meets an earlier run's entry and the tree helpers' name lookups match one row.
- **D5 — L-codes found free.** Location labels are only 4 digits; a fixed code needs a stack reset between runs. The spec takes two consecutive codes in bucket 9, codes L9800–L9997, that `/locations` does not list yet — the `freeLcodePair` approach of `epic13-smoke.spec.ts` (bucket 8 there). Bucket 9 below 800 belongs to `location-description.spec.ts` (`100 + slot * 10 + n`); `L9998` / `L9999` are taken by `highest-label-codes` / `shelving` and never drawn. DRY (Rule #1): the helper moves to `tests/e2e/helpers/locations.ts` as `freeLcodePair(page, bucket, minSeq, maxSeq)`; `epic13-smoke.spec.ts` calls it with `(8, 0, 998)` — same behaviour (its seq was `floor(random * 499) * 2`, i.e. even starts 0–996); the smoke calls `(9, 800, 998)` (starts 800–996, codes L9800–L9997).
- **D6 — the pre-filled title is read, not rebuilt.** The wish list title is read from `#expected-tome-title` (`inputValue()`) after the form opens, and then used to find the entry and the Remove button; the spec asserts only that it contains the series name and "3", so it does not depend on the locale's `wishlist_title_prefill` wording.
- **D7 — the entry is found by its id.** The success link's `href` (`/wishlist/{id}`) identifies the entry on `/wishlist` (`table a[href="…"]`, the desktop table: the mobile cards are `md:hidden` at the default 1280 px viewport and `getByRole` ignores them). The Remove button is matched by its accessible name `Remove: “{title}”` / `Retirer : « {title} »` with the title regex-escaped (it contains a comma and the run stamp).
- **D8 — keyboard reach without walking the whole tree.** `/locations` lists every spec's locations; tabbing from the top of the page would depend on them. The spec focuses the root's own link (`#tree-node-{pid} > div > a[href="/location/{pid}"]`) and presses Shift+Tab: the disclosure button is the element right before it in the row (20-5 Tab order: button → link → add → edit [→ delete]). Enter twice, not Enter + Space: the epic AC says "with Enter"; Space is covered by `locations.spec.ts`.
- **D9 — no Trash restore.** The 20-3 restore line is covered by `series-total-tomes.spec.ts` and needs an Admin; the epic AC chain does not include it. The smoke stays on the epic's core journey.
- **D10 — no visible change.** The story adds one spec and moves one test helper; no template, Rust, locale or JS file changes.

## Dependency

**20-3 must be merged first** (or the spec developed on the 20-3 branch under the Rule #18 local-work exception and rebased with `--onto main` after its merge). AC3 needs `#series-total-error` and `#series-form-submit`, which exist only on `story/20-3-expected-tomes-consistency` (PR #747) — `main` has neither. Nothing is pushed before 20-3 is green and merged.

## Tasks / Subtasks

- [ ] **T1 — helper (D5).** `tests/e2e/helpers/locations.ts`: `export async function freeLcodePair(page, bucket, minSeq, maxSeq): Promise<[string, string]>` (reads `/locations`, draws `seq = minSeq + Math.floor(Math.random() * Math.floor((maxSeq - minSeq) / 2)) * 2`, returns `[specLcode(bucket, seq), specLcode(bucket, seq + 1)]` when neither appears in the page content, 50 attempts, then throws). `epic13-smoke.spec.ts` imports it (`freeLcodePair(page, 8, 0, 998)`) and drops its local copy.
- [ ] **T2 — `tests/e2e/specs/journeys/epic20-smoke.spec.ts` (AC1–AC6).** Header comment: story, journey, data (as the other smokes). One `test.describe("Epic 20 smoke — wish list, series and locations")`, one test, `test.setTimeout(120000)`:
  1. `context.clearCookies()`; `loginAs(page, "librarian")`.
  2. Series: `/series/new`, `#series-name`, `#series-type` = `closed`, `#series-total` = 4, `#series-form-submit`; `waitForURL(/\/series\/\d+$/)`; keep `seriesPath`.
  3. Title: `/catalog`, `simulateScan(page, "#scan-field", isbn)`, `titleIdFromSkeleton(page)`; `/title/{id}`, `#assign-series` `selectOption({ label: seriesName })`, `#assign-position` = 1, `#assign-series-submit`, `waitForURL(/\/title\/\d+/)`. On `seriesPath`: the owned cell `a[role="gridcell"][href="/title/{id}"]` is visible.
  4. Gap → wish list: `[data-gap-wishlist="3"]` has the accessible name `/Add to wish list: tome 3|Ajouter à la wish list : tome 3/i`; click; `waitForURL(/\/expected-tomes\/new\?position=3&wishlist=1$/)`; `#expected-tome-position` = "3"; read `#expected-tome-title` (D6); `#expected-tome-wishlist-help` visible; `#expected-tome-save` click.
  5. Back on the series page: `waitForURL(url => url.pathname === seriesPath && url.searchParams.has("wishlist_item"))`; `#series-feedback` contains `/added to your wish list|ajouté à votre wish list/i`; its link `/open the wish list entry|ouvrir l'entrée de la wish list/i` has an `href` matching `^/wishlist/\d+$`; `[data-expected-tome][data-position="3"]` visible with the title; `[data-gap-wishlist="3"]` count 0.
  6. `/wishlist`: `table a[href="{href}"]` contains the title. Remove button (D7) click; `#modal-slot dialog[open]` visible, contains the title, not `/Trash|Corbeille/i`; `expectHtmxProcessed(dialog.locator("[data-modal-confirm]"))`; Confirm; `waitForURL(/\/wishlist(\?.*)?$/)`; `table a[href="{href}"]` count 0.
  7. `seriesPath`: `[data-expected-tome][data-position="3"]` still visible.
  8. `${seriesPath}/edit`: `#series-total` = 2, `#series-form-submit`; `#series-total-error` contains `/Total must be at least 3|Le total doit être d’au moins 3/`; `#series-total` has value "2" and `aria-invalid="true"`. Reopen `${seriesPath}/edit`: `#series-total` = "4".
  9. Locations: `const [rootCode, childCode] = await freeLcodePair(page, 9, 800, 998)`; `createLocation(page, rootName, rootCode)`; `createChildLocation(page, rootName, childName, childCode)`; `/scan?code={rootCode}` → `/location/{pid}` (read `pid` from the URL); `#location-description` count 0.
  10. `/locations/{pid}/edit`: `#edit-description` fill a two-line text with the run stamp; `#edit-location-submit`; `toHaveURL(/\/locations\/?$/)`. `/location/{pid}`: `#location-description` visible, contains both lines.
  11. `/locations`: toggle `#tree-toggle-{pid}` has the accessible name `^(Locations inside|Emplacements dans) {rootName}$`, `aria-expanded="true"`, `#tree-children-{pid}` visible. Focus the root's link (D8), `page.keyboard.press("Shift+Tab")` → toggle focused; `Enter` → `aria-expanded="false"`, `#tree-children-{pid}` hidden, toggle focused; `Enter` → `"true"`, the child row (`#tree-children-{pid}` filtered by `childName`) visible.
- [ ] **T3 — gates.** `cd tests/e2e && npx tsc --noEmit`; the `waitForTimeout` grep gate; `tests/e2e/vcode-literals.sh` (no V-code literal); the spec alone against a fresh stack (`./scripts/e2e-reset.sh` under `flock … mem-guard`), then `epic13-smoke.spec.ts` alone (helper move); then the full suite once, `CI=true npx playwright test --workers=2` under `flock -w 10800 /tmp/station-heavy.lock mem-guard`. No Rust change: no `cargo` gate beyond what 20-3 ran.

## Dev Notes

### Files

| File | Change |
|---|---|
| `tests/e2e/specs/journeys/epic20-smoke.spec.ts` | new |
| `tests/e2e/helpers/locations.ts` | `freeLcodePair` added (moved from `epic13-smoke.spec.ts`) |
| `tests/e2e/specs/journeys/epic13-smoke.spec.ts` | uses the helper; local copy removed |
| this file, `sprint-status.yaml` | status line + `last_updated` |

### Selectors and copy the spec relies on

| Selector / copy | Where it is rendered |
|---|---|
| `#series-name`, `#series-type`, `#series-total` | `templates/pages/series_form.html` |
| `#series-form-submit`, `#series-total-error` | `series_form.html` / `src/routes/series.rs` **on the 20-3 branch** |
| `series.total_below_expected` "Total must be at least %{position}: …" / "Le total doit être d’au moins %{position} : …" | `locales/{en,fr}.yml` on the 20-3 branch |
| `#scan-field` | `templates/components/scan_field.html` |
| `#assign-series`, `#assign-position`, `#assign-series-submit` | `templates/pages/title_detail.html` |
| owned cell `a[role="gridcell"][href="/title/{id}"]`; `[data-gap-wishlist]`, `[data-expected-tome][data-position]`, `[data-expected-tome-title]` | `templates/components/series_gap_grid.html` |
| `#expected-tome-position`, `#expected-tome-title`, `#expected-tome-wishlist-help`, `#expected-tome-save` | `templates/pages/expected_tome_form.html` |
| `#series-feedback` | `templates/pages/series_detail.html` |
| Remove button name `wishlist.action_remove_named` "Remove: “%{title}”" / "Retirer : « %{title} »" | `templates/pages/wishlist.html` (aria-label built in `src/routes/wishlist.rs::action_labels`) |
| `#modal-slot dialog[open]`, `[data-modal-confirm]` | `layouts/base.html`, `templates/components/modal.html` |
| `#edit-description`, `#edit-location-submit` | `templates/pages/location_edit.html` |
| `#location-description` | `templates/pages/location_detail.html` |
| `#tree-node-{id}`, `#tree-toggle-{id}`, `#tree-children-{id}`, `location.tree.toggle_named` "Locations inside %{name}" / "Emplacements dans %{name}" | `src/routes/locations.rs::render_node_at_depth` |

### Patterns to reuse

- `createClosedSeries` / `openGapWishlist` shapes from `series-gap-wishlist.spec.ts`; the Remove modal flow from `wishlist-remove.spec.ts`; the description round trip from `location-description.spec.ts`; the toggle checks from the 20-5 block of `locations.spec.ts`; the scan + assign steps from `series-total-tomes.spec.ts` (20-3). Copy the selectors, not the specs' data (own ids and names only).
- Smoke header and structure: `epic18-smoke.spec.ts`.
- File order no longer matters (#528, story 19-2): the spec creates series and locations but asserts only on its own rows.
- `expectHtmxProcessed` before clicking Confirm in a modal HTMX just swapped in (story 19-2, #685).

### Risks

- The metadata fetch renames the title in the background ("Test Title <isbn>", mock catch-all); the spec never asserts the title's name, only its id and position.
- The Remove modal and the scan-guard: Confirm is clicked with the mouse, not with a scanner-paced Enter.
- `localStorage` collapse state is per browser context: the test's fresh context starts expanded; it ends expanded.

## Open questions

None. No visible copy or behaviour change is needed.

## Dev Agent Record

### Agent Model Used

### Completion Notes List

### File List
