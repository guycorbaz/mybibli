-- CR #243 — Per-volume purchase price + current value, with aggregations.
--
-- Owner-facing valuation feature. All five columns are nullable —
-- the user opts in per-volume; an untouched volume stays NULL across
-- the board. The `/stats/value` page filters to "rows where at least
-- one of the two values is set" so unvalued volumes don't dilute the
-- aggregations.
--
-- Currency strategy v1: per-column ISO 4217 code. No FX conversion;
-- the /stats/value page groups totals per currency. A future CR can
-- add rate-table conversion to a single target currency.

ALTER TABLE volumes
  ADD COLUMN purchase_price DECIMAL(10,2) NULL COMMENT 'CR #243 — historical cost paid for the volume',
  ADD COLUMN purchase_currency CHAR(3) NULL COMMENT 'ISO 4217 code (default copied from settings.default_currency at write time)',
  ADD COLUMN current_value DECIMAL(10,2) NULL COMMENT 'Last-set estimated market value',
  ADD COLUMN current_value_currency CHAR(3) NULL,
  ADD COLUMN current_value_updated_at DATETIME NULL COMMENT 'When was the current_value last touched; surfaced in admin to nudge re-estimation';

-- New K/V settings rows. `default_currency` is admin-overridable from
-- the System tab and seeded with CHF per the v1.5.0 install decision;
-- `show_value_indicators` is the on-home-dashboard opt-in toggle
-- (default OFF — keep the home page neutral for users who don't
-- track money).
INSERT INTO settings (setting_key, setting_value, version)
VALUES ('default_currency', 'CHF', 1)
  ON DUPLICATE KEY UPDATE setting_key = setting_key;

INSERT INTO settings (setting_key, setting_value, version)
VALUES ('show_value_indicators', 'false', 1)
  ON DUPLICATE KEY UPDATE setting_key = setting_key;
