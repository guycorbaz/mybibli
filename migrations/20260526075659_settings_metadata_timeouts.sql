-- v1.7.9 #334 — seed the runtime-tunable metadata-chain and provider-health
-- per-probe timeouts.
--
-- Two timeouts surface in /admin > System (pattern v1.7.1 #308 log_level):
--   * metadata_chain_per_provider_timeout_secs — per-provider call timeout
--     inside the chain (was hardcoded 5 in src/metadata/chain.rs).
--   * provider_health_probe_timeout_secs — per-probe HEAD timeout for the
--     5-min reachability ping in src/tasks/provider_health.rs (was env-var
--     only via MYBIBLI_PROVIDER_HEALTH_TIMEOUT_SECS, default 10).
--
-- Both accept 1..=60 (validated server-side in
-- services::admin_system::validate_provider_timeout_secs). Defaults match
-- the prior hardcoded values so an upgraded install behaves identically
-- until the admin tunes them.
--
-- Env-var bootstrap (one-shot) via config::migrate_legacy_env_vars copies
-- the respective env var into the row IF the row's current value is the
-- seeded default AND the env var is set, preserving the
-- "deployment-time intent wins on boot" model from story 8-5.

INSERT INTO settings (setting_key, setting_value, version)
VALUES ('metadata_chain_per_provider_timeout_secs', '5', 1)
  ON DUPLICATE KEY UPDATE setting_key = setting_key;

INSERT INTO settings (setting_key, setting_value, version)
VALUES ('provider_health_probe_timeout_secs', '10', 1)
  ON DUPLICATE KEY UPDATE setting_key = setting_key;
