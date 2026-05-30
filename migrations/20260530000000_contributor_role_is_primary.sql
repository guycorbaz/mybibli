-- CR #19: add a stable `is_primary` marker to `contributor_roles` so the
-- "author-first" sort order in title-contributor queries (and the lookup
-- in the async metadata-fetch chain) stops depending on the hardcoded
-- 'Auteur' string. The admin reference-data CRUD (story 8-4) lets an
-- operator rename a role row at any time, which would silently break the
-- previous `WHERE cr.name = 'Auteur'` pattern.
--
-- The new column flags the canonical "primary author" role regardless of
-- its current display name. Multiple rows may be flagged primary; the
-- ORDER BY just promotes any of them to the front, which is the intent.

ALTER TABLE contributor_roles
    ADD COLUMN is_primary BOOLEAN NOT NULL DEFAULT FALSE AFTER name;

-- Mark the seeded "Auteur" row as primary. The seed migration
-- (20260330000002) creates this row idempotently; the UPDATE here flips
-- the flag on whatever row currently carries the canonical name, even if
-- a prior operator renamed it.
UPDATE contributor_roles
SET is_primary = TRUE
WHERE name = 'Auteur' AND deleted_at IS NULL;
