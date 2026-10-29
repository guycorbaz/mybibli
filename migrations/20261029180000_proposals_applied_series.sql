-- Story 11-17 (FR138, FR146): accepting a `series_create` proposal
-- creates a series; its id is recorded here so the proposals page can
-- link the series the decision produced (the proposal's own items name
-- no target for a creation). NULL for every other proposal.
--
-- No foreign key: series are hard-purged 30 days after a soft delete
-- (story 8-7) and a proposal must never block that purge; a purged id
-- reads as "no longer in the catalog".

ALTER TABLE proposals
    ADD COLUMN applied_series_id BIGINT UNSIGNED NULL DEFAULT NULL AFTER rejection_reason;
