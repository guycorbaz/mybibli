-- Story 17-6 (#656, FR177) — default MCP skills shipped with mybibli.
--
-- `provided_by_mybibli` is set only when startup inserts a default skill
-- (`services::default_skills`), and cleared by every admin write (edit,
-- activation, deactivation): it means "the last change was mybibli's own".
-- Admin → Skills then reads "provided by mybibli" instead of "author
-- unknown" (`updated_by` is NULL for such a row).

ALTER TABLE mcp_skills
    ADD COLUMN provided_by_mybibli BOOLEAN NOT NULL DEFAULT FALSE AFTER updated_by;
