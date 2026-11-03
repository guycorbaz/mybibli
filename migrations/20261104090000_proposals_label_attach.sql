-- Story 13-9 (#581, FR156) — an agent proposes attaching one of the
-- library's labels (CR #443) to a title or a volume.
--
-- * `proposals.kind` gains `label_attach` (existing values first, in
--   their original order).
-- * `proposal_items.target_table` gains `volumes`: a label proposal
--   targets the title or the volume it marks, recording its `version`
--   like every targeted line (a deleted volume makes the proposal stale).
--
-- Item shape: module doc of `services::label_proposals`.

ALTER TABLE proposals
    MODIFY kind ENUM('series_create', 'series_attach', 'metadata', 'cover', 'label_attach') NOT NULL;

ALTER TABLE proposal_items
    MODIFY target_table ENUM('titles', 'series', 'volumes') NULL DEFAULT NULL;
