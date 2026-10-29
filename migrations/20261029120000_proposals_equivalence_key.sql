-- Story 11-15 (FR136, AR30): resubmission delay for rejected proposals.
--
-- `proposals.equivalence_key` — what makes two proposals "the same
-- request": the kind plus its target, built in Rust at filing
-- (`services::proposal_history::ProposalSubject::equivalence_key`):
--   series_create:<folded series name>
--   series_attach:series/<id>
--   metadata:titles/<id>:<sorted field keys, comma-separated>
--   cover:titles/<id>
-- Binary collation: the Rust side already normalises (compatibility
-- forms, case, accents, whitespace), so the comparison must not depend on the server charset.
-- NULL for a proposal whose items match none of these shapes, and for
-- rows filed before this migration (proposals are unreleased): they
-- never block a filing.
--
-- `proposal_resubmission_delay_days` — the admin-configurable delay
-- (Admin → System, story 8-5 pattern). 0..=3650, 0 disables the rule;
-- an invalid stored value reads as the default 90.

ALTER TABLE proposals
    ADD COLUMN equivalence_key VARCHAR(700) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NULL DEFAULT NULL
        AFTER sources,
    ADD INDEX idx_proposals_equivalence (equivalence_key, status, decided_at);

INSERT INTO settings (setting_key, setting_value, version) VALUES
('proposal_resubmission_delay_days', '90', 1)
ON DUPLICATE KEY UPDATE setting_key = setting_key;
