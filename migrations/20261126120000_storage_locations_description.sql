-- Story 20-4 (#716): an optional description for a storage location —
-- where to find it ("left bookcase, top shelf, behind the door").
--
-- Plain text, at most 500 characters (checked by the edit form, not by the
-- schema). Additive: existing rows read NULL, no default text, no index —
-- the description is shown on the location page only, never searched.
-- Sorts after 19-7's 20261126100000 so v1.29.0 applies both in order.

ALTER TABLE storage_locations
  ADD COLUMN description TEXT NULL
    COMMENT 'Story 20-4: optional plain-text description of where the location is';
