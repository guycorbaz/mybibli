-- v1.7.9 #46 — persist anonymous-session-purge last-run timestamp so the
-- 24h cadence survives restart loops within a 24h window.
--
-- src/tasks/anonymous_session_purge.rs previously slept a fixed 24h after
-- every boot before its first run. If the app crash-looped or rolling-
-- deployed faster than 24h, the purge never ran once and the `sessions`
-- table grew unbounded for as long as the loop lasted.
--
-- Seed empty: the spawn code maps empty → "no prior purge, sleep 24h then
-- run" which preserves the spec §Task 4.3 default for fresh installs.
-- After each run it writes back an RFC3339 UTC timestamp; subsequent
-- spawns compare NOW() - last_run >= 24h to decide between catch-up
-- (run immediately) and remainder-sleep (sleep 24h - elapsed).
--
-- Stored as a string (not DATETIME) so we reuse the existing
-- `settings.setting_value VARCHAR` schema and the same K/V helpers
-- (`save_setting`, `fetch_setting_rows`) — no new schema surface needed.

INSERT INTO settings (setting_key, setting_value, version)
VALUES ('last_anonymous_session_purge_at', '', 1)
  ON DUPLICATE KEY UPDATE setting_key = setting_key;
