-- Story 11-19 (FR139, FR146): a librarian edits or removes the lines of
-- a pending proposal before deciding it; the agent's values stay on
-- record next to the edited ones.
--
-- `proposal_items.original_values` — the item's `proposed_values` as the
-- agent filed it, copied by the first edit or by the removal of the
-- line; NULL while the line is as filed. Read with
-- `CAST(original_values AS CHAR)`.
-- `edited_by_name` / `edited_at` — who last edited or removed the line
-- and when (username copied at that time, like
-- `proposals.decided_by_name`: no FK, so nothing blocks the purge of a
-- user row).
--
-- A removed line is a soft-deleted item (`deleted_at` set): acceptance,
-- staleness, the subject and the item count already read live items
-- only. `proposal_items` is not in the soft-delete purge whitelist, so a
-- removed line stays on record with its proposal.

ALTER TABLE proposal_items
    ADD COLUMN original_values JSON NULL DEFAULT NULL AFTER proposed_values,
    ADD COLUMN edited_by_name VARCHAR(255) NULL DEFAULT NULL AFTER original_values,
    ADD COLUMN edited_at TIMESTAMP NULL DEFAULT NULL AFTER edited_by_name;
