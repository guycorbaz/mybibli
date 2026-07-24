-- #389 Palier 1 — UNIMARC internal-model conformity.
--
-- Additive migration: adds nullable columns to `titles` for UNIMARC zones the
-- flat schema did not yet capture explicitly. Existing rows are preserved
-- unchanged (every column is NULLable, no backfill inside the migration — the
-- BnF re-fetch backfill runs at the application layer, observable via #434 logs).
--
-- Zone reference (see docs/unimarc-mapping.md for the full mapping table):
--   200$f/$g  statement of responsibility  (already read by src/metadata/bnf.rs, previously dropped)
--   205$a     edition statement
--   225$a     collection (publisher's series) title — distinct from the work-level `series` table
--   225$v     collection numbering
--   300$a     general note
--   454/500   original / uniform title (useful for translations)

ALTER TABLE titles
    ADD COLUMN statement_of_responsibility VARCHAR(1000) NULL AFTER subtitle,
    ADD COLUMN edition_statement          VARCHAR(255)  NULL AFTER statement_of_responsibility,
    ADD COLUMN collection_title           VARCHAR(500)  NULL AFTER publisher,
    ADD COLUMN collection_number          VARCHAR(50)   NULL AFTER collection_title,
    ADD COLUMN general_note               TEXT          NULL AFTER description,
    ADD COLUMN original_title             VARCHAR(500)  NULL AFTER title;
