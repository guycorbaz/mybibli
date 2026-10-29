# Story 11.21: Access to the proposals page

Status: ready-for-dev

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR143, UX-DR32, UX-DR33. Also closes #545 (phone-width overflow of the bar and of the `/proposals` list). Epic 11 — twenty-first story. Not here: CR #547 (left sidebar — explicitly out of scope until epic 11 is closed, decided by the project lead on 2026-10-29).

## Story

As a librarian,
I want to see at once that proposals are waiting,
so that they do not pile up.

## History of this story — the UX-DR32 stop clause fired

A first attempt (2026-10-29, morning) measured the bar with an Admin's full link set before writing any code: at the CR #443 `lg` switch (1024 px) the bar **already** overflowed by 4 px in French and 8 px in German, with 0 px of slack in Italian — before the Proposals entry. Per the epic AC it stopped and reported. Measured minimum clean widths with the entry simulated (label + a two-digit badge): **fr 1264 px, de 1168, it 1216, en 1116**. The project lead decided on 2026-10-29 (logged as D-11-21-1 and D-11-21-2, both [PRODUCT]):

1. The desktop/hamburger switch moves from `lg` to `xl` (1280 px) for **every role** — same kind of fix as CR #443; it supersedes story 11-4's "breakpoints unchanged". The bar is still **not compacted** (no shrinking of labels, font or spacing). The longer-term answer is CR #547 (left sidebar), out of scope until epic 11 closes.
2. A pending proposal with no line left to apply (`has_lines_to_apply` false) still counts in the badge.

The AC below are the epic's AC amended accordingly (AC4 replaces "stop at `lg`").

## Existing-code reality check (code map, 2026-10-29, `main` fbf7152)

| Place | Today | Consequence |
|---|---|---|
| `templates/components/nav_bar.html` | desktop strip `hidden lg:flex`, hamburger `lg:hidden`, panel `#mobile-nav` `hidden lg:hidden`; the right-hand group (`ml-auto`: language select, theme toggle, login/logout form, hamburger) is visible at every width | the three `lg` switches move to `xl`; the bar's logout form gets `hidden xl:contents` (the panel already carries its own logout, #150) |
| `src/utils.rs::BaseContextFields` / `base_context()` (50 call sites) / `test_base_context()` | pool-free, built from `&Session` + locale | the badge count must reach the template without touching 50 call sites → it travels on `Session` |
| `src/middleware/auth.rs::session_resolve_middleware` | resolves the session (DB) on every non-static request, inserts it into request extensions | computes the pending count for Librarian+ on page requests and stores it on the session |
| `proposals` table | `status ENUM('pending','accepted','rejected','stale')`, `deleted_at`, `INDEX idx_proposals_status (status, deleted_at)` | `SELECT COUNT(*) … WHERE status = 'pending' AND deleted_at IS NULL` is index-only |
| `src/routes/home_indicators.rs` | `IndicatorTag { label, count, filter_name, is_active, clear_aria_label }`, `build_indicator_tags` (5 tags, each linking to `/?filter=<name>`), macro `components/filter_tag.html` | a sixth tag that links to `/proposals` → `IndicatorTag` gains an `href`; `home.rs` (2562 lines, already over Rule #12) gets one line only |
| `templates/pages/proposals.html` | table in an `overflow-x-auto` box; the page still scrolls horizontally at 360 px (#545: 641–663 px) | the list must fit 360 px without a page-level horizontal scroll |
| `src/routes/nav_sticky_tests.rs` | asserts `lg:flex` / `lg:hidden` (11-4 AC6) | updated to `xl` |
| E2E `navbar-hamburger.spec.ts` | locator `div.hidden.lg\\:flex`; TABLET 600, DESKTOP 1280 | locator → `xl`; DESKTOP 1280 stays desktop (`xl` = `min-width: 80rem`) |
| E2E `responsive-layouts.spec.ts` | 375 / 768 / 1280 px, 32 px tolerance, librarian | tightened (AC5) |
| Playwright default viewport | 1280 × 720 (no override in `playwright.config.ts`) | every other spec keeps the desktop strip |

## Acceptance Criteria

1. **AC1 — nav entry and badge (FR143, UX-DR32).** For a Librarian or Admin, a "Proposals" entry (fr *Propositions*, en *Proposals*, de *Vorschläge*, it *Proposte*) appears in the desktop strip (before Admin) and in the mobile panel, linking to `/proposals`, with `aria-current="page"` on `/proposals` and its detail pages. When proposals are pending it carries a badge with the count (text, not colour only; capped at `99+`); the link's `aria-label` states the label and the exact count ("Proposals, 3 pending"); at zero the badge and the aria-label are absent. Anonymous visitors see neither the entry nor the badge. The count = live proposals with `status = 'pending'`, **including** pending proposals with no line left to apply (D-11-21-2).
2. **AC2 — dashboard indicator (UX-DR33).** The home "What needs attention" section shows a "Pending proposals" pill with the count, following the story 9-7 indicator pattern (same macro, zero-count rule: absent at zero), linking to `/proposals` (not to a `/?filter=`). Librarian+ only.
3. **AC3 — freshness.** After a decision taken on `/proposals/{id}` (accept / reject / a proposal going stale), the badge and the indicator are up to date on the next navigation (computed per page request, no cache).
4. **AC4 — the bar is not compacted; the switch moves to `xl` (amended by D-11-21-1).** The desktop strip shows from `xl` (1280 px) for every role, the hamburger and the mobile panel below it; labels, font sizes, gaps and bar heights are unchanged. At 1280 px and above, an Admin's full link set (with the Proposals entry and a `99+` badge) fits on one line in all four locales: no horizontal overflow, no wrapped link. Below `xl`, the bar's own logout button is hidden (the panel's logout, #150, stays reachable); nothing becomes unreachable.
5. **AC5 — `responsive-layouts.spec.ts` tightened.** As an Admin, in all four locales, at 360 px, 1279 px, 1280 px and a wide width (1920 px): no horizontal page overflow beyond **1 px** (was 32) and no nav link wraps; at 1279 px the hamburger is shown and the strip hidden, at 1280 px the reverse.
6. **AC6 — #545.** `/proposals` (list) has no horizontal page scroll at 360 px; the bar does not overflow at 360 px. #545 is closed by this story's PR.
7. **AC7 — cheap count.** One indexed `COUNT(*)` per Librarian+ page request (not on HTMX fragment requests, static assets, `/health` or `/api/mcp`); anonymous requests run no query. A count failure degrades to "no badge" (logged), never to an error page.
8. **AC8 — i18n and CSP.** New labels in all four locale files (`locale_parity`); no inline style/script; `templates_audit` passes.
9. **AC9 — E2E.** With a pending proposal (filed through `helpers/proposals.ts`), the badge and the indicator appear; once it is decided (rejected), they disappear on the next navigation (or the count drops by one — the shared E2E DB may hold other pending proposals). The hamburger / sticky-nav / mobile-menu specs that depended on `lg` are updated.

## Tasks / Subtasks

- [ ] T1 — Count (AC1, AC3, AC7): `ProposalModel::count_pending(pool)`; `Session.pending_proposals: Option<u64>`; filled in `session_resolve_middleware` for Librarian+ on non-HTMX requests (and HTMX history-restore requests); pure predicate `wants_pending_count(role, headers)` unit-tested; `sqlx` dynamic query (no offline cache change) or macro + `cargo sqlx prepare`.
- [ ] T2 — Nav (AC1, AC4, AC6): `BaseContextFields` gains `nav_proposals`, `nav_proposals_badge` (`Option<String>`: "3" / "99+") and `nav_proposals_aria` (`Option<String>`); `nav_bar.html` entry in strip + panel; `lg` → `xl` for strip / hamburger / panel; logout `hidden xl:contents`; comments updated (CR #443 → 11-21).
- [ ] T3 — Indicator (AC2): `IndicatorTag.href`; `pending_proposals_tag(count, loc)` in `home_indicators.rs`; one line in `home.rs`; macro uses `tag.href`.
- [ ] T4 — `/proposals` list fits 360 px (AC6).
- [ ] T5 — Locales ×4 (AC8).
- [ ] T6 — Unit + router tests: badge text cap, aria, anonymous sees nothing, librarian/admin see the entry, count includes a pending proposal without lines to apply (DB test), indicator tag href, nav_sticky_tests `xl`.
- [ ] T7 — E2E: `proposals-access.spec.ts` (new spec id), `responsive-layouts.spec.ts` tightened, `navbar-hamburger.spec.ts` locator.
- [ ] T8 — Gates, decisions log, sprint-status.

## Dev Notes

- **Why the count rides on `Session`:** `base_context()` is pool-free and called from 50 handlers; a request-extension field read by the extractor-backed `Session` gives every full page the count with zero handler churn. `Session` literals (18, mostly tests) get `pending_proposals: None`.
- **Which requests count:** page renders only. HTMX fragment requests (`HX-Request: true`) never render the nav, except history-restore requests (`HX-History-Restore-Request: true`, htmx 2 `historyRestoreAsHxRequest`), which may — they count too.
- **Badge text:** `n` for 1..=99, `99+` above; aria-label carries the exact number. The first-attempt measurements leave French ~16 px at 1280 px with a two-digit badge; the E2E width checks use a `99+`-sized badge to prove the worst case fits.
- **D-11-21-2:** "pending" = `status = 'pending'`; whether lines remain to apply is not looked at.
- Error copy: none user-facing added (a count failure is silent, logged at `warn`).
- E2E pitfalls: spec id must be fresh; no `waitForTimeout`; i18n regexes; the E2E instance renders French; the shared DB may already hold pending proposals — assert relative counts or on the specific proposal's disappearance carefully; reject-then-refile is blocked library-wide 90 days (11-15) — use unique targets.

### Validation notes (create-story checklist)

See below, after validation.

### References

- `_bmad-output/planning-artifacts/epics.md` § Story 11.21, FR143, UX-DR32, UX-DR33.
- `_bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md` § 11-4 (D-11-4-*), § 11-16..11-20.
- CR #443 (switch `md` → `lg`), #545, CR #547.

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
