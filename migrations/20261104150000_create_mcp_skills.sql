-- Story 15-2 (#614, FR162) — MCP skills: instructions an administrator
-- writes once in mybibli for the agent to read (story 15-3 serves the
-- active ones over MCP). Admin-only editing; no version history — the
-- row holds the current text plus the date and author of the last change.
--
-- Name reuse: `name` is UNIQUE across ALL rows, trashed ones included.
-- Creating (or renaming to) a name held by a trashed skill is refused:
-- the admin restores that skill from the Trash or picks another name. A
-- trashed skill is never overwritten, and restoring it from the Trash can
-- never collide with a live one.
--
-- `name` is a lowercase slug (`^[a-z0-9]+(-[a-z0-9]+)*$`, checked by
-- `services::mcp_skills`). No charset / collation is pinned on purpose:
-- like `titles`, `series`, `borrowers`…, the table inherits the database
-- default, because the Trash panel UNIONs every trashable table's name
-- column and a mismatched collation there fails the UNION ("illegal mix
-- of collations") or makes the Trash search case-sensitive.

CREATE TABLE mcp_skills (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(64) NOT NULL,
    -- One line shown to the agent in the skill list; no newline.
    description VARCHAR(200) NOT NULL,
    -- Markdown, at most 64 KiB (checked by the service). Never rendered
    -- as HTML by the web UI: it is displayed escaped, as text.
    body MEDIUMTEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    -- Author of the last change. SET NULL so a purged user never blocks
    -- the row; the date stays in `updated_at`.
    updated_by BIGINT UNSIGNED NULL DEFAULT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    UNIQUE KEY uq_mcp_skills_name (name),
    INDEX idx_mcp_skills_deleted_at (deleted_at),
    CONSTRAINT fk_mcp_skills_updated_by FOREIGN KEY (updated_by) REFERENCES users (id)
        ON DELETE SET NULL
);
