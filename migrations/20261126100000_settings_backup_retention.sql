-- Story 19-7 (#657, D4 amended by the project lead, 2026-11-26) — how many
-- backup days the rotation keeps (Admin → System → Backups).
--
-- The rotation keeps every backup of the last 24 hours, then the newest
-- backup of each of the N most recent local days on which a backup was
-- written. N is an integer from 1 to 365, default 14; any other stored
-- value reads as the default (`AppSettings::load_from_db`).

INSERT INTO settings (setting_key, setting_value, version) VALUES
('backup_retention_days', '14', 1)
ON DUPLICATE KEY UPDATE setting_key = setting_key;
