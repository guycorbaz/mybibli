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

- **D1 — one check, reusing 16-5 (DRY).** `routes/catalog_code.rs::typed_upc_verdict(upc, confirmed) -> TypedUpcVerdict { Keep, Confirm(CodeIrregularity), Refuse(UpcRefusal) }` calls `code_irregularity`; no copy of its rules. `check_typed_upc` wraps it for both forms (separators, the unchanged stored value of D4, the response). Positive shapes (Rule #20): only all-digit codes that `code_irregularity` passes are kept silently. One irregularity only a typed code can have: `NotDigits` (a character other than a digit; the manual form is `novalidate`). A 10-character code of 9 digits and an `X` that fails the ISBN-10 check gets the scan's 10-digit warning (review round 2).
- **D2 — refusals.** A 12-digit UPC-A with a wrong check digit is refused in the forms too, with the #384 message, and cannot be confirmed: the scan refuses it, so a form accepting it would be the one path where such a code enters. One predicate, `TitleService::is_bad_upc_a`, serves `create_from_code`, the scan's error arms (`upc_checksum_refusal`) and the forms (`UpcRefusal::CheckDigit`). A code longer than 13 characters is refused with its own hint (`error.upc.too_long`): `titles.upc` is `VARCHAR(13)` (review round 1; a 14-digit GTIN cannot be stored either way). A valid ISBN-10 typed in the UPC field of a **printed** title (`MediaType::is_print`: book, BD, magazine, report — the media of the 13-3 boot conversion) is refused with a hint pointing to the ISBN field (`error.upc.isbn10`): it is the title's ISBN, the scan reads it as one, and kept as the UPC of a title without ISBN it would be moved there at the next start (`convert_isbn10_upc_titles`), breaking "kept as entered". On a CD or a DVD it is no ISBN: it gets the length warning ("10 digits") and can be confirmed (review rounds 1–3). The media type is the one the title will have once saved.
- **D3 — confirm or correct.** An irregular typed code is not stored; the response swaps (`HX-Retarget` to a slot under the UPC field, `innerHTML`) the scan's warning (same `data-code-warning` paragraph, shared renderer `code_warning_html`) and a checkbox "Keep {code} as entered" (`name="upc_confirmed"`, `value=<code>`). Submitting again with it checked stores the code as entered (never rewritten). A corrected code no longer matches the confirmation and is checked again. The swap goes into the slot only, so focus stays where the librarian left it (Rule #21). Changing the UPC field clears its slot (`static/js/title-form.js`, delegated `input` listener, CSP-clean), so a warning never stays next to a code it was not shown for (review round 1).
- **D4 — an unchanged stored UPC saves as stored** on the edit form, without a check (same spirit as 17-3's unchanged ISBN): typed and stored compared without separators; the stored value is written back unchanged.
- **D5 — the manual form removes separators** like the edit form (`identifier_without_separators`, CR #272), so both forms check and store the same digits. Both fields take 40 characters (room for separators; the server enforces 13); the edit field's `pattern="[0-9]{0,14}"` is dropped — the server now checks, and the pattern would block the whole edit form of a title holding a confirmed non-digit code (review round 1).
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

- **Round 2** (combined adversarial + edge pass on the round-1 fixes). Medium, fixed: a valid ISBN-10 confirmed in the UPC field would be rewritten by the 13-3 boot conversion — now refused towards the ISBN field (D2). Low, fixed: the #384 predicate shared (`is_bad_upc_a`, DRY); a 10-character code ending in `X` with a failed check gets the 10-digit warning instead of "not digits"; fields raised to 40 characters so the browser never cuts a paste before the server's length check; the ISBN-10 message/action mismatch disappears with the refusal.

- **Round 3** (combined pass on the round-2 fixes). Medium, fixed: the ISBN-10 refusal applied to every medium, while the boot conversion and the ISBN advice only hold for printed media — now print only, other media get the length warning (D2); DB tests for both and for the edit form's refusal. Low, fixed: an unchanged ISBN-10-shaped legacy UPC is covered by the unchanged-value test; the ISBN-10 shape test shared (`utils::is_isbn10_shaped`, DRY with `compact_isbn`); the unchanged-value comparison ignores the case of an `X`. Low, accepted: a failed ISBN-10 ending in `X` gets "This code has 10 digits", while it has 9 digits and an `X` (form only; the warning still names the right fix).

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
