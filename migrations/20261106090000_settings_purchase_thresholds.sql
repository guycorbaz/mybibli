-- Story 16-3 (#639, FR168) — when an agent may propose replacing a purchase
-- price (Admin → System, story 8-5 pattern). Read at each filing, no cache;
-- an invalid stored value reads as its default
-- (`services::volume_value_proposals::PriceThresholds`).
--
-- `purchase_negligible_amount` — a decimal in the default currency: a purchase
-- price at or below it, against a list price above it, is negligible.
-- `purchase_gap_factor` — a whole factor, 2..=100: a purchase price this far
-- from the list price, either way, looks wrong.

INSERT INTO settings (setting_key, setting_value, version) VALUES
('purchase_negligible_amount', '1.00', 1),
('purchase_gap_factor', '3', 1)
ON DUPLICATE KEY UPDATE setting_key = setting_key;
