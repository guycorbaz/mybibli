# Story 11.2: Library of Congress through SRU

Status: done

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

1. **AC1 — SRU is the primary LoC source.** Given an ISBN the LoC catalogue holds, when the chain consults the LoC provider, then `lookup_by_isbn` issues the SRU MARCXML request only and maps the record: title (245$a), subtitle (245$b), contributors (100$a then 700$a, deduplicated, `"Surname, Forename"` inverted to `"Forename Surname"` like the BnF provider), publisher (264$b of the publication statement — `ind2="1"`, else the first 264 that is not a copyright notice `ind2="4"` — else 260$b), publication date (008/07–10 when four digits, else the first digits run of 264$c / 260$c), language (008/35–37 through the existing `normalize_language`), page count (300$a), description (520$a), Dewey (082$a without the `/` segmentation marks), plus the six zones read today. ISBD trailing punctuation (` /`, ` :`, ` ;`, ` =`, `,`, a final `.` that does not close an initial) is removed from title, subtitle, contributors and publisher. `www.loc.gov/books/?fo=json` is no longer called and the JSON parser, its wire types and `LOC_API_BASE_URL` are removed.
2. **AC2 — text decoding.** Every field the provider returns is XML-entity-decoded (numeric `&#xHH;` / `&#NN;` and the five named entities) and NFC-normalised, so `L'e&#x301;conomie` becomes `L'économie`.
3. **AC3 — empty / no title.** A response with `numberOfRecords` 0, no `<record`, or a record without 245$a gives `Ok(None)` (a miss, the chain moves on).
4. **AC4 — Cloudflare challenge.** A response from the LoC endpoint carrying `cf-mitigated: challenge` (any status, header value case-insensitive) is logged at `warn` as a Cloudflare challenge — a message distinct from the plain HTTP-error one — and is never retried; `lookup_by_isbn` returns an error so the chain moves on to the next provider; `lookup_marc_zones` returns `None`.
5. **AC5 — non-success SRU status.** Any other non-success status is logged at `warn` (today `debug`) with the status; `lookup_by_isbn` maps 429 → `MetadataError::RateLimited`, 503 → `MetadataError::Unavailable`, others → `MetadataError::Network`; `lookup_marc_zones` returns `None`. Transport errors keep today's 3-attempt retry.
6. **AC6 — User-Agent.** One User-Agent built at compile time from `CARGO_PKG_VERSION` plus the project URL (`mybibli/<version> (https://github.com/guycorbaz/mybibli)`) replaces `mybibli/1.5.0` and `mybibli/1.14.0`. No attempt is made to pass the challenge (no browser imitation, no cookie, no retry on a challenge).
7. **AC7 — zone completion unchanged.** `supplies_marc_zones` stays `true`; `lookup_marc_zones` still returns only the six zones (via `parse_marcxml_response`), and the existing zone tests (`marcxml_*`, chain `zone_completion_*`, `catalog-marc-zones.spec.ts`) stay green.
8. **AC8 — health probe.** `health_check_url` points at the SRU host (`http://lx2.loc.gov:210/LCDB`) instead of `https://www.loc.gov/` (a `HEAD` there answers 400 → `Reachable` under the #285 any-status rule).
9. **AC9 — tests.** Unit tests: full-record mapping on the live-shaped fixture (both the 264 and the 260 shapes), entity decoding + NFC, punctuation cleanup and name inversion (initials kept), page-count forms, empty result, record without 245, challenge-header detection. Provider tests against a local mock HTTP server on a random port (the `with_base_url` constructor — the code path `LOC_SRU_BASE_URL` drives): nominal MARCXML record, empty result, challenge response (error + `warn` log asserted with `tracing-test`), plain 500 (distinct log), and the zone path returning `None` on a challenge. E2E: an ISBN only LoC holds is catalogued with the LoC title, author and publisher; an ISBN whose LoC answer is a Cloudflare challenge is still resolved by the next provider (Open Library).

## Tasks / Subtasks

- [x] T1 — `src/metadata/marc.rs` shared helpers (AC1, AC2)
  - [x] `datafields(xml, tag) -> Vec<&str>` (one slice per datafield; `extract_subfields` rebuilt on it, behaviour unchanged)
  - [x] `controlfield(xml, tag) -> Option<String>`
  - [x] `decode_xml_text(&str) -> String` (entities + NFC)
  - [x] `page_count_from_extent(&str) -> Option<i32>` lifted from `bnf.rs`, extended to `page`/`pages`; BnF switched onto it (existing BnF tests guard behaviour)
  - [x] `trim_isbd_punctuation(&str) -> String`
  - [x] unit tests
- [x] T2 — `Cargo.toml`: direct dependency `unicode-normalization = "0.1"` (already in `Cargo.lock` transitively, no new crate downloaded)
- [x] T3 — `src/metadata/library_of_congress.rs` (AC1–AC8)
  - [x] drop the JSON path (`base_url`, `LOC_API_BASE_URL`, `parse_json_response`, `LocEnvelope`, `LocResult`, `strip_html_tags`, `with_sru_base_url`); `with_base_url` now sets the SRU base
  - [x] `parse_marcxml_record(body) -> Option<MetadataResult>` (full record, AC1–AC3); `parse_marcxml_response` keeps the zones-only contract
  - [x] one `fetch_sru(&self, safe_isbn) -> Result<String, SruFailure>` used by both entry points: challenge detection (`is_cloudflare_challenge(&HeaderMap)`), warn logs, retry loop unchanged
  - [x] `USER_AGENT` const; `health_check_url`; module docs rewritten
  - [x] unit + mock-server tests (AC9)
- [x] T4 — E2E stack: remove `LOC_API_BASE_URL` from `tests/e2e/docker-compose.test.yml`; mock server: remove `/books/` + `_handle_loc_json`, add `LOC_PRIMARY_ISBN` (full MARC record with an entity-encoded accent) and `LOC_CHALLENGE_ISBN` (`403` + `cf-mitigated: challenge`, HTML body) on `/LCDB`, both on the BnF blocklist; `LOC_CHALLENGE_ISBN` in `OPEN_LIBRARY_KNOWN_ISBNS`
- [x] T4b — `docs/unimarc-mapping.md` § MARC 21 equivalence: note that LoC is now queried through SRU only and add the descriptive-field table (245$a/$b, 100/700$a, 264/260$b, 008/07–10, 008/35–37, 300$a, 520$a, 082$a)
- [x] T5 — E2E `tests/e2e/specs/journeys/catalog-loc-sru.spec.ts` (AC9)
- [x] T6 — gates: clippy `-D warnings`, `cargo test` (test DB), sqlx check, flake grep, `tsc --noEmit`, full E2E (`CI=true --workers=2`)

### Review Findings

Code review 2026-10-27 (Blind Hunter, Edge Case Hunter, Acceptance Auditor): 0 decision-needed, 11 patch, 1 defer, 17 dismissed.

- [x] [Review][Patch] Zone completion re-asked LoC (and BnF) after it had already missed or failed in the same run — a second identical SRU request, a doubled challenge warn, or a second retry loop out of the global budget. New trait hook `zones_come_with_lookup` (true for LoC and BnF); the chain skips a filler that failed this run, or missed when its zones ride on that lookup [src/metadata/chain.rs, src/metadata/provider.rs]
- [x] [Review][Patch] Page count read the bracketed unnumbered leaves (`"xii, 245, [2] p."` → 2) [src/metadata/marc.rs:page_count_from_extent]
- [x] [Review][Patch] A 200 whose body is not an SRU answer was reported as "LoC does not hold this ISBN" — now a `Parse` error with a warn [src/metadata/library_of_congress.rs:lookup_by_isbn]
- [x] [Review][Patch] Health probe ignored `LOC_SRU_BASE_URL` (E2E stack probed the real host) — now the queried endpoint [src/metadata/library_of_congress.rs:health_check_url]
- [x] [Review][Patch] `unsafe set_var` test with a false SAFETY claim (other tests call `new()` in parallel) — removed; `with_base_url` covers the endpoint wiring [src/metadata/library_of_congress.rs tests]
- [x] [Review][Patch] Publisher/date fallback could take a copyright 264 (`ind2="4"`) [src/metadata/library_of_congress.rs:parse_marcxml_record]
- [x] [Review][Patch] ISBD trim removed periods belonging to the value (`U.S.A.`, `Co.`, `Jr.`, ellipsis) and bare trailing `/` — now one closing period at most, separators only when space-preceded [src/metadata/marc.rs:trim_isbd_punctuation]
- [x] [Review][Patch] Numeric references to C0 controls (`&#0;`, `&#x1B;`) were decoded into the value — kept verbatim [src/metadata/marc.rs:decode_xml_text]
- [x] [Review][Patch] Challenge E2E did not show the chain went past LoC — asserts the provenance badge (LoC on the primary journey, Open Library on the challenge journey) [tests/e2e/specs/journeys/catalog-loc-sru.spec.ts]
- [x] [Review][Patch] Mock SRU handler panicked on a User-Agent mismatch (slow, misleading failure through the transport back-off) — answers 418 instead; tautological UA assertion removed [src/metadata/library_of_congress.rs tests]
- [x] [Review][Patch] Spec text vs code: publisher rule and E2E fixture wording aligned; retry-budget comment made accurate [this file, src/metadata/library_of_congress.rs]
- [x] [Review][Defer] 245$n/$p dropped, corporate 110/710 main entries ignored, relator-coded 700s kept as authors — deferred (#504)

Dismissed (17): namespace-prefixed `<marc:record>` / controlfield (LoC sends none; the old zone parser required a bare `<record` too); unmapped 3-letter MARC language codes (the BnF provider already stores raw UNIMARC codes); plaintext HTTP for SRU (the only endpoint LoC serves — no HTTPS on :210, 404 on :443; recorded as D-11-2-7); retry loop vs 5 s budget (D-11-2-5, comment corrected); attribute order before `tag=` (LoC serialises `tag ind1 ind2`); entity-only subfield shadowing a later repeat; `numberOfRecords` with inner whitespace; 008 date `0000`; `invert_name` leaving `"Smith, John, Jr."` inverted; `LOC_API_BASE_URL` ignored silently (test-only variable); "since v1.20.1" wording (the planned release); BnF path lacking its own `pages` test (covered by the shared `marc.rs` tests); File List vs patch scope; and duplicates across layers.

#### Re-review (Rule #6) — 2026-10-27

Second pass, same three layers: 0 High; Medium findings on the first-pass fixes, all patched; the rest Low.

- [x] [Review][Patch] The first-pass chain fix settled a provider on a primary MISS when its zones "ride on the lookup" — but a LoC record without 245$a is a miss that still carries zones, which were then lost. The hook is now `zones_share_lookup_endpoint` and the chain skips only a provider whose primary lookup FAILED in the run; a miss is re-asked (one cheap request). K10plus-shaped providers (separate zone endpoint) are re-asked even after a failure [src/metadata/chain.rs, src/metadata/provider.rs]
- [x] [Review][Patch] An SRU diagnostic (it carries `numberOfRecords 0`) passed as "not held" — now a failure on the primary path and a logged skip on the zone path; the zone path also logs a non-SRU 200 [src/metadata/library_of_congress.rs:sru_body_problem]
- [x] [Review][Patch] 264 fallback could take a distribution / manufacture statement, and the `ind2` test depended on attribute order and could match subfield text — `marc::indicator2` reads the opening tag (any attribute order, either quote); `datafields` slices now start at `<` [src/metadata/marc.rs, src/metadata/library_of_congress.rs]
- [x] [Review][Patch] C1 controls: the first-pass filter used `is_control()`, which also caught C1 — MARC 21 non-sort markers (U+0088/89, U+0098/9C) are now dropped, C0 stays verbatim [src/metadata/marc.rs:decode_xml_text]
- [x] [Review][Patch] ISBD trim: `"Gallimard, ."` kept its comma, a period after `)` was kept, `"Version 2.0."` kept its period, `Univ.`/`Pub.`/`Assoc.`… lost theirs, a separator after a no-break space was kept [src/metadata/marc.rs:trim_isbd_punctuation]
- [x] [Review][Patch] Plates before the main sequence (`"[16] p. of plates, 245 p."`) gave 16 [src/metadata/marc.rs:page_count_from_extent]
- [x] [Review][Patch] Blank `LOC_SRU_BASE_URL` burned the retry loop — falls back to production; unit-tested through `sru_base_url_from` (replaces the removed env test) [src/metadata/library_of_congress.rs]
- [x] [Review][Patch] Language mapping test restored for MARC codes; misplaced rustdoc on `page_count_from_extent`; mapping docs aligned with the publisher rule [src/metadata/library_of_congress.rs, src/metadata/marc.rs, docs/unimarc-mapping.md]

Dismissed (second pass): `tracing-test` dev-dependency (already in `Cargo.toml`); E2E provenance badge "could be another list" (it is the resolving provider's badge, `find_metadata_source`); namespace-prefixed `<marc:record>`; `"Plan B."` keeping its period (indistinguishable from an initial); `"Vol. 2, [100] p."`; plain-HTTP port 210 (D-11-2-7); hard-coded v1.20.1 (the planned release); `Node.js.`-style dotted words keeping the period.

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

### Validation additions (checklist pass)

- Mock fixtures, concrete: `LOC_PRIMARY_ISBN = "9780449000038"` → one record with `008` `200101s2020    nyu           000 0 eng d`, `100$a Primary, Sample,`, `245$a L'e&#x301;conomie de Congress :` / `$b a sample /` / `$c by S. Primary.` (so the E2E author assertion can only be met by the inverted 100$a), `264 _1 $b Congress Press,` `$c [2020]`, `300$a xii, 321 pages ;`, `520$a Congress summary.`; `LOC_CHALLENGE_ISBN = "9780449000045"` → `403`, headers `cf-mitigated: challenge`, `Content-Type: text/html`, a short HTML body. Both go on the BnF `NO_METADATA_ISBNS` blocklist (otherwise the BnF catch-all wins and LoC is never consulted as primary); the Google Books mock already misses unknown ISBNs. `LOC_CHALLENGE_ISBN` joins `OPEN_LIBRARY_KNOWN_ISBNS` with a distinctive title (`"Resolved After The Challenge"`) and no `covers` key.
- No integration test in `tests/` constructs `LibraryOfCongressProvider` (grep 2026-10-27), so removing `with_sru_base_url` and the JSON constructor arguments breaks nothing outside the module.
- `provider_health.rs` reads `health_check_url()` only to issue a HEAD; no template renders the URL.
- User manual chapters mention LoC as a source (`05-metadata-providers.tex`), not its endpoint — no change here; the release notes for v1.20.1 carry the user-visible part.

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
- T1: `marc::datafields` (slices include the closing tag, so `extract_subfield` runs on them unchanged; `extract_subfields` rebuilt on it), `controlfield`, `decode_xml_text` (entities + NFC), `trim_isbd_punctuation`, `page_count_from_extent` (lifted from `bnf.rs`, which now calls it; also fixes a latent slice panic on an extent where `)` precedes `(`).
- T2: `unicode-normalization = "0.1"` — resolved to the 0.1.25 already in `Cargo.lock`.
- T3: JSON path removed; `fetch_sru` shared by both entry points with distinct `warn` lines for a challenge and a plain status; challenge never retried; 429/503 typed; `parse_marcxml_record` for the full description; `parse_marcxml_response` (zones) now reads the first `<record>` only and decodes entities. A LoC win now costs one request instead of two. `health_check_url` → SRU host. `library_of_congress.rs` was reformatted with rustfmt (largely rewritten); `marc.rs` / `bnf.rs` left in their existing formatting (rustfmt of Rust 1.99 reports pre-existing diffs on `main` too).
- T4/T4b: mock `/books/` removed, `LOC_PRIMARY_ISBN` / `LOC_CHALLENGE_ISBN` added; compose variable removed; mapping doc updated.
- T5: `catalog-loc-sru.spec.ts` — 2 journeys, also green twice against a non-reset stack. App log during the run shows the distinct warn line: `Library of Congress answered with a Cloudflare challenge (cf-mitigated: challenge); not retried, moving on`.
- T6 gates (2026-10-27, under `mem-guard`): `cargo clippy --all-targets -- -D warnings` clean; `cargo test -j 1 --no-fail-fast` against the test DB — 61 binaries, 1595 passed, 0 failed (lib: 1205); `cargo sqlx prepare --check --workspace -- --all-targets` clean; flake grep clean; `tsc --noEmit` clean. Full E2E after `e2e-reset.sh`, `CI=true --workers=2`: **342 passed, 2 skipped, 0 failed**. Note: `cargo test` at the default job count was killed twice by the memory guard while compiling (oomd pressure on the user slice); `-j 1` went through.

### File List

- `Cargo.toml`, `Cargo.lock` — `unicode-normalization` direct dependency
- `src/metadata/marc.rs` — `datafields`, `controlfield`, `decode_xml_text`, `trim_isbd_punctuation`, `page_count_from_extent`, tests
- `src/metadata/bnf.rs` — page count through the shared helper; `zones_share_lookup_endpoint`
- `src/metadata/provider.rs` — `zones_share_lookup_endpoint` trait hook (review)
- `src/metadata/chain.rs` — completion skips providers settled in the run; 3 DB-backed tests (review)
- `src/metadata/library_of_congress.rs` — SRU-only provider, challenge detection, User-Agent, health URL, tests (incl. mock-server tests)
- `tests/e2e/mock-metadata-server/server.py` — LoC JSON mock removed; primary-record and challenge fixtures; BnF blocklist; Open Library entry
- `tests/e2e/docker-compose.test.yml` — `LOC_API_BASE_URL` removed
- `tests/e2e/specs/journeys/catalog-loc-sru.spec.ts` — new, 2 E2E tests
- `docs/unimarc-mapping.md` — descriptive-field table for LoC
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — 11-2 → done
- `_bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md` — § 11-2

### Change Log

- 2026-10-27 — Validation pass (checklist.md): concrete mock fixtures for the two E2E journeys, the `docs/unimarc-mapping.md` update (T4b), confirmation that no out-of-module caller uses the removed constructors. No blocker found.
- 2026-10-27 — Implementation T1–T6; status → review.
- 2026-10-27 — Code review (3 layers): 11 patches applied, 1 deferred (#504), 17 dismissed; status → done.
- 2026-10-27 — Re-review (Rule #6): Medium findings on the first-pass fixes patched (chain skip rule narrowed to failures, SRU diagnostics, 264 indicators, C1 controls, ISBD and page-count edge cases); third pass run on the delta.
