-- Issue #70: preserve admin_audit history across user hard-delete.
--
-- The original FK in 20260424000001_create_admin_audit_table.sql is
-- `ON DELETE CASCADE`, which means hard-deleting a user (via the
-- admin Trash flow or the auto-purge scheduler) silently wipes
-- their entire audit history. That defeats the forensics intent of
-- the audit trail.
--
-- This migration:
--   * makes `admin_audit.user_id` NULLable (required for SET NULL);
--   * drops the old CASCADE FK;
--   * adds a new SET NULL FK so a deleted actor leaves their audit
--     rows behind (with `user_id = NULL`).
--
-- The actor's identity is preserved out-of-band in the JSON
-- `details` payload: every `AdminAuditModel::create` call site
-- now records `user_username` and `user_role` at action time, so
-- forensic readers can reconstruct who did what even after the
-- user row vanishes.

ALTER TABLE admin_audit
    DROP FOREIGN KEY admin_audit_ibfk_1;

ALTER TABLE admin_audit
    MODIFY COLUMN user_id BIGINT UNSIGNED NULL;

ALTER TABLE admin_audit
    ADD CONSTRAINT admin_audit_user_fk
        FOREIGN KEY (user_id) REFERENCES users(id)
        ON DELETE SET NULL;
