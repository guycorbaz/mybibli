-- Story 11-9 (FR134, FR140, NFR47, AR29): agent proposals.
--
-- An AI agent holding a `propose` API key (story 11-5) never writes to
-- the catalogue: everything it suggests is filed here and decided by a
-- Librarian or Admin on the proposals page (stories 11-16..11-19).
--
-- `proposals` — one row per submission.
--   * `kind`: what is proposed (series creation, attachment to an
--     existing series, title metadata, cover).
--   * `status`: `pending` until decided; only `pending` → `accepted` /
--     `rejected` / `stale` is allowed (enforced in
--     `services::proposals`), and nothing leaves a final state.
--   * `api_key_id`: the filing key (FR134). API keys are never
--     hard-deleted (soft `deleted_at` / `revoked_at`), so a plain FK.
--   * `sources`: JSON array of http(s) URLs, validated at filing
--     (NFR47). Read with `CAST(sources AS CHAR)`.
--   * `decided_by` / `decided_at` / `rejection_reason`: the decision
--     trail (FR146). `decided_by` is SET NULL when the user row is
--     hard-purged by the 30-day auto-purge (story 8-7, issue #69) —
--     otherwise a decision would block that purge; `decided_by_name`
--     keeps the decider's username for the trail. A `stale` transition
--     records `decided_at` and no user.
--
-- `proposal_items` — one line per action of a proposal.
--   * `target_table` / `target_id` / `target_version`: the existing
--     entity the line acts on and its `version` at filing time, for
--     staleness detection (FR140). All three NULL for a line that
--     creates something new (the series of a `series_create`, an
--     expected tome) — enforced by `chk_proposal_items_target`.
--     No FK on `target_id`: the target is polymorphic, and titles /
--     series are hard-purged 30 days after a soft delete — a purged
--     target simply reads as "deleted", hence stale.
--   * `proposed_values`: JSON object whose shape depends on the
--     proposal kind (defined by stories 11-12..11-14). Agent data is
--     untrusted and is never rendered as HTML (NFR47).
--
-- Both tables carry the AR24 quartet (checked by tests/seed_drift.rs).

CREATE TABLE proposals (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    kind ENUM('series_create', 'series_attach', 'metadata', 'cover') NOT NULL,
    status ENUM('pending', 'accepted', 'rejected', 'stale') NOT NULL DEFAULT 'pending',
    api_key_id BIGINT UNSIGNED NOT NULL,
    sources JSON NOT NULL,
    decided_by BIGINT UNSIGNED NULL DEFAULT NULL,
    -- Username of the decider copied at decision time: the FR146 trail
    -- survives the purge of the user row (decided_by is then SET NULL).
    decided_by_name VARCHAR(255) NULL DEFAULT NULL,
    decided_at TIMESTAMP NULL DEFAULT NULL,
    rejection_reason TEXT NULL DEFAULT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_proposals_status (status, deleted_at),
    INDEX idx_proposals_api_key (api_key_id),
    INDEX idx_proposals_deleted_at (deleted_at),
    INDEX idx_proposals_listing (deleted_at, created_at, id),
    CONSTRAINT fk_proposals_api_key FOREIGN KEY (api_key_id) REFERENCES api_keys (id),
    CONSTRAINT fk_proposals_decided_by FOREIGN KEY (decided_by) REFERENCES users (id)
        ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE proposal_items (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    proposal_id BIGINT UNSIGNED NOT NULL,
    target_table ENUM('titles', 'series') NULL DEFAULT NULL,
    target_id BIGINT UNSIGNED NULL DEFAULT NULL,
    target_version INT NULL DEFAULT NULL,
    proposed_values JSON NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_proposal_items_proposal (proposal_id),
    INDEX idx_proposal_items_target (target_table, target_id),
    INDEX idx_proposal_items_deleted_at (deleted_at),
    CONSTRAINT fk_proposal_items_proposal FOREIGN KEY (proposal_id) REFERENCES proposals (id),
    CONSTRAINT chk_proposal_items_target CHECK (
        (target_table IS NULL AND target_id IS NULL AND target_version IS NULL)
        OR (target_table IS NOT NULL AND target_id IS NOT NULL AND target_version IS NOT NULL)
    )
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
