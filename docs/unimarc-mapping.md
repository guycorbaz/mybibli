# UNIMARC field mapping (#389 — Palier 1)

This document is the authoritative correspondence table between mybibli's internal
data model and the [UNIMARC](https://www.ifla.org/g/unimarc-rc/) bibliographic
standard. It exists so that:

1. Every cataloged title can be **expressed and stored conformant to UNIMARC**
   without abandoning the flat, UX-friendly schema.
2. There is a documented, testable contract (`tests/unimarc_mapping.rs`) that a
   title's fields round-trip losslessly to/from their UNIMARC zones.
3. A future **Palier 2** (ISO 2709 `.mrc` / UNIMARC XML import-export — tracked
   separately) has a stable foundation to build on.

**Scope of Palier 1:** internal-model conformity only. No `.mrc` / XML
serialization is produced yet — that is Palier 2.

## Storage decision

mybibli keeps the **flat `titles` table** and enriches it with a pragmatic
subset of the most common UNIMARC zones (below). We deliberately do **not**
introduce a parallel "raw UNIMARC zones" table: the pragmatic subset is
sufficient for a domestic / small-association library, and a raw-zone store only
becomes necessary if Palier 2 demands byte-fidelity round-trip with an external
SIGB. Structured contributors and work-level series already live in their own
tables (`title_contributors`, `series` / `title_series`).

## Mapping table

Legend — **Source**: `titles` = column on the `titles` table; `title_contributors`
/ `series` = related table. **Status**: `existing` (pre-#389), `new` (added by
migration `20260724000000_titles_unimarc_zones.sql`), `related-table`.

| UNIMARC zone | Label | mybibli field | Source | Status |
|---|---|---|---|---|
| 010$a | ISBN | `isbn` | titles | existing |
| 011$a | ISSN | `issn` | titles | existing |
| 101$a | Language of the work | `language` | titles | existing |
| 200$a | Title proper | `title` | titles | existing |
| 200$e | Other title information (subtitle) | `subtitle` | titles | existing |
| 200$f / 200$g | Statement of responsibility | `statement_of_responsibility` | titles | **new** |
| 205$a | Edition statement | `edition_statement` | titles | **new** |
| 210$c | Publisher | `publisher` | titles | existing |
| 210$d | Date of publication | `publication_date` | titles | existing |
| 215$a | Extent (page count) | `page_count` | titles | existing |
| 225$a | Collection / publisher's series title | `collection_title` | titles | **new** |
| 225$v | Collection volume numbering | `collection_number` | titles | **new** |
| 300$a | General note | `general_note` | titles | **new** |
| 330$a | Summary / abstract | `description` | titles | existing |
| 454 / 500$a | Uniform / original title | `original_title` | titles | **new** |
| 676$a | Dewey Decimal Classification | `dewey_code` | titles | existing |
| 700 / 701 / 702 | Personal-name responsibility (author, etc.) | contributor + `contributor_roles.name` | `title_contributors` | related-table |
| 410 (work-level series) | Series link | `series` + `title_series.position_number` | `series` / `title_series` | related-table |

### Notes on zone choices

- **200$f vs 700**: 200$f is the transcribed *statement of responsibility* (the
  by-line exactly as printed); the 7xx block holds *structured, indexed* name
  entries. mybibli keeps both — `statement_of_responsibility` for fidelity and
  `title_contributors` for search/indexing. `src/metadata/bnf.rs` already reads
  200$f as an author fallback; Palier 1 stops discarding it.
- **225 vs 410**: 225 is the collection *as printed on the item* (publisher's
  series, e.g. "Folio SF, 42"); 410 links to the *work-level* series entity.
  mybibli's `series` table models 410; the new `collection_*` columns model 225.
  They are intentionally distinct and may both be populated.
- **Cover image** (`cover_image_url`) has no standard UNIMARC bibliographic zone
  and is out of scope for conformity — it is a mybibli convenience field.

## MARC 21 equivalence (#439)

UNIMARC is the French/European standard; the Library of Congress catalogs in
**MARC 21**. Both serialise identically — `<datafield tag><subfield code>` —
so `src/metadata/marc.rs` reads either, and only the tag table below differs.
The LoC provider uses it to fill the same six columns for anglophone titles
that the BnF does not hold.

| mybibli field | UNIMARC (BnF) | MARC 21 (LoC) | MARC 21 label |
|---|---|---|---|
| `statement_of_responsibility` | 200$f | **245$c** | Statement of responsibility |
| `edition_statement` | 205$a | **250$a** | Edition statement |
| `collection_title` | 225$a | **490$a** | Series statement |
| `collection_number` | 225$v | **490$v** | Series volume number |
| `general_note` | 300$a | **500$a** | General note |
| `original_title` | 454 / 500$a | **240$a** | Uniform title |

**Read that table carefully: `500` means opposite things in the two standards.**
UNIMARC 500 is the uniform/original title; MARC 21 500 is the general note. The
two rows are not a typo.

Two further asymmetries worth knowing:

- **MARC 21 500 repeats.** Real LoC records routinely carry several general
  notes (confirmed on ISBN 9780134685991, which has two). `marc::extract_subfield`
  returns the first; `marc::extract_subfields` returns all. We store the first —
  a concatenated blob reads badly on the title-detail page.
- **240$a is a uniform title, not strictly an original title.** For a translated
  work it usually *is* the original title, which is why it is mapped here, but it
  is also used to disambiguate works with generic titles. Treat it as a good
  approximation rather than an exact equivalence.

### Coverage reality

Measured against the live LoC SRU endpoint on 2026-07-28 over the production
catalog's zone-less titles, by ISBN prefix: 9780 → 6/6 recovered, 9781 → 4/6,
and **zero** for 9782 (FR), 9783 (DE), 9791 and 9798. The two national-library
providers are complementary, not redundant: BnF covers the francophone catalog,
LoC the anglophone one, and a title in neither stays empty. French-prefix titles
the BnF could not describe will **not** be rescued by LoC.

## Data migration for existing titles

The migration is **additive** (all new columns NULLable) — the 207 titles already
in production keep every value; nothing is transformed. The new zones are
populated by an application-layer **BnF re-fetch backfill** (same mechanism as the
cover re-fetch, #427): for any title BnF knows, `statement_of_responsibility`,
`collection_*`, etc. are filled from the UNIMARC record. Titles unknown to BnF
stay sparse until manually enriched — incompleteness, never corruption. The
backfill run is observable through the `info`-level cataloging logs added by #434.
