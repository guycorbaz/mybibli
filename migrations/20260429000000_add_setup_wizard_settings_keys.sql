-- Story 8-8: First-launch setup wizard sentinels.
-- Idempotent INSERT IGNORE per project convention (see
-- 20260428000000_seed_system_settings_rows.sql).
--
-- setup_completed_at — RFC 3339 / ISO 8601 UTC timestamp set when the admin
--   clicks "Complete setup" (e.g. '2026-04-29T12:34:56Z'). Empty string =
--   "not yet completed". `AppSettings::load_from_db` parses via
--   `chrono::DateTime::parse_from_rfc3339`; empty/malformed → `None`.
--   The wizard gate middleware reads this row to decide whether to fire.
-- setup_step_3_done — '1' once the Preferences step has been visited; resolves
--   the Step 3 vs Step 4 ambiguity when language='fr' + overdue=30 (defaults
--   that the user may have explicitly re-confirmed).

INSERT IGNORE INTO settings (setting_key, setting_value) VALUES
    ('setup_completed_at', ''),
    ('setup_step_3_done', '0');
