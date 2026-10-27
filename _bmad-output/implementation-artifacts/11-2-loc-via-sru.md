# Story 11.2: Library of Congress through SRU

Status: ready-for-dev

GitHub issue: #495 (type:bug, Medium). Covers: FR123. Epic 11 — second story (production fix, ships in v1.20.1 with 11-1 and 11-3).

## Story

As a librarian,
I want the Library of Congress provider to answer again,
so that English-language titles get their authoritative source back.

## Existing-code reality check (code map, 2026-10-27)

| Place | Today | Problem |
|---|---|---|
| `src/metadata/library_of_congress.rs::lookup_by_isbn` | primary call `GET {LOC_API_BASE_URL}?q=<isbn>&fo=json&c=1` (default `https://www.loc.gov/books/`), then a second SRU call for the six zones | `www.loc.gov` now answers every request with `403` + `cf-mitigated: challenge` (Cloudflare bot challenge, verified from two networks — issue #495 comment). The provider has returned nothing since 2026-10-26 |
| same, `fetch_marc_zones` | SRU `GET {LOC_SRU_BASE_URL}?version=1.1&operation=searchRetrieve&query=bath.isbn=<isbn>&recordSchema=marcxml&maximumRecords=1` (default `http://lx2.loc.gov:210/LCDB`), 3 attempts on transport errors (2 s, 4 s linear back-off) | still answers `200 text/xml` (verified live 2026-10-27), but a non-success status is logged at `debug` only |
| same, User-Agent | `mybibli/1.5.0` (JSON) and `mybibli/1.14.0` (SRU), hard-coded | stale, no contact URL |
| same, `health_check_url` | `https://www.loc.gov/` | no longer the host the provider talks to |
| `src/metadata/marc.rs` | `extract_subfield` / `extract_subfields` string scanner shared by BnF, LoC, K10plus; does **not** decode XML entities | the LoC feed encodes every non-ASCII char as a numeric reference, often decomposed (`L'e&#x301;conomie immate&#x301;rielle`, `&#xA9;2018`) — harmless in the six zones so far, unacceptable for a title / author |
| `src/metadata/bnf.rs::parse_sru_response` | page count parsed by an inline closure over UNIMARC 215$a (`"245 p."`) | the MARC 21 300$a needs the same parse plus the RDA form `"xx, 392 pages ;"` — a second copy is forbidden (Rule #1) |
| `src/metadata/chain.rs` | per-provider timeout default **5 s** (`metadata_chain_per_provider_timeout_secs`), zone-completion pass calls `lookup_marc_zones` on non-winning providers | once SRU is primary, a transport retry sequence (2 s + 4 s sleeps) can exceed 5 s; the chain then records LoC as timed out and moves on — acceptable, see Dev Notes |
| `src/services/cover.rs::resolve_cover_url_with_fallback` | cover-only fallbacks (BnF cover service, Inventaire, Open Library by ISBN) when the winning provider has no cover | the MARC record has no cover URL — these fallbacks cover the loss of LoC's `image_url` |
| `tests/e2e/docker-compose.test.yml` | sets `LOC_API_BASE_URL` and `LOC_SRU_BASE_URL` to the mock | `LOC_API_BASE_URL` becomes dead |
| `tests/e2e/mock-metadata-server/server.py` | `/books/` (LoC JSON, `_handle_loc_json`) and `/LCDB` (LoC SRU) for `LOC_MARC_ISBN = 9780449000014` | the JSON handler becomes dead; no LoC-wins journey and no challenge journey exist |

Live record shape (`bath.isbn=9780134685991`, captured 2026-10-27): `008` = `170925t20182018mau      b    001 0 eng d` (date1 at 07–10, language at 35–37); `100$a` `Bloch, Joshua,`; `245$a` `Effective Java /`, `245$c` `Joshua Bloch.`; `250$a`; `264 _1 $a Boston : $b Addison-Wesley, $c [2018]`; `300$a` `xx, 392 pages ;`; `082$a` `005.133`; repeated `500$a`; `520$a` summary. An older record (`9782070360246`) uses `260 $b Gallimard, $c c2010.` and `300$a 282 p. ;`.

Data model: no migration, no new setting, no user-facing string (no locale change).

## Acceptance Criteria

1. **AC1 — SRU is the primary LoC source.** Given an ISBN the LoC catalogue holds, when the chain consults the LoC provider, then `lookup_by_isbn` issues the SRU MARCXML request only and maps the record: title (245$a), subtitle (245$b), contributors (100$a then 700$a, deduplicated, `"Surname, Forename"` inverted to `"Forename Surname"` like the BnF provider), publisher (first 264$b, else 260$b), publication date (008/07–10 when four digits, else the first digits run of 264$c / 260$c), language (008/35–37 through the existing `normalize_language`), page count (300$a), description (520$a), Dewey (082$a without the `/` segmentation marks), plus the six zones read today. ISBD trailing punctuation (` /`, ` :`, ` ;`, ` =`, `,`, a final `.` that does not close an initial) is removed from title, subtitle, contributors and publisher. `www.loc.gov/books/?fo=json` is no longer called and the JSON parser, its wire types and `LOC_API_BASE_URL` are removed.
2. **AC2 — text decoding.** Every field the provider returns is XML-entity-decoded (numeric `&#xHH;` / `&#NN;` and the five named entities) and NFC-normalised, so `L'e&#x301;conomie` becomes `L'économie`.
3. **AC3 — empty / no title.** A response with `numberOfRecords` 0, no `<record`, or a record without 245$a gives `Ok(None)` (a miss, the chain moves on).
4. **AC4 — Cloudflare challenge.** A response from the LoC endpoint carrying `cf-mitigated: challenge` (any status, header value case-insensitive) is logged at `warn` as a Cloudflare challenge — a message distinct from the plain HTTP-error one — and is never retried; `lookup_by_isbn` returns an error so the chain moves on to the next provider; `lookup_marc_zones` returns `None`.
5. **AC5 — non-success SRU status.** Any other non-success status is logged at `warn` (today `debug`) with the status; `lookup_by_isbn` maps 429 → `MetadataError::RateLimited`, 503 → `MetadataError::Unavailable`, others → `MetadataError::Network`; `lookup_marc_zones` returns `None`. Transport errors keep today's 3-attempt retry.
6. **AC6 — User-Agent.** One User-Agent built at compile time from `CARGO_PKG_VERSION` plus the project URL (`mybibli/<version> (https://github.com/guycorbaz/mybibli)`) replaces `mybibli/1.5.0` and `mybibli/1.14.0`. No attempt is made to pass the challenge (no browser imitation, no cookie, no retry on a challenge).
7. **AC7 — zone completion unchanged.** `supplies_marc_zones` stays `true`; `lookup_marc_zones` still returns only the six zones (via `parse_marcxml_response`), and the existing zone tests (`marcxml_*`, chain `zone_completion_*`, `catalog-marc-zones.spec.ts`) stay green.
8. **AC8 — health probe.** `health_check_url` points at the SRU host (`http://lx2.loc.gov:210/LCDB`) instead of `https://www.loc.gov/` (a `HEAD` there answers 400 → `Reachable` under the #285 any-status rule).
9. **AC9 — tests.** Unit tests: full-record mapping on the live-shaped fixture (both the 264 and the 260 shapes), entity decoding + NFC, punctuation cleanup and name inversion (initials kept), page-count forms, empty result, record without 245, challenge-header detection. Provider tests against a local mock HTTP server on a random port (the `with_base_url` constructor — the code path `LOC_SRU_BASE_URL` drives): nominal MARCXML record, empty result, challenge response (error + `warn` log asserted with `tracing-test`), plain 500 (distinct log), and the zone path returning `None` on a challenge. E2E: an ISBN only LoC holds is catalogued with the LoC title, author and publisher; an ISBN whose LoC answer is a Cloudflare challenge is still resolved by the next provider (Open Library).

## Tasks / Subtasks

- [ ] T1 — `src/metadata/marc.rs` shared helpers (AC1, AC2)
  - [ ] `datafields(xml, tag) -> Vec<&str>` (one slice per datafield; `extract_subfields` rebuilt on it, behaviour unchanged)
  - [ ] `controlfield(xml, tag) -> Option<String>`
  - [ ] `decode_xml_text(&str) -> String` (entities + NFC)
  - [ ] `page_count_from_extent(&str) -> Option<i32>` lifted from `bnf.rs`, extended to `page`/`pages`; BnF switched onto it (existing BnF tests guard behaviour)
  - [ ] `trim_isbd_punctuation(&str) -> String`
  - [ ] unit tests
- [ ] T2 — `Cargo.toml`: direct dependency `unicode-normalization = "0.1"` (already in `Cargo.lock` transitively, no new crate downloaded)
- [ ] T3 — `src/metadata/library_of_congress.rs` (AC1–AC8)
  - [ ] drop the JSON path (`base_url`, `LOC_API_BASE_URL`, `parse_json_response`, `LocEnvelope`, `LocResult`, `strip_html_tags`, `with_sru_base_url`); `with_base_url` now sets the SRU base
  - [ ] `parse_marcxml_record(body) -> Option<MetadataResult>` (full record, AC1–AC3); `parse_marcxml_response` keeps the zones-only contract
  - [ ] one `fetch_sru(&self, safe_isbn) -> Result<String, SruFailure>` used by both entry points: challenge detection (`is_cloudflare_challenge(&HeaderMap)`), warn logs, retry loop unchanged
  - [ ] `USER_AGENT` const; `health_check_url`; module docs rewritten
  - [ ] unit + mock-server tests (AC9)
- [ ] T4 — E2E stack: remove `LOC_API_BASE_URL` from `tests/e2e/docker-compose.test.yml`; mock server: remove `/books/` + `_handle_loc_json`, add `LOC_PRIMARY_ISBN` (full MARC record with an entity-encoded accent) and `LOC_CHALLENGE_ISBN` (`403` + `cf-mitigated: challenge`, HTML body) on `/LCDB`, both on the BnF blocklist; `LOC_CHALLENGE_ISBN` in `OPEN_LIBRARY_KNOWN_ISBNS`
- [ ] T5 — E2E `tests/e2e/specs/journeys/catalog-loc-sru.spec.ts` (AC9)
- [ ] T6 — gates: clippy `-D warnings`, `cargo test` (test DB), sqlx check, flake grep, `tsc --noEmit`, full E2E (`CI=true --workers=2`)

## Dev Notes

- **Why not keep the JSON as a fallback.** The epic AC says `?fo=json` is no longer called; a Cloudflare challenge is a policy, not an outage, so a fallback would add a guaranteed-403 request per scan. Removed outright.
- **Cover.** MARC 21 carries no cover URL. The chain's cover-only fallbacks (`services::cover::resolve_cover_url_with_fallback`: BnF cover service → Inventaire → Open Library by ISBN) run whenever the winner has no cover, so no LoC-specific work is needed. Do not invent a cover from 856$u (links to tables of contents, not images).
- **Name inversion.** `extract_subfield` cannot see indicators, so invert on the string: exactly one `", "` → `"Forename Surname"`; otherwise keep as is. Keeps the BnF convention (`"Albert Camus"`). Relator subfields ($e, $4) and dates ($d) are separate subfields and never read. 700 fields carrying a `$t` (name-title added entries — a related work, not a contributor) are skipped; this is why `datafields` is needed.
- **Final period.** MARC ends fields with `.`; strip it unless the last whitespace-separated token is a single letter followed by `.` (an initial: `"Tolkien, J. R. R."` keeps its last period).
- **Dates.** 008/07–10 is date1 and may contain `u` (`19uu`) or blanks → fall back to 264$c / 260$c (`[2018]`, `c2010.`, `©2018`) and take the first run of four digits.
- **Timeouts.** The chain's per-provider default is 5 s. With SRU primary, a dropped first connection + 2 s sleep + second attempt fits; a third attempt (after a further 4 s) does not, and the chain then records LoC as `TimedOut` and moves on. That is the same budget the zone-completion pass already lives under; an admin can raise `provider_timeout.library_of_congress` in Admin → System. No default change in this story.
- **Challenge detection** reads only the header (`cf-mitigated`), never the body; it applies to any status (Cloudflare sends 403, but a 503 challenge exists too) and short-circuits the retry loop.
- **Entity decoding is LoC-only.** BnF and K10plus readers are untouched (the shared `extract_subfield` stays raw); decoding happens in the LoC parser on every value it emits, zones included.
- **Rule #12.** `library_of_congress.rs` (657 lines) shrinks on the JSON removal and grows on the record parser/tests; stays far below 2000.
- **Production follow-up (release notes, not code).** Titles scanned since 2026-10-26 whose metadata came from no provider, or from a provider other than LoC, keep their cached result; English-language titles can be refreshed through the existing per-title re-download or the Admin metadata backfill once v1.20.1 is deployed.
- Out of scope: other providers' stale User-Agents (BnF `mybibli/0.1.0`, K10plus `mybibli/1.16.0 …`, MusicBrainz, the shared client's `mybibli/1.0`); user-manual wording (story 11-24).

### Regression map

- `parse_json_response` tests (8) disappear with the JSON path; `json_result_keeps_its_fields_when_marc_zones_are_merged_in` becomes a full-record test (the record now carries both the descriptive fields and the zones).
- `marcxml_maps_the_marc21_tags_onto_the_internal_zones`, `marcxml_zero_records_is_none`, `marcxml_garbage_is_none_not_a_panic` keep `parse_marcxml_response` unchanged. `marcxml_never_supplies_a_cover_or_title` is about the zones-only parser and stays true (zones parser never sets a title).
- BnF page-count tests (`bnf.rs` tests) guard the `page_count_from_extent` lift.
- `marc.rs` tests guard the `datafields` rewrite of `extract_subfields`.
- E2E `catalog-marc-zones.spec.ts`: `LOC_MARC_ISBN` is answered by the BnF catch-all, so LoC is consulted only through `lookup_marc_zones` — unchanged. The K10plus journey needs the LoC SRU mock to miss `9780449000021` — unchanged.
- E2E specs relying on "every provider misses" (`9780000000002`, `…019`, `…026`, `…033`): the LoC SRU mock answers `numberOfRecords 0` for any unknown ISBN — unchanged.
- `provider-chain.spec.ts` known ISBNs `9780134685991` / `9780201633610` are answered by Google Books before LoC — unchanged.

### Testing standards

- Unit tests in `#[cfg(test)] mod tests`; async provider tests with `#[tokio::test]` and an `axum` router bound to `127.0.0.1:0` (no new dev-dependency); log assertions with `#[tracing_test::traced_test]` + `logs_contain`.
- E2E: `loginAs(page, "librarian")`, fixed mock-keyed ISBNs (same convention as `catalog-marc-zones.spec.ts`), rerunnable against a non-reset stack (assert on the title page, not on the "new title" feedback), no `waitForTimeout`, `toPass` polling for the background fetch.

### References

- `_bmad-output/planning-artifacts/epics.md` § Story 11.2, FR123
- Issue #495 and its investigation comment (2026-10-27)
- CR #263 (LoC provider), CR #439 / #450 (SRU zones, K10plus)
- `CLAUDE.md` Foundation Rules #1, #2, #3, #12, #13

## Dev Agent Record

### Agent Model Used

claude-opus-5-5

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created

### File List

### Change Log
