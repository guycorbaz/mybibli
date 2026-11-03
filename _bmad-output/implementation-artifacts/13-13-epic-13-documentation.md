# Story 13.13: Documentation

Status: ready-for-dev

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

- [ ] T1 — chapters 3, 5, 12, 13 in EN and FR (keep both languages in sync; French text in good French)
- [ ] T2 — CLAUDE.md notes
- [ ] T3 — PDFs rebuilt; commit the PDFs
- [ ] T4 — Completion Notes with the release-note items
