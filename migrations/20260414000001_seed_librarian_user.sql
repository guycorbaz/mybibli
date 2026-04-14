-- Story 6-2: Seed a second user with role='librarian' for multi-role E2E testing.
-- Credentials: librarian / librarian
-- Hash: Argon2id, m=19456, t=2, p=1 (matches admin seed in 20260331000004_fix_dev_user_hash.sql)
-- Hash generation: throwaway src/bin/generate_librarian_hash.rs (deleted after use)
--   using argon2 crate v0.5 with random salt, then verified via src/routes/auth.rs::verify_password.
--
-- Idempotent: INSERT ... WHERE NOT EXISTS so re-applying is a no-op against a DB
-- that already has the librarian row.
--
-- NOTE on soft-delete edge case: the `users.username` column has a UNIQUE index
-- (see 20260329000000_initial_schema.sql) that covers ALL rows regardless of
-- `deleted_at`. The guard below only checks live rows, so re-running this
-- migration against a DB where 'librarian' was soft-deleted would fail with a
-- duplicate-key error. This is acceptable in practice because sqlx tracks
-- applied migrations and will not re-run this file; the pattern mirrors
-- 20260329000002_seed_dev_user.sql. If this seed is ever reconciled to support
-- rotated hashes / undelete, switch to INSERT ... ON DUPLICATE KEY UPDATE.
INSERT INTO users (username, password_hash, role, active)
SELECT 'librarian',
       '$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU',
       'librarian',
       TRUE
FROM DUAL
WHERE NOT EXISTS (
    SELECT 1 FROM users WHERE username = 'librarian' AND deleted_at IS NULL
);
