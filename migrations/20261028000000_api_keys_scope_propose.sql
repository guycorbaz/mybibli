-- Story 11-5 (FR126, NFR44): the `propose` API-key scope.
--
-- A `propose` key reads everything and may create proposals for a
-- librarian to review (MCP agent access, `/api/mcp` from story 11-6);
-- it never writes catalogue data directly. Plaintext prefix:
-- `mybibli_pr_…` (12-char `key_prefix` unchanged in length).
--
-- The value is appended to the ENUM, so every existing row keeps its
-- stored `read` / `write` scope.

ALTER TABLE api_keys
  MODIFY COLUMN scope ENUM('read', 'write', 'propose') NOT NULL;
