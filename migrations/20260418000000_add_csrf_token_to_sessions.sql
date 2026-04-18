-- Story 8-2: CSRF synchronizer-token column on sessions.
--
-- Add a 64-char hex CSRF token column and backfill every existing row at
-- migration time. Backfill avoids a deploy-to-first-read race where a
-- row with csrf_token='' could be tricked into validating an empty
-- X-CSRF-Token header. RANDOM_BYTES() requires MariaDB >= 10.10; the
-- project's MariaDB image is 10.11.
--
-- The `user_id` column is already NULL-able per the initial schema
-- (migrations/20260329000000_initial_schema.sql line 224) — no ALTER is
-- needed (and any ALTER would be destructive to the fk_sessions_user FK).

ALTER TABLE sessions
    ADD COLUMN csrf_token VARCHAR(64) NOT NULL DEFAULT '';

UPDATE sessions
   SET csrf_token = LOWER(HEX(RANDOM_BYTES(32)))
 WHERE csrf_token = '';
