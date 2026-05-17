-- Fix #232: strip the spurious "genre_id" entry from
-- titles.manually_edited_fields. The Rust code at
-- src/models/title.rs::detect_edited_fields used to push "genre_id"
-- on every title-edit form submit, which forced every subsequent
-- re-fetch through the conflict-confirmation flow and silently
-- swallowed the Open Library cover fallback (issues #225 / #228).
--
-- The change is purely cosmetic at the data layer: `titles.genre_id`
-- (the actual foreign-key column) is untouched. Only the audit-flag
-- array is cleaned up.

-- 1. Remove the "genre_id" element wherever it appears in the JSON
-- array. JSON_SEARCH returns the path to the first match (or NULL),
-- JSON_REMOVE then drops that path. We restrict the UPDATE to rows
-- that actually contain "genre_id" so we don't rewrite rows for
-- nothing.
UPDATE titles
SET manually_edited_fields = JSON_REMOVE(
        manually_edited_fields,
        JSON_UNQUOTE(JSON_SEARCH(manually_edited_fields, 'one', 'genre_id'))
    )
WHERE manually_edited_fields IS NOT NULL
  AND JSON_SEARCH(manually_edited_fields, 'one', 'genre_id') IS NOT NULL;

-- 2. If stripping "genre_id" left an empty array, collapse to NULL so
-- the re-fetch flow correctly takes the direct-apply branch on next
-- run. We compare via JSON_LENGTH rather than string equality because
-- MariaDB may normalize the JSON storage and the literal '[]' check
-- can miss in some configurations.
UPDATE titles
SET manually_edited_fields = NULL
WHERE manually_edited_fields IS NOT NULL
  AND JSON_LENGTH(manually_edited_fields) = 0;
