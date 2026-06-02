-- #21 (DB hardening): document why `pending_metadata_updates.session_token`
-- intentionally has NO foreign key to `sessions.token`.
--
-- Rationale (single-tenant lifecycle decoupling):
--   * pending_metadata_updates rows are ephemeral — created when a scan
--     kicks off an async metadata fetch and resolved (resolved_at set)
--     within seconds, then soft-deleted and auto-purged.
--   * sessions rows — anonymous ones especially — are hard-purged after
--     7 days of inactivity by tasks::anonymous_session_purge, on a cadence
--     entirely independent of pending-update resolution.
--   * An FK with ON DELETE RESTRICT would let a stale pending row block a
--     session purge; ON DELETE CASCADE would couple two independently
--     managed lifecycles for no integrity gain. The PendingUpdates
--     middleware already treats a token with no matching session as a
--     benign no-op (it simply delivers no OOB update), so a dangling token
--     is not a data-integrity hazard.
--
-- We record the decision as a column COMMENT so it stays visible in
-- `SHOW CREATE TABLE pending_metadata_updates` rather than living only in
-- a closed GitHub issue.
ALTER TABLE pending_metadata_updates
    MODIFY session_token VARCHAR(44) NOT NULL
    COMMENT 'No FK to sessions.token by design (#21): ephemeral pending row vs independently-purged session; a dangling token is a no-op in the PendingUpdates middleware.';
