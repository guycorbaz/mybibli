# Story 13.13: Documentation

Status: done

Covers: FR153–FR160 (documentation). Epic 13 — last story; ships in v1.23.0.

## Story

As an operator,
I want the manual to describe the new behaviour,
so that cataloguing is documented as it now works.

## Context

None of the epic 13 stories updated the user manual (`docs/manual/{en,fr}/*.tex`, LaTeX, built by `docs/manual/build.sh` into `docs/manual/mybibli-manual-{en,fr}.pdf`). Read each story file of epic 13 (`_bmad-output/implementation-artifacts/13-*.md`, Completion Notes and Dev Notes) for the exact behaviour, and the merged code where needed.

## Acceptance Criteria

1. **Chapter 3 — usage (EN + FR):**
   - scan: a double read of the scanner is ignored (1 s window); re-scanning a label of the current title says "already recorded"; a label of another title names its title and points to the volume page (13-1); a failed shelve / a deleted active location are reported;
   - moving a volume to another title from its page (13-8; the loan follows the volume);
   - date added / modified on title and volume pages, the "added since" filter and the "date added" sort on the home page, the Recent additions table in list mode (13-5, 13-7); days are cut in UTC (#602);
   - the tome number: "Tome N" display, the detected hint for a title in no series, pre-filled position, inline position edit; markers stripped from provider titles (13-10) with its known limits (bd titles like "L'Ennemi public n°1" — fix by hand, then protected);
   - full location paths in volume lists (13-6); every series offered in the selector (13-5);
   - labels as the way to mark doubtful records (13-9 decision: e.g. a "Code douteux" label; `/labels/{id}` lists them).
2. **Chapter 5 — metadata providers:** descriptions converted to plain text (Google Books, Comic Vine) and the one-time repair at upgrade (13-4); a lookup that met a saturated provider (429/503) is retried later (15 min → 24 h, 5 attempts) (13-2); titles stored with an ISBN-10 as UPC before v1.21.0 are converted at startup and looked up again (13-3).
3. **Chapter 13 — agents and proposals:** new tools `list_labels`, `propose_label` (label proposals, review, acceptance, staleness); `get_title` new fields (dates, location path, labels, detected tome); `search_titles` `added_since` and `created_at`/`last_added_at`; an owned title may fill an expected tome (omnibus rule; the review note "Fills the expected tome …") (13-11); the tool reference table updated.
4. **Chapter 12 — operations:** the `metadata_fetch_retries` queue and its log lines; the `data_repairs` marker table (one-time repairs run at boot: description cleanup); the boot-time ISBN-10 conversion log line; key revocation retries on deadlock (13-14) if operator-relevant.
5. **PDFs rebuilt** (EN + FR) with `mem-guard docs/manual/build.sh` (check its usage; LaTeX is heavy — `free -g` first); no LaTeX warnings about undefined references.
6. **CLAUDE.md:** a short pattern note for each new mechanism not yet documented there (check first — some stories already added notes): metadata retry queue (13-2), data_repairs run-once (13-4), tome hint and parser shapes (13-10), volume move + title delete transaction (13-8), date-added rule (13-7), label proposals (13-9, maybe done), expected-tome fill by proposal (13-11, done).
7. **Release documentation** (Foundation Rule #19, v1.23.0) is NOT in this story — it is done at release time. Collect in Completion Notes the release-note items (upgrade effects: one-time description repair, ISBN-10 conversion and re-fetch, new tables, the date filter ignoring exact ISBN hits on the web, tome markers stripped on re-download, etc.).

## Tasks

- [x] T1 — chapters 3, 5, 12, 13 in EN and FR (keep both languages in sync; French text in good French)
- [x] T2 — CLAUDE.md notes
- [x] T3 — PDFs rebuilt; commit the PDFs
- [x] T4 — Completion Notes with the release-note items

## Completion Notes

**Manual (T1)** — EN and FR kept in step, same labels and structure.

- Chapter 3: home page (Recent cataloged rule, Recent additions following List/Grid view); `sec:scan-existing-vcode` (double read 1 s, same-title info, other-title warning + volume link, shelve failed / active location gone); `sec:added-since` (Added on / Modified on, the "added" rule, Added since filter, Added column and sort, indicator, UTC days, indicator replaces the date, exact ISBN/V-code hit ignores it, date not saved with a search); full location paths (under `sec:move-volume`); "Marking doubtful records" (management label such as *Code douteux*, `/labels/{id}`, agent may propose); new `sec:move-volume-title` (13-8, loan follows, refusals); new `sec:tome-number` (all series in the selector, tome display, Change editor, markers stripped per media type, detected hint and pre-fill, re-download preview, hand-typed titles, known limits with the hand-edit remedy, no bulk cleanup).
- Chapter 5: descriptions as plain text (Google Books, Comic Vine; agent filings; one-time repair, hand-edited untouched); retry of a saturated lookup (15 min → 24 h, 5 attempts, pacing, bulk-run pause); ISBN-10 stored as UPC converted at startup and looked up again.
- Chapter 12: new `sec:startup-repairs` — `metadata_fetch_retries` queue (task timing, log lines, SQL to inspect), `data_repairs` (marker, log lines, do not delete rows), ISBN-10 conversion log lines, key revocation retried on deadlock.
- Chapter 13: tool count 12 → 14; `search_titles` (`added_since`, `created_at`, `last_added_at`), `get_title` (dates, `location_path`, labels, `detected_tome`), `list_labels`, `propose_label`; list/detail/edit/accept/outdated/resubmission for label proposals; owned title filling an expected tome (omnibus rule, "Fills the expected tome …" note, settle at acceptance, staleness); revocation retry.
- Terminology: EN interface shows "Volume N" for the tome number (`tome.word` = Volume) — documented as such, with the French "Tome N". V-code stickers are "V-code" / "code V"; CR #443 labels are "management labels" / "étiquettes de gestion", as in the existing chapters.

**CLAUDE.md (T2)** — new pattern notes: metadata retry queue (13-2, with the 13-3 boot conversion), one-time data repairs + HTML → text placement (13-4), tome number (13-10), volume move + transactional title delete (13-8), date-added rule (13-7), existing-V-code scan + location paths (13-1, 13-6); revocation retry (13-14) added to the Proposal lifecycle note. Label proposals (13-9) and expected-tome fill (13-11) were already documented.

**PDFs (T3)** — `mem-guard docs/manual/build.sh en` then `fr`: no undefined reference or citation; the only "Missing character" warnings (box-drawing U+2500/U+2514 in the mono font) predate this story. Both PDFs committed; no build artefact tracked.

**Finding (UI copy):** `locales/fr.yml` `proposals.accept_modal_note_label` said "marquée comme dépassée" while the status everywhere else is "Périmée" — fixed by the coordinator in 648cdb7.

### Release-note items for v1.23.0 (AC7 — for the release-time doc sync)

- **Migrations (additive):** `metadata_fetch_retries`, `data_repairs`, `titles.detected_tome`, `proposals.kind` + `label_attach` and `proposal_items.target_table` + `volumes`. Back up first, as usual.
- **One-time repair at first start:** stored descriptions holding HTML are converted to plain text (hand-edited descriptions untouched); pending proposal descriptions converted; cached provider answers with markup discarded. Runs once per database (`data_repairs`).
- **Boot-time ISBN-10 conversion:** print titles stored before v1.21.0 with an ISBN-10 as UPC get their ISBN-13 and are looked up again through the retry queue; collisions are logged (`warn`) at every start until resolved by hand (#591).
- **Throttled lookups retried** automatically (15 min, 1 h, 4 h, 12 h, 24 h); the eight production titles of #583 can also simply be re-downloaded.
- **Scan:** double reads ignored (1 s); re-scanning a V-code says "already recorded" or names the title that holds it; shelve failures reported.
- **Move a volume to another title** from its page; the loan follows.
- **Dates:** Added on / Modified on on title and volume pages; "Added since" filter and "Added" column/sort on the home page; Recent cataloged counts titles that received a volume; days are UTC (#602). The web date filter ignores exact ISBN / V-code hits (MCP applies it); saved searches do not keep the date.
- **Recent additions** follow the List/Grid toggle (table in list view).
- **Full location paths** in volume lists; **every series** in the title-page selector.
- **Tome number:** "Tome N" (FR) / "Volume N" (EN) display, inline position edit, detected hint; tome markers stripped from provider titles on (re-)download unless the title was hand-edited — titles already catalogued are not cleaned in bulk; known false positives ("L'Ennemi public n°1") need one hand edit.
- **Agents:** 14 tools — `list_labels`, `propose_label` (label proposals: review, acceptance, staleness, resubmission); `get_title` dates, `location_path`, labels, `detected_tome`; `search_titles` `added_since`, `created_at`, `last_added_at`; an owned title may fill an expected tome by proposal (omnibus fills its range without ISBN comparison); agent descriptions converted to plain text at filing.
- **Ops:** API key revocation retried on deadlock (no more "Server error").

### Review Findings

Documentation review, single round (on a726d4c): 0 Medium+, 6 Low — all applied.

- [x] [Review][Patch] L1 — ch. 12 (EN/FR) intro: "two start-up repairs and a background retry task", not "three start-up steps".
- [x] [Review][Patch] L2 — ch. 13 (EN/FR): "accepted at filing" → "the agent may file it" / « le dépôt est admis ».
- [x] [Review][Patch] L3 — FR: « régler / règle » an expected tome → « résoudre / résout » (03-usage, 13-agents), as the glossary.
- [x] [Review][Patch] L4 — FR 03-usage: « scanner un autre label » (UI wording) for the other-title case.
- [x] [Review][Patch] L5 — ch. 12 (EN/FR): a failed repair rolls back its title/proposal changes and writes no marker (cache purge runs before, separately); collision log line quoted with its real suffix "(see #591)".
- [x] [Review][Patch] L6 — overfull boxes removed (en/12 data-repairs paragraph, fr/13 expected-tome item: reworded + `\allowbreak`); `~` before the closing guillemet of « Tome 3 (détecté) ». Rebuilt PDFs: no undefined reference, no overfull box added compared with the previous build.
- [x] Optional — CLAUDE.md retry-queue note: 13-3 sentence trimmed.

