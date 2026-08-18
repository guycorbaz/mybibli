-- CR #443 — management labels on titles and volumes.
--
-- A fifth admin-editable taxonomy beside genres / volume_states /
-- contributor_roles / location_node_types, but with a shape none of them
-- have: labels are MANY-TO-MANY and apply to two different entity kinds
-- from one shared vocabulary. "À vérifier" must be attachable to a title
-- whose metadata looks wrong AND to a volume whose V-code binding is
-- doubtful, without duplicating the vocabulary.
--
-- Deliberately NOT modelled as two separate vocabularies: the requester
-- asked for one list to maintain, and a librarian who renames a label
-- expects the rename to hold everywhere it is used.

CREATE TABLE labels (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    -- COLLATE pinned for the same reason as the 2026-04-27 migration on the
    -- other four taxonomies: without it the column inherits a per-deployment
    -- default, and a case-sensitive one lets "À vérifier" and "à vérifier"
    -- coexist as two rows the admin cannot tell apart.
    name VARCHAR(255) COLLATE utf8mb4_unicode_ci NOT NULL UNIQUE,
    -- Optional presentation hint, free-form so the UI can render a chip.
    -- Nullable: a label is useful without one, and forcing a choice at
    -- creation time would slow down the flow this feature exists to speed up.
    color VARCHAR(32) NULL DEFAULT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_labels_deleted_at (deleted_at)
);

-- Join tables. Composite UNIQUE prevents the same label being attached
-- twice to the same entity — a double click must not create a duplicate
-- the user then has to remove twice.
--
-- Both carry the soft-delete quartet like every other entity table, so the
-- auto-purge whitelist (services/soft_delete.rs) can order them before
-- their parents and Trash semantics stay uniform.

CREATE TABLE title_labels (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    title_id BIGINT UNSIGNED NOT NULL,
    label_id BIGINT UNSIGNED NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    UNIQUE KEY uq_title_labels (title_id, label_id),
    INDEX idx_title_labels_deleted_at (deleted_at),
    INDEX idx_title_labels_label (label_id),
    CONSTRAINT fk_title_labels_title FOREIGN KEY (title_id) REFERENCES titles (id),
    CONSTRAINT fk_title_labels_label FOREIGN KEY (label_id) REFERENCES labels (id)
);

CREATE TABLE volume_labels (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    volume_id BIGINT UNSIGNED NOT NULL,
    label_id BIGINT UNSIGNED NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    UNIQUE KEY uq_volume_labels (volume_id, label_id),
    INDEX idx_volume_labels_deleted_at (deleted_at),
    INDEX idx_volume_labels_label (label_id),
    CONSTRAINT fk_volume_labels_volume FOREIGN KEY (volume_id) REFERENCES volumes (id),
    CONSTRAINT fk_volume_labels_label FOREIGN KEY (label_id) REFERENCES labels (id)
);
