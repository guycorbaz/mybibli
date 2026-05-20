-- CR #280: mark a storage_locations row as organizational.
--
-- An organizational location can have child locations (organizational or
-- not) but CANNOT be assigned to any volume's `location_id`. The flag is
-- explicit, per-row, opt-in. Existing rows default to FALSE → zero
-- behavior change on the v1.5.2 → v1.6.0 upgrade.
--
-- Pairs with CR #237 (shelf audit workflow) where the distinction
-- between "container" nodes and "shelving" nodes matters.

ALTER TABLE storage_locations
  ADD COLUMN is_organizational BOOLEAN NOT NULL DEFAULT FALSE
    COMMENT 'CR #280: TRUE = this row can only hold child locations, not volumes';
