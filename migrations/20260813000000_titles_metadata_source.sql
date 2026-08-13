-- Issue #202 — record WHICH provider resolved a title's metadata.
--
-- The provider chain has always known this at the moment of success
-- (`chain.rs` logs `provider = …` on "Provider returned result") and then
-- discarded it: neither `metadata_cache`, nor `titles`, nor `MetadataResult`
-- carried the provenance. A librarian looking at a thin or empty record had no
-- way to tell "BnF answered but holds little" from "nothing answered at all" —
-- which is the question behind the original report on #202.
--
-- NULL is meaningful and is the correct value for every pre-existing row: it
-- reads as "unknown provenance", not "no provider". Rows cataloged before this
-- migration genuinely have no recorded source, and back-filling them would
-- require re-running the chain — an explicit admin action (the bulk metadata
-- backfill), not a migration side effect.
--
-- VARCHAR(32) fits every registered provider name with room to spare; the
-- longest today is "Library of Congress" at 19 characters. The value stored is
-- `MetadataProvider::name()` verbatim, NOT a display string and NOT a
-- translated one (NFR41): presentation is `metadata_source_display_name()`.
ALTER TABLE titles ADD COLUMN metadata_source VARCHAR(32) NULL AFTER cover_image_url;

-- Supports the "titles with no recorded source" scan an admin surface would
-- run; also keeps the column cheap to filter on when the follow-up stories
-- (#202-b structured failure surface, #202-c per-provider retry) land.
CREATE INDEX idx_titles_metadata_source ON titles (metadata_source);
