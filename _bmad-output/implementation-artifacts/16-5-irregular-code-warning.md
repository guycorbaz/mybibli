# Story 16.5: Warn on an irregular scanned code

Status: done

GitHub issue: #638 (type:change-request). Covers: FR170, UX-DR42. Epic 16; ships in v1.26.0.

## Story

As a librarian,
I want to be warned when a scanned or typed code looks wrong,
so that a lost `X` or a typing error does not quietly become a title that never resolves.

## Decisions

- **Project lead, 2026-11-06:** warn and let the librarian confirm; never refuse, never rewrite.
- **D1 — positive shapes (Rule #20).** Only an EAN-8 or EAN-13 with a valid check digit, and any 12-digit code, pass silently. Everything else reaching the `upc` arm is named: 9 digits (with the lost-`X` hint), 10 digits (failed ISBN-10), another length, a wrong check digit.
- **D2 — 12 digits stay with #384.** A UPC-A with a wrong check digit is already refused at creation, so a warning offering to keep it would be false.
- **D3 — the preference cookie does not skip the warning.** From the second UPC scan, the preference created the title with no question; that is how such a code could enter silently. An irregular code now always goes through the choice.
- **D5 — a known code does not warn again.** Rescanning a code the librarian confirmed (a second copy) reaches its title directly.
- **D6 — the scan flow only.** The UPC field of the title forms is a deliberate hand edit and stays unchecked; #643 tracks it.
- **D4 — stored codes untouched.** A confirmed code is kept as entered, through the existing `scan-with-type` route.

## Acceptance Criteria

1. **AC1.** An irregular code shows the media-type choice with a warning naming what is wrong (Rule #9); nothing is created until the choice.
2. **AC2.** A valid barcode behaves as before, preference included.
3. **AC3.** Confirmed, the code is kept as entered.
4. **AC4.** Unit (`catalog_code`), DB integration (`tests/scan_irregular_code.rs`), E2E (`irregular-code-warning.spec.ts`: confirm, correct, focus stays in the scan field, Rule #21).
