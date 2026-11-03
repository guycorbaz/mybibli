-- Story 13-4 (#578), code review M1: one-shot startup data repairs.
--
-- A startup repair whose effect cannot be told apart from healthy data
-- afterwards (an HTML description converted to plain text may still hold
-- `<b>` or `&lt;` as text) must run exactly once per database, not "until
-- nothing matches". `services::data_repairs::run_once` checks for the
-- repair's row before running it and inserts it after the repair succeeded.
--
-- Not an entity table: no soft delete, no version — a row is a fact
-- ("this repair ran at that time") that is never edited or removed.

CREATE TABLE data_repairs (
    name VARCHAR(100) NOT NULL PRIMARY KEY,
    applied_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
