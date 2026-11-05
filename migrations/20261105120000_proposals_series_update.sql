-- Story 15-4 (#590, FR164) — an agent proposes to modify an existing
-- series: name, open / closed type, total, removal of expected tomes.
--
-- * `proposals.kind` gains `series_update` (existing values first, in
--   their original order).
-- * No new target table: the update line targets `series/<id>`; removal
--   lines are untargeted and carry the tome id + version in their JSON
--   (D1 of the story).
--
-- Item shape: module doc of `services::series_update_proposals`.

ALTER TABLE proposals
    MODIFY kind ENUM('series_create', 'series_attach', 'metadata', 'cover', 'label_attach', 'series_update') NOT NULL;
