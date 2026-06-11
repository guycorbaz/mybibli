-- CR #396 — per-provider metadata-chain timeout overrides.
--
-- One row per registered provider, keyed `provider_timeout.<slug>` where
-- <slug> is the provider's display name lowercased with non-alphanumeric
-- runs collapsed to `_` (see src/metadata/provider.rs::provider_slug).
-- An EMPTY value means "no override — use the global scalar"
-- (`metadata_chain_per_provider_timeout_secs`, v1.7.9 #334). A non-empty
-- value must parse as 1..=60 seconds (same bounds as the scalar,
-- validated by services::admin_system::validate_provider_timeout_secs);
-- out-of-range rows are ignored at load time with a warning.
--
-- Seeding all rows up-front (rather than INSERTing on first save) keeps
-- the optimistic-lock UPDATE pattern of services::admin_system::save_setting
-- unchanged. A future provider needs one more row in its own migration —
-- same contract as the keyed-provider API-key rows from story 8-5.

INSERT INTO settings (setting_key, setting_value, version) VALUES
('provider_timeout.bdgest', '', 1),
('provider_timeout.bnf', '', 1),
('provider_timeout.google_books', '', 1),
('provider_timeout.library_of_congress', '', 1),
('provider_timeout.open_library', '', 1),
('provider_timeout.musicbrainz', '', 1),
('provider_timeout.omdb', '', 1),
('provider_timeout.tmdb', '', 1)
ON DUPLICATE KEY UPDATE setting_key = setting_key;
