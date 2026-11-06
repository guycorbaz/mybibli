# Story 16.2: Amounts on the volume page

Status: done

GitHub issue: #630, item 7 (type:code-review-finding). Covers: FR167. Epic 16; ships in v1.26.0.

## Story

As a librarian,
I want to see what a volume cost and what it is worth on its page,
so that I can check an accepted value without opening the edit form.

## Decisions

- **D1.** Librarian+ only, enforced by not building the block (`amounts: None`), as the labels region: the edit form that holds these amounts is Librarian+, and a purchase price is not public data.
- **D2.** Existing labels "Prix d'achat" and "Valeur actuelle estimée" (project lead, 2026-11-06); one new key, `volume.current_value_dated`, in all four locales.
- **D3.** Formatting reuses `stats::format_amount`, so the decimal comma stays French-only like `/stats/value`; #630 item 2 covers de/it for both surfaces.
- **D4.** A date without a value is not shown. A row with no amount reads "—", as the Condition and Edition rows.

## Acceptance Criteria

1. **AC1.** The page shows both amounts and the value's date in the instance zone; an unset amount reads as such.
2. **AC2.** Anonymous visitors see neither row.
3. **AC3.** Unit (`volume_amounts_*`, render tests in `volume_detail_tests.rs`) and E2E (`volume-page-amounts.spec.ts`, spec ID VA, axe, blank browser for the anonymous case).
