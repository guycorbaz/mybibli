# Story 17.4: Typed codes warned like scanned ones

Status: ready-for-dev

GitHub issue: #643 (items 1 and 2 only; item 3 stays open). Covers: FR175. Epic 17; ships in v1.27.0.

## Story

As a librarian typing a barcode by hand,
I want the same warnings as when I scan it,
so that a mistyped code is caught wherever I enter it.

## Where a UPC is read today

| Entry point | Code | Separators | Irregularity warning (16-5) | 12-digit check digit (#384) |
|---|---|---|---|---|
| `/catalog/scan`, `upc` arm (no preference) | `routes/catalog.rs` → `media_type_selector_html` | – (digits only reach the arm) | yes | refused at creation, but the `Err` arm of `handle_scan_with_type` shows `error.title.creation_failed` (#643 item 2) |
| `/catalog/scan`, `upc` arm (preference cookie) | `routes/catalog.rs` | – | yes (D3 of 16-5) | same generic `Err` arm |
| Manual title creation | `routes/catalog_title_form.rs::create_title` → `services/title.rs::create_manual` (`non_empty_option`) | **kept as typed** | **no** | **no** |
| Title edit (CR #272) | `routes/titles.rs::update_title` (`identifier_without_separators`); field `pattern="[0-9]{0,14}"` | removed | **no** | **no** |

## Decisions

- **D1 — one check, reusing 16-5 (DRY).** `routes/catalog_code.rs::typed_upc_verdict(upc, stored, confirmed) -> TypedUpcVerdict { Keep, Confirm(CodeIrregularity), Refuse }` calls `code_irregularity`; no copy of its rules. Positive shapes (Rule #20): only all-digit codes that `code_irregularity` passes are kept silently. A code with a character other than a digit (possible in the manual form, which is `novalidate`) is a new irregularity `NotDigits` with its own warning; the scan never reaches it (only digits are classified `upc`).
- **D2 — a 12-digit UPC-A with a wrong check digit is refused in the forms too**, with the #384 message, and cannot be confirmed: the scan refuses it, so a form accepting it would be the one path where such a code enters. One helper, `TitleService::upc_checksum_refusal(code, loc)`, serves `create_from_code`, the scan's error arms and the forms.
- **D3 — confirm or correct.** An irregular typed code is not stored; the response swaps (`HX-Retarget` to a slot under the UPC field, `innerHTML`) the scan's warning (same `data-code-warning` paragraph, shared renderer `code_warning_html`) and a checkbox "Keep {code} as entered" (`name="upc_confirmed"`, `value=<code>`). Submitting again with it checked stores the code as entered (never rewritten). A corrected code no longer matches the confirmation and is checked again. The swap goes into the slot only, so focus stays where the librarian left it (Rule #21). No new JavaScript (CSP-clean).
- **D4 — an unchanged stored UPC saves as stored** on the edit form, without a check (same spirit as 17-3's unchanged ISBN): typed and stored compared without separators; the stored value is written back unchanged.
- **D5 — the manual form removes separators** like the edit form (`identifier_without_separators`, CR #272), so both forms check and store the same digits.
- **D6 — a code already on another title still warns** in the forms. 16-5's D5 (a known code does not warn again) exists because a rescan reaches its title; a form creates or edits a title and does not.
- **D7 — #643 item 2.** Both UPC error arms of the scan (preference path and `handle_scan_with_type`) show the localized #384 message for a 12-digit code with a wrong check digit, the generic `error.title.creation_failed` otherwise. `error.upc.invalid_checksum` is reworded as a terse field hint (Rule #9), in the shape of `error.isbn.invalid_checksum`, in all four locales.

## Acceptance Criteria

1. **AC1.** Manual creation and title edit: a typed UPC that `code_irregularity` flags (or that has a non-digit) shows the scan's warning under the field with a "keep as entered" confirmation; nothing is written until the librarian confirms or corrects; a confirmed code is stored as entered.
2. **AC2.** A 12-digit code with a wrong check digit is refused with the #384 message: on both forms, and on the scan after a media-type choice (no preference) or with a preference — never `error.title.creation_failed`.
3. **AC3.** An unchanged stored UPC on the edit form saves without a warning.
4. **AC4.** Unit (`typed_upc_verdict`, `upc_checksum_refusal`), DB integration (both forms: warn, confirm, correct, refuse, unchanged; scan-with-type message) and E2E (manual form warn → confirm, edit form warn → correct, wrong 12-digit after the choice). #643 stays open (item 3).

## Tasks

- [ ] `catalog_code.rs`: `NotDigits`, `TypedUpcVerdict`, `typed_upc_verdict`, `code_warning_html` (extracted from `media_type_selector_html`) + unit tests.
- [ ] `services/title.rs`: `upc_checksum_refusal`; `create_from_code` uses it.
- [ ] Scan error arms (D7).
- [ ] Manual form + edit form: `upc_confirmed`, slot, verdict handling (D3–D5).
- [ ] Locales en/fr/de/it: `scan.irregular_not_digits`, `title.form.upc_keep`, reworded `error.upc.invalid_checksum`.
- [ ] DB integration `tests/typed_upc_warning.rs`; E2E `typed-upc-warning.spec.ts` (new spec id).

## Dev Notes

- References: `_bmad-output/planning-artifacts/epics.md` § Story 17.4; issue #643; stories 16-5 (`code_irregularity`), 17-3 (unchanged-value rule), #384 (`validate_upc12_checksum`).
- No SQL change; no lock.
- `cargo fmt` is not run; write in the neighbouring style.

## Dev Agent Record

### Agent Model Used

### Completion Notes List

### File List
