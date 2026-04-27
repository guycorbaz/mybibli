-- Story 8-4 P19: pin `utf8mb4_unicode_ci` collation on the four reference
-- taxonomy tables' UNIQUE `name` columns.
--
-- Without an explicit COLLATE, the column inherits the table-level (or
-- DB-level) default. Different deployment environments use different
-- defaults — a binary or case-sensitive collation lets "Roman" and
-- "roman" coexist as two distinct rows under the UNIQUE constraint,
-- producing visually duplicate entries that the admin cannot disambiguate.
--
-- `utf8mb4_unicode_ci` is case-INSENSITIVE and accent-insensitive enough
-- for typical taxonomy names. Aligns the four ref tables on the same
-- semantic so admins get the case-insensitive "name already exists" error
-- they expect.

ALTER TABLE genres
    MODIFY COLUMN name VARCHAR(255) COLLATE utf8mb4_unicode_ci NOT NULL;

ALTER TABLE volume_states
    MODIFY COLUMN name VARCHAR(255) COLLATE utf8mb4_unicode_ci NOT NULL;

ALTER TABLE contributor_roles
    MODIFY COLUMN name VARCHAR(255) COLLATE utf8mb4_unicode_ci NOT NULL;

ALTER TABLE location_node_types
    MODIFY COLUMN name VARCHAR(255) COLLATE utf8mb4_unicode_ci NOT NULL;
