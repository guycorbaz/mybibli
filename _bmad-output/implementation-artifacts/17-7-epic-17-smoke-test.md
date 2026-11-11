# Story 17.7: Epic 17 smoke test (Foundation Rule #7)

Status: ready-for-dev

Covers: Foundation Rule #7. Epic 17; ships in v1.27.0.

## Story

As the project,
I want one end-to-end journey from a blank browser through the epic's core flow,
so that the epic is proven done.

## Acceptance Criteria

1. **AC1** — From a blank browser (no injected cookie), the test chains: `loginAs` → create a title by hand with a hyphenated ISBN (17-3) → on its page, add a volume with a lowercase V-code (17-1, 17-2) → scan the bare ISBN on `/catalog` and land on the same title (17-3) → find the title by its id (17-5) → see a default skill, inactive, in Admin → Skills (17-6).
2. **AC2** — No `waitForTimeout`; green under `CI=true npx playwright test --workers=2`; if this test fails, epic 17 is not done.

## Design (dev, 2026-11-11)

- One test, admin role (`loginAs(page, "admin")` from a cleared context): the admin is the only role that reaches Admin → Skills, and it holds every Librarian right the rest of the journey uses.
- Title by hand on `/catalog` (`#new-title-btn`, `#title-form-container form`), ISBN `specIsbn("E7", clock-based seq)` written `978-xx-xxx-xxxx-x` — manual creation refuses an ISBN another title holds, so a rerun on a database not reset must not draw a used one (same clock rule as `isbn-one-rule.spec.ts`). The title name carries the spec id and a timestamp.
- Manual creation returns no title id: the title page is reached the way a librarian does — the home search on the unique name, then the result link. The id is read from the URL and checked against `#title-id` (17-5).
- Volume: scanner burst of `v<digits>` into `#title-volume-vcode`; the volume appears in `#title-volumes-list` as `V<digits>`, `#title-volumes-count` reads `(1)`, focus stays in the field (Rule #21). V-code band V5000–V5499, random (no spec hardcodes or derives a code there; `home.spec.ts` and `loans.spec.ts` draw widely, so a collision is rare but possible).
- Bare ISBN scanned on `/catalog`: "already in your catalog" feedback and the context banner names the title — no duplicate. Its volume count reads 1 in the banner.
- Id search: `#<id>` typed in the home search lists that title alone; following the row opens `/title/<id>`.
- Default skill: `comics-covers` row in Admin → Skills, "Inactive", "provided by mybibli". Read only (the spec never activates, edits or deletes it — `default-skills.spec.ts` reads the same row).
- Assertions only on this spec's rows (Rule #23); the default skill is a seeded row every database has, asserted by name.

## Tasks

- [ ] `tests/e2e/specs/journeys/epic17-smoke.spec.ts`.

## Dev Agent Record

### File List

### Completion Notes
