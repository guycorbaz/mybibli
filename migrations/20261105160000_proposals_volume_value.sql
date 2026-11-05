-- Story 15-5 (#564, FR165) — an agent proposes what a volume is worth:
-- its current value and, when the volume has no purchase price, the
-- publisher's list price at publication as a labelled stand-in.
--
-- * `proposals.kind` gains `volume_value` (existing values first, in
--   their original order).
-- * Items target `volumes/<id>` (the target table of story 13-9).
--
-- Item shape: module doc of `services::volume_value_proposals`.

ALTER TABLE proposals
    MODIFY kind ENUM('series_create', 'series_attach', 'metadata', 'cover', 'label_attach', 'series_update', 'volume_value') NOT NULL;
