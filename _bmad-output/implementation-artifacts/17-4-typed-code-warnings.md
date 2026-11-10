# Story 17.4: Typed codes warned like scanned ones

Status: review

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

- **D1 — one check, reusing 16-5 (DRY).** `routes/catalog_code.rs::typed_upc_verdict(upc, confirmed) -> TypedUpcVerdict { Keep, Confirm(CodeIrregularity), Refuse(UpcRefusal) }` calls `code_irregularity`; no copy of its rules. `check_typed_upc` wraps it for both forms (separators, the unchanged stored value of D4, the response). Positive shapes (Rule #20): only all-digit codes that `code_irregularity` passes are kept silently. Two irregularities only a typed code can have: `NotDigits` (a character other than a digit; the manual form is `novalidate`) and `Isbn10` (a valid ISBN-10 typed in the UPC field: the scan reads it as an ISBN, so `code_irregularity`'s "10 digits, wrong check digit" would be false — review round 1).
- **D2 — refusals.** A 12-digit UPC-A with a wrong check digit is refused in the forms too, with the #384 message, and cannot be confirmed: the scan refuses it, so a form accepting it would be the one path where such a code enters. One helper, `TitleService::upc_checksum_refusal(code, loc)`, serves `create_from_code`, the scan's error arms and the forms. A code longer than 13 characters is refused with its own hint (`error.upc.too_long`): `titles.upc` is `VARCHAR(13)` (review round 1; a 14-digit GTIN cannot be stored either way).
- **D3 — confirm or correct.** An irregular typed code is not stored; the response swaps (`HX-Retarget` to a slot under the UPC field, `innerHTML`) the scan's warning (same `data-code-warning` paragraph, shared renderer `code_warning_html`) and a checkbox "Keep {code} as entered" (`name="upc_confirmed"`, `value=<code>`). Submitting again with it checked stores the code as entered (never rewritten). A corrected code no longer matches the confirmation and is checked again. The swap goes into the slot only, so focus stays where the librarian left it (Rule #21). Changing the UPC field clears its slot (`static/js/title-form.js`, delegated `input` listener, CSP-clean), so a warning never stays next to a code it was not shown for (review round 1).
- **D4 — an unchanged stored UPC saves as stored** on the edit form, without a check (same spirit as 17-3's unchanged ISBN): typed and stored compared without separators; the stored value is written back unchanged.
- **D5 — the manual form removes separators** like the edit form (`identifier_without_separators`, CR #272), so both forms check and store the same digits. Both fields take 20 characters (room for separators); the edit field's `pattern="[0-9]{0,14}"` is dropped — the server now checks, and the pattern would block the whole edit form of a title holding a confirmed non-digit code (review round 1).
- **D6 — a code already on another title still warns** in the forms. 16-5's D5 (a known code does not warn again) exists because a rescan reaches its title; a form creates or edits a title and does not.
- **D7 — #643 item 2.** Both UPC error arms of the scan (preference path and `handle_scan_with_type`) show the localized #384 message for a 12-digit code with a wrong check digit, the generic `error.title.creation_failed` otherwise. `error.upc.invalid_checksum` is reworded as a terse field hint (Rule #9), in the shape of `error.isbn.invalid_checksum`, in all four locales.

## Acceptance Criteria

1. **AC1.** Manual creation and title edit: a typed UPC that `code_irregularity` flags (or that has a non-digit) shows the scan's warning under the field with a "keep as entered" confirmation; nothing is written until the librarian confirms or corrects; a confirmed code is stored as entered.
2. **AC2.** A 12-digit code with a wrong check digit is refused with the #384 message: on both forms, and on the scan after a media-type choice (no preference) or with a preference — never `error.title.creation_failed`.
3. **AC3.** An unchanged stored UPC on the edit form saves without a warning.
4. **AC4.** Unit (`typed_upc_verdict`, `upc_checksum_refusal`), DB integration (both forms: warn, confirm, correct, refuse, unchanged; scan-with-type message) and E2E (manual form warn → confirm, edit form warn → correct, wrong 12-digit after the choice). #643 stays open (item 3).

## Tasks

- [x] `catalog_code.rs`: `NotDigits`, `TypedUpcVerdict`, `typed_upc_verdict`, `code_warning_html` (extracted from `media_type_selector_html`) + unit tests.
- [x] `services/title.rs`: `upc_checksum_refusal`; `create_from_code` uses it.
- [x] Scan error arms (D7).
- [x] Manual form + edit form: `upc_confirmed`, slot, verdict handling (D3–D5).
- [x] Locales en/fr/de/it: `scan.irregular_not_digits`, `title.form.upc_keep`, reworded `error.upc.invalid_checksum`.
- [x] DB integration `tests/typed_upc_warning.rs`; E2E `typed-upc-warning.spec.ts` (new spec id).

## Dev Notes

- References: `_bmad-output/planning-artifacts/epics.md` § Story 17.4; issue #643; stories 16-5 (`code_irregularity`), 17-3 (unchanged-value rule), #384 (`validate_upc12_checksum`).
- No SQL change; no lock.
- `cargo fmt` is not run; write in the neighbouring style.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5

### Completion Notes List

- The scan's warnings were split into a cause and an action sentence (`scan.irregular_action_scan` / `irregular_action_form`), so the form shows the same cause with the action that fits it ("correct it, or tick the box below").
- The edit form's post-save display (`metadata_display_html`) does not show the UPC (existing, unrelated); the E2E reloads the page to read it.

### Review

- **Round 1** (Blind Hunter, Edge Case Hunter, Acceptance Auditor). Medium, fixed: a 14-digit code could be confirmed although `titles.upc` is `VARCHAR(13)` — refused with `error.upc.too_long` (D2); a non-digit code kept through the creation form made the edit form unsubmittable (its `pattern="[0-9]{0,14}"`) — pattern dropped, the server checks (D5); a valid ISBN-10 typed as UPC got the false "wrong check digit for an ISBN-10" warning — new `Isbn10` irregularity (D1); a warning stayed next to a changed code — the slot is cleared when the field changes (D3); `upc_checksum_refusal` had no unit test — added. Low, fixed: doc comment moved back onto `validate_upc12_checksum`; both fields take 20 characters (separators); DB tests for the edit-form correction, the preference scan path, the too-long refusal and the ISBN-10 warning; the E2E's title lookup matches its exact name and asserts `UPC: <code>`; Italian `upc_keep` wording. Low, deferred to #672: non-HTMX creation gets a bare fragment, the service's #384 text uses the global locale (only logged), the edit form's post-save display omits the UPC (pre-existing), `upc_confirmed` accepted on a first submit (by design).

### File List

- `src/routes/catalog_code.rs` — `NotDigits`, `form_message`, `code_warning_html`, `TypedUpcVerdict`, `typed_upc_verdict`, `TypedUpcOutcome`, `check_typed_upc`, `typed_upc_warning_html` + unit tests.
- `src/services/title.rs` — `upc_checksum_refusal`; `TitleForm.upc_confirmed`.
- `src/routes/catalog.rs` — shared warning renderer; `upc_creation_error_message` in both UPC error arms (D7).
- `src/routes/catalog_title_form.rs`, `src/routes/titles.rs` — the UPC check on both forms.
- `templates/components/title_form.html`, `templates/fragments/title_edit_form.html` — warning slots, field constraints.
- `static/js/title-form.js` — a changed UPC field clears its warning slot.
- `locales/{en,fr,de,it}.yml` — split warnings, `irregular_not_digits`, `title.form.upc_keep`, reworded `error.upc.invalid_checksum`.
- `tests/typed_upc_warning.rs` (new) — DB integration.
- `tests/e2e/specs/journeys/typed-upc-warning.spec.ts` (new, title names `TU`).
