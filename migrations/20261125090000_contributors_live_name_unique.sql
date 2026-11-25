-- Story 19-3 (#541, D4) — one live contributor per name.
--
-- `contributors.name` had no unique key: two find-or-creates of one new
-- name racing each other (two background author fetches, a title-page add
-- and a metadata acceptance) both read "none" and both inserted.
--
-- `name_live` is the name of a live row and NULL for a soft-deleted one, so
-- the Trash never blocks a new row. It carries no COLLATE clause on purpose:
-- like `name` (never altered since the initial schema), it takes the table
-- default, so the key compares names exactly as `find_by_name_exact_in`
-- does (case, accents and trailing spaces ignored).
--
-- Existing duplicates are left exactly as they are (merging them is #591):
-- every live row that is not the lowest id among the live rows of its name
-- gets `legacy_duplicate_slot = id`; every other row keeps 0. New rows have
-- slot 0 and therefore collide with the oldest live namesake. A rename that
-- changes the name and a Trash restore reset the slot to 0.
--
-- A generated column cannot be written: every INSERT into `contributors`
-- lists its columns. A backup loader must skip `name_live` and carry
-- `legacy_duplicate_slot`.

ALTER TABLE contributors
    ADD COLUMN name_live VARCHAR(255) AS (IF(deleted_at IS NULL, name, NULL)) PERSISTENT,
    ADD COLUMN legacy_duplicate_slot BIGINT UNSIGNED NOT NULL DEFAULT 0;

-- `updated_at = updated_at` keeps the ON UPDATE CURRENT_TIMESTAMP from
-- touching the legacy rows: nothing visible changes.
UPDATE contributors c
    JOIN contributors o
      ON o.name = c.name AND o.deleted_at IS NULL AND o.id < c.id
   SET c.legacy_duplicate_slot = c.id, c.updated_at = c.updated_at
 WHERE c.deleted_at IS NULL;

ALTER TABLE contributors
    ADD UNIQUE KEY uq_contributors_live_name (name_live, legacy_duplicate_slot);
