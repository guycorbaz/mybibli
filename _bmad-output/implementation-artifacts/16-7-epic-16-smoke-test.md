# Story 16.7: Epic 16 smoke test

Status: done

Covers: Foundation Rule #7. Epic 16; ships in v1.26.0.

## Journey

Blank browser, `loginAs` → a code of no known shape shows the choice with its warning → confirmed → a volume scanned on that title → the agent uploads a cover file, checks the receipt and files `propose_cover` by token, then proposes a purchase price alone → the librarian accepts both → the title shows the cover, the volume page shows the purchase price and no value.

Spec: `tests/e2e/specs/journeys/epic16-smoke.spec.ts`. Green under `CI=true npx playwright test --workers=2`. If it fails, the epic is not done.
