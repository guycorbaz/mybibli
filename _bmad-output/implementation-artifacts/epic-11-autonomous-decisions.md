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

## Story 11-2 — Library of Congress through SRU (#495)

### D-11-2-1 — Sprint-status: the story line plus the line-2 header
**Context:** the orchestrator's brief for 11-2 allows the `# last_updated:` comment on line 2 in addition to the story line.
**Chosen:** change the `11-2-loc-via-sru` line and the line-2 comment only; `epic-11` and the YAML `last_updated:` key are left alone (Rule #16).
**Rejected:** touching `epic-11` from a story branch.
**Reversible:** yes.

### D-11-2-2 — Remove the JSON search outright, no fallback
**Context:** the issue offered "keep `?fo=json` as a secondary attempt that fails fast on `cf-mitigated`"; the epic AC says it "is no longer called".
**Chosen:** delete the JSON path, its parser, wire types, `LOC_API_BASE_URL`, the mock `/books/` handler and the compose variable.
**Rejected:** a fail-fast fallback — a guaranteed 403 per scan for as long as the Cloudflare policy holds, and dead code to maintain.
**Reversible:** yes (git history).

### D-11-2-3 — Fields mapped from MARC 21 beyond the AC minimum
**Context:** the AC names title, contributors, publisher and date; the JSON path also supplied language, description and a cover.
**Chosen:** also map subtitle (245$b), language (008/35–37), page count (300$a), description (520$a) and Dewey (082$a), so that the switch does not lose what the JSON gave (except the cover). The cover is left to the existing cover-only fallbacks (BnF cover service, Inventaire, Open Library by ISBN).
**Rejected:** the AC minimum only (a visible regression on language/description); a cover from 856$u (not an image link).
**Reversible:** yes.

### D-11-2-4 — Entity decoding + NFC in the LoC parser, via `unicode-normalization`
**Context:** LoC MARCXML encodes non-ASCII characters as numeric references, often decomposed (`e&#x301;`); `metadata::marc` never decodes entities.
**Chosen:** a `marc::decode_xml_text` helper (entities + NFC) applied to every value the LoC provider returns; direct dependency `unicode-normalization = "0.1"`, already in `Cargo.lock` through `sqlx-mysql` → `stringprep`, so no new crate is fetched. BnF and K10plus readers stay raw.
**Rejected:** no NFC (decomposed titles would not match a precomposed search query); decoding inside `extract_subfield` for every provider (changes BnF/K10plus output — out of scope).
**Reversible:** yes.

### D-11-2-5 — Per-provider timeout unchanged
**Context:** the default per-provider budget is 5 s; the SRU transport retry (2 s + 4 s sleeps) can exceed it now that SRU is primary.
**Chosen:** no default change; the chain records LoC as timed out and moves on, as the zone-completion pass already does. Admins can raise `provider_timeout.library_of_congress`.
**Rejected:** a migration seeding a larger LoC override (a settings change nobody asked for).
**Reversible:** yes.

### D-11-2-6 — Health probe on the SRU host; other providers' User-Agents untouched
**Context:** `health_check_url` pointed at `https://www.loc.gov/`, which the provider no longer calls. BnF, K10plus, MusicBrainz and the shared client also carry stale hard-coded User-Agents.
**Chosen:** probe `http://lx2.loc.gov:210/LCDB` (HEAD answers 400 → reachable under #285); fix the LoC User-Agent only, as the AC asks.
**Rejected:** a repo-wide User-Agent sweep in a production-fix story.
**Reversible:** yes.
