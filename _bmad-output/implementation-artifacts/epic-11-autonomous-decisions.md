# Epic 11 — autonomous decisions registry

Decisions taken by story agents running without user interaction. Entries are appended, never
rewritten.

### D-11-1-1 — Sprint-status: touch only the story line
**Context:** `bmad-create-story` would flip `epic-11` to `in-progress` and rewrite `last_updated`; 11-1, 11-2 and 11-3 run in parallel branches.
**Chosen:** change only the `11-1-isbn10-at-scan` line (Foundation Rule #16); leave `epic-11` and `last_updated` to the orchestrator.
**Rejected:** editing `epic-11` / `last_updated` from this branch — guaranteed merge conflicts with the sibling story PRs.
**Reversible:** yes — a one-line chore commit on `main`.

### D-11-1-2 — Canonicalise the scanned code once, right after classification
**Context:** `handle_scan` and `handle_home_scan` pass `code` to five downstream consumers (duplicate check, title creation, wishlist auto-link, skeleton feedback, provider chain).
**Chosen:** `CodeDetection` gains `normalized_code: Option<String>` (Some only for an ISBN-10 rewritten to ISBN-13); both handlers rebind `code` to it immediately after `detect_code_type`.
**Rejected:** converting at each consumer (five edits, easy to miss one); converting before `detect_code_type` (the contract asks `detect_code_type` itself to use the helper and classify the ISBN-10 as `isbn`).
**Reversible:** yes.

### D-11-1-3 — Share the EAN-13 check-digit computation
**Context:** the ISBN-10→ISBN-13 conversion needs the EAN-13 check digit; two copies already exist (`TitleService::validate_isbn13_checksum`, `catalog.rs::validate_issn_from_ean`).
**Chosen:** `utils::ean13_check_digit` used by the conversion and by both existing validators (mechanical refactor, existing unit tests guard behaviour).
**Rejected:** a third private copy (Rule #1).
**Reversible:** yes.

### D-11-1-4 — Client-side scan JS and other ISBN inputs unchanged
**Context:** `static/js/scan-field.js::detectPrefix` labels an ISBN-10 `upc`/`unknown` but never blocks its submission; the manual title form, wishlist "By ISBN" form and title-edit ISBN field accept ISBN-13 only.
**Chosen:** no JS change (the server is authoritative, `data-detected-prefix` is read nowhere); other ISBN inputs stay out of scope (11.10 reuses the helper for expected tomes). User-manual wording is left to the epic documentation story 11-24.
**Rejected:** a JS mod-11 copy (second source of truth); widening the story to every ISBN input.
**Reversible:** yes.

### D-11-1-5 — Search: bad-checksum ISBN-10 shape keeps today's behaviour
**Context:** `search::detect_code` accepts the ISBN-10 shape without checksum.
**Chosen:** a valid ISBN-10 becomes `CodeType::Isbn(<ISBN-13>)`; an ISBN-10-shaped query with a bad checksum stays `CodeType::Isbn(<raw>)` (exact lookup, then fulltext fallback) as before.
**Rejected:** turning a bad-checksum query into `Text` — a behaviour change nobody asked for; the fulltext fallback already covers it.
**Reversible:** yes.
