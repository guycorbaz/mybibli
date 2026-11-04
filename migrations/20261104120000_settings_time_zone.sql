-- Story 15-1 (#602) — the instance time zone (Admin → System).
--
-- An IANA zone name in which the web interface shows dates and cuts
-- days (`utils::local_time`). Storage, the HTTP API and the MCP server
-- stay UTC. Default `Europe/Zurich`; an invalid stored value reads as
-- that default (`AppSettings::load_from_db`).

INSERT INTO settings (setting_key, setting_value, version) VALUES
('time_zone', 'Europe/Zurich', 1)
ON DUPLICATE KEY UPDATE setting_key = setting_key;
