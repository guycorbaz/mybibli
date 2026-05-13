-- Issue #68: dedicated SYSTEM user for audit-trail attribution.
--
-- Pre-1.1.0 auto-purge audit rows hardcoded `user_id = 1` because no
-- system actor existed. That coupled the audit history to the admin
-- row at id=1 — deleting that admin (CASCADE on admin_audit.user_id)
-- would wipe the entire purge audit trail. This migration creates a
-- dedicated SYSTEM user so background tasks can attribute their
-- audit rows to a non-deletable actor, decoupling the audit history
-- from any human admin.
--
-- Schema changes:
--   * Extend `users.role` ENUM to add `'system'`. The role is NOT
--     login-eligible — the `POST /login` query in src/routes/auth.rs
--     filters `role IN ('admin', 'librarian')`. Defense in depth:
--     the SYSTEM row also carries `active = FALSE` and a
--     password_hash that is not a valid argon2 string, so
--     `verify_password` would short-circuit `false` even if the
--     login query were ever loosened.
--   * Insert the SYSTEM user row. Idempotent via `WHERE NOT EXISTS`
--     so a re-applied migration is a no-op.
--
-- The SYSTEM row's `id` is auto-assigned by AUTO_INCREMENT; callers
-- discover it by querying `WHERE role = 'system'` (see
-- `models::user::find_system_user_id`).

ALTER TABLE users
    MODIFY COLUMN role ENUM('librarian', 'admin', 'system')
                  NOT NULL DEFAULT 'librarian';

INSERT INTO users (username, password_hash, role, active)
SELECT 'SYSTEM',
       'NO_LOGIN_SYSTEM_USER_HASH_NOT_A_VALID_ARGON2_STRING',
       'system',
       FALSE
FROM DUAL
WHERE NOT EXISTS (
    SELECT 1 FROM users WHERE role = 'system'
);
