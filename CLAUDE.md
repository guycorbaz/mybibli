# mybibli — Foundation Rules

These rules apply to ALL development sessions and must be followed without exception.

## 1. DRY (Don't Repeat Yourself)
No duplicated code. Create functions/modules for any reused logic.

## 2. Unit Tests
All functions must have unit tests, written alongside implementation.

## 3. E2E Tests
All features must have Playwright end-to-end tests.

## 4. Code Language
All code, comments, variables, and commit messages must be in English.

## 5. Code Consistency
Maintain architecture doc and coding conventions as reference across sessions. Follow patterns established in `_bmad-output/planning-artifacts/architecture.md`.

## 6. Gate Rule
No milestone transition until ALL tests (unit + E2E) are green.

## 7. Retrospectives
Mandatory at the end of each milestone/epic, never postponed.

## 8. Pre-Retrospective Testing
Run the complete test suite before each retrospective.

## 9. Error Message Quality
Error messages are iteratively improved via milestone retrospectives from real usage.

## Architecture Quick Reference

- **Stack:** Rust + Axum + SQLx (MariaDB) + Askama + HTMX + Tailwind CSS v4
- **Error handling:** Use `AppError` enum — no `anyhow` or raw strings
- **Logging:** `tracing` macros only — no `println!`
- **i18n:** `t!("key")` for all user-facing text — never hardcode strings
- **DB queries:** Include `deleted_at IS NULL` in every query/JOIN
- **Services:** Business logic in `services/`, never in route handlers
- **DB pool:** Pass as `pool: &DbPool` (type alias for `sqlx::MySqlPool`)
- **HTMX:** Check `HxRequest` header, return fragment or full page accordingly
- **SQLx offline:** Run `cargo sqlx prepare` after any query change, commit `.sqlx/`
