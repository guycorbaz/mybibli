-- v1.7.1 fix #308 — seed the runtime-tunable log_level setting row.
--
-- CR #301 in v1.7.0 advertised that admins could flip the log level
-- from /admin > System without a redeploy, but the admin form +
-- runtime reload were never implemented. This migration seeds the
-- K/V row that the form (added in this release) writes to and that
-- `AppSettings::load_from_db` reads back on each save.
--
-- The default 'info' matches `MYBIBLI_LOG_LEVEL` env-var default in
-- `src/main.rs` so a fresh install and an upgraded install both
-- end up at the same starting level.
--
-- Accepted values (validated server-side, see
-- `services::admin_system::validate_log_level`):
--   - plain level: trace, debug, info, warn, error
--   - tracing-subscriber EnvFilter directive list:
--       mybibli=debug,sqlx=warn
--       mybibli=trace
--       etc.

INSERT INTO settings (setting_key, setting_value, version)
VALUES ('log_level', 'info', 1)
  ON DUPLICATE KEY UPDATE setting_key = setting_key;
