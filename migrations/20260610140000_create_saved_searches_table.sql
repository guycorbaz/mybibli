-- CR #367: saved_searches table.
--
-- A saved search is a named, re-runnable bundle of the home browse state
-- (free-text `q`, active filter chip, sort column + direction) — everything
-- already encoded in the `/?q=...&filter=...&sort=...&dir=...` URL. The four
-- criteria are stored as separate nullable columns (not a blob) so the run
-- path can re-validate `sort`/`dir` against the existing whitelist.
--
-- Single-tenant: saved searches are GLOBAL (shared by every librarian),
-- consistent with the one-settings-table model — no per-user FK.
--
-- UNIQUE on `name` (case-insensitive via utf8mb4_unicode_ci). Soft-delete +
-- name reuse reactivates the trashed row (mirrors the genres CRUD pattern),
-- so the UNIQUE is on `name` alone, not `(name, deleted_at)`.

CREATE TABLE IF NOT EXISTS saved_searches (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(255) NOT NULL,
  q TEXT NULL COMMENT 'Free-text search query (the home `q` param)',
  filter VARCHAR(255) NULL COMMENT 'Active filter chip value, e.g. genre:3 / no_cover / overdue',
  sort VARCHAR(50) NULL COMMENT 'Sort column name (validated against whitelist on run)',
  dir VARCHAR(4) NULL COMMENT 'Sort direction: asc / desc',
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  deleted_at DATETIME NULL,
  version INT NOT NULL DEFAULT 1,

  UNIQUE KEY uq_saved_searches_name (name),
  KEY idx_saved_searches_deleted_at (deleted_at),
  KEY idx_saved_searches_name (name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
