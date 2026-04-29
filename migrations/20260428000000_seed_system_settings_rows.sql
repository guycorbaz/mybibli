-- Story 8-5: seed the four system-settings rows so the optimistic-locking
-- UPDATE has a row to target on first save.
--
-- INSERT IGNORE is the project convention for idempotent seed migrations
-- (see migrations/20260330000001_seed_default_genres.sql). On a fresh
-- install, all four rows land. On an upgrade from a pre-8-5 DB, the
-- four rows land. If any of them happens to already exist (e.g.,
-- default_language seeded by a hypothetical earlier story), IGNORE
-- protects us.
--
-- Empty strings for the API keys (not NULL): the `setting_value` column
-- is `TEXT NOT NULL`. The handler distinguishes "no change" from
-- "explicit clear" via a separate `_clear_<key>` form field, so the
-- stored value `""` always means "no key configured".

INSERT IGNORE INTO settings (setting_key, setting_value) VALUES
    ('default_language', 'fr'),
    ('google_books_api_key', ''),
    ('omdb_api_key', ''),
    ('tmdb_api_key', '');
