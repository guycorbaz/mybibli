-- Story 19-7 (#657, D1) — the automatic backup interval (Admin → System).
--
-- Hours between two checks of the backup scheduler; `0` is off. Offered
-- values: 0, 1, 2, 3, 4, 6, 8, 12, 24. Default 1 (every hour): a check
-- writes a backup only when something changed since the last one. Any
-- other stored value reads as the default (`AppSettings::load_from_db`).

INSERT INTO settings (setting_key, setting_value, version) VALUES
('backup_interval_hours', '1', 1)
ON DUPLICATE KEY UPDATE setting_key = setting_key;
