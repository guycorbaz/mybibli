# Story 15.8: Epic 15 documentation

Status: done

Covers: FR161–FR166 (documentation). Epic 15; ships in v1.25.0.

## Story

As an operator,
I want the manual to describe the new features,
so that they are documented as they work.

## Acceptance Criteria

1. **AC1** — The user manual (English and French) describes the time zone setting, the Skills tab and how an agent uses skills, series modification and value proposals, and value coverage; the agent chapter lists the new tools and prompts.
2. **AC2** — The PDFs are rebuilt; `CLAUDE.md` and the threat model reflect the new patterns.
3. **AC3** — Release documentation (Foundation Rule #19, v1.25.0) is done at release time, not in this story.

## What changed

- **Ch. 2 Configuration** (EN + FR): *System settings → Time zone* (IANA name, default `Europe/Zurich`, what it changes, UTC for storage / API / agents).
- **Ch. 3 Usage** (EN + FR): *What the library is worth → Coverage* (share, oldest / newest estimate, estimates older than a year or undated, per genre and series, the agent's help; totals leave out deleted titles).
- **Ch. 13 Agents** (EN + FR): scope list (series correction, volume values, skills); tools `get_skill`, `list_volumes_to_value`, `propose_series_update`, `propose_volume_value` and the `get_title` amounts; refusal list; kind filter; outdated rules for series changes and values; new subsection *Series changes and volume values* (review, edit and removal rules, the list-price label); new section *Skills: instructions for the agent* (Admin → Skills, prompts, `get_skill`, no capability).
- **Threat model** § 5.10: series changes and volume values (nothing to mark hand-set, the total and currency rules, the amounts the read tools expose). § 5.11 (skills) was written in stories 15-2 / 15-3; CLAUDE.md patterns were added in each story.
- PDFs rebuilt (`docs/manual/build.sh`, XeLaTeX); every cross-reference resolves.

## Dev Agent Record

### File List

- docs/manual/{en,fr}/02-configuration.tex, 03-usage.tex, 13-agents-proposals.tex
- docs/manual/mybibli-manual-{en,fr}.pdf
- docs/auth-threat-model.md
