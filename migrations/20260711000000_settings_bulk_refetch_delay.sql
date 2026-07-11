-- Issue #419 — configurable inter-request delay for the admin bulk
-- cover-refetch loop (milliseconds between titles).
--
-- Prod evidence 2026-07-10: two back-to-back runs over ~113 titles at
-- ~1.2s/title tripped Google Books' throttling (503 storm) and recovered
-- ~0 covers. A 1000 ms default gap keeps a 113-title run at ~2 extra
-- minutes — irrelevant for a background admin action — and lets the
-- admin tune it from /admin > System without a restart.
--
-- Bounds 0..=60000 ms, validated by
-- services::admin_system::validate_bulk_refetch_delay_ms and re-checked
-- at load time in AppSettings::load_from_db (out-of-range rows fall back
-- to the default with a warning).

INSERT INTO settings (setting_key, setting_value, version) VALUES
('bulk_refetch_delay_ms', '1000', 1)
ON DUPLICATE KEY UPDATE setting_key = setting_key;
