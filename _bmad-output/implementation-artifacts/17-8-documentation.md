# Story 17.8: Documentation

Status: ready-for-dev

No GitHub issue. Covers: FR172–FR177 (documentation). Epic 17; ships in v1.27.0. Sprint-status key: `17-8-epic-17-documentation`.

## Story

As an operator,
I want the manual to describe the new behaviour of epic 17,
so that it is documented as it works.

## Decisions

- **D1 — describe what shipped, read from the code and the story files** (`17-1` … `17-6`), never from the plan: labels and messages are quoted from `locales/{en,fr}.yml`, log lines from `src/services/isbn_repair.rs` and `src/services/default_skills.rs`, skill names and descriptions from `services::default_skills::DEFAULT_SKILLS`.
- **D2 — where each topic goes** (EN and FR, same structure so cross-references stay valid):
  - ch. 3 *Usage*, § catalog-scan: a new subsection *Typing codes by hand* (`sec:typed-codes`) — V-code in any case and near V-codes (17-1), the one ISBN rule of every form: separators, `ISBN` prefix, ISBN-10 → ISBN-13, 978/979 + check digit, duplicate refused at manual creation, unchanged stored value round-trips (17-3), the typed-UPC warning and its "keep as entered" box, the refusals that cannot be confirmed (17-4); a new subsection *Adding a volume from the title page* (`sec:title-add-volume`, 17-2); the reworded "code not recognised" answer.
  - ch. 3 § search: `#<id>` (17-5) — lists the title ignoring filters, the Trash / unknown messages, Enter after a scanner burst opens the title, the *Title ID* line of the title page.
  - ch. 3 § wish list: "By ISBN" takes 10 or 13 digits with separators (17-3).
  - ch. 12 § start-up repairs: the stored-ISBN repair (marker `17-3-stored-isbn-one-rule`, before the 13-3 conversion, Trash included, collisions, the three log lines) and the default-skill markers (`default-skill:<name>`, their log lines).
  - ch. 13 *Agents*: § skills gets *Default skills* (four names and purposes, inserted once and inactive as models, "provided by mybibli", never updated by a later release, a deleted default never returns, a name already taken is skipped); the tools/scope text mentions that the agent names titles by id and the librarian reaches them with `#<id>`.
  - Glossary: V-code entry (V + 4 digits, any case).
- **D3 — no release documentation** (Rule #19): no version bump, no *What's new in 1.27.0*, no README / website. "Since 1.27.0" is written in the body as the previous documentation stories did.
- **D4 — `CLAUDE.md` Key Patterns:** one concise paragraph each for V-code normalisation (17-1), the ISBN rule + boot repair (17-3), add-volume-from-title (17-2), typed UPC check (17-4), title id search (17-5). 17-6 is already there.

## Acceptance Criteria

1. **AC1.** The EN and FR manuals describe adding a volume from the title page, the V-code and ISBN entry rules (including the startup repair of stored ISBNs and its log lines), the typed-code warning, the search by `#<id>`, and the default skills (names, purpose, inserted once inactive as models, never updated, a deleted default never returns); the agent chapter is up to date.
2. **AC2.** Both PDFs are rebuilt with `docs/manual/build.sh` (under flock + mem-guard) with no undefined reference and no new warning; the PDFs are committed.
3. **AC3.** `CLAUDE.md` Key Patterns cover 17-1 … 17-5.
4. **AC4.** No release documentation is touched.

## Tasks

- [ ] ch. 3 EN + FR (typed codes, title-page volume, `#id` search, wish list ISBN).
- [ ] ch. 12 EN + FR (ISBN repair, default-skill markers).
- [ ] ch. 13 EN + FR (default skills, title ids).
- [ ] Glossary EN + FR (V-code).
- [ ] `CLAUDE.md` Key Patterns.
- [ ] Build PDFs; check warnings and references; commit.

## Dev Notes

- Build: `docs/manual/build.sh` (XeLaTeX + latexmk); heavy → `flock /tmp/station-heavy.lock mem-guard ./build.sh`.
- Docs only: Rust gates and E2E not required (brief).
- References: `_bmad-output/planning-artifacts/epics.md` § Story 17.8; story files 17-1 … 17-6.

## Dev Agent Record

### File List

### Change Log

- 2026-11-11: story created.
