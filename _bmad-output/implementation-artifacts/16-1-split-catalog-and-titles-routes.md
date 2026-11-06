# Story 16.1: Split the catalog and titles routes

Status: done

GitHub issue: #597 (type:code-review-finding). Covers: Foundation Rule #12. Epic 16; ships in v1.26.0.

## Story

As a maintainer,
I want `routes/catalog.rs` and `routes/titles.rs` under 2 000 lines,
so that the next changes land in focused files.

## Acceptance Criteria

1. **AC1.** Each file is under 2 000 lines, tests included, by moving whole concerns into new modules.
2. **AC2.** No route, template, SQL query, response or log message changes; the `.sqlx` cache is unchanged.
3. **AC3.** The full Rust suite stays green. No new test is needed beyond those that moved.

## Result

| Module | Lines | Concern |
|---|---|---|
| `routes/catalog.rs` | ≈ 1 730 | Catalog page, scan handlers, feedback helpers, keepalive |
| `routes/catalog_code.rs` | ≈ 275 | `detect_code_type` and its tests (story 16-5 lands here) |
| `routes/catalog_title_form.rs` | ≈ 245 | Manual title form, type-specific fields |
| `routes/catalog_contributors.rs` | ≈ 445 | Contributor routes |
| `routes/catalog_volumes.rs` | ≈ 605 | Volume delete, detail, edit, update (story 16-2 lands here) |
| `routes/titles.rs` | ≈ 1 800 | Title detail, edit, update |
| `routes/title_metadata_confirm.rs` | ≈ 730 | Re-download and confirmation |
| `routes/title_cover_upload.rs` | ≈ 205 | Manual cover upload |
| `routes/title_series_assign.rs` | ≈ 100 | Series assignment from the title page |

Only item visibility (`pub(crate)`) and imports changed. A line-by-line comparison of the old and new files, ignoring imports and module docs, shows no other difference.
