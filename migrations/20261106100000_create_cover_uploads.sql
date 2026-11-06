-- Story 16-4 (#587, FR169) — a cover image an agent uploaded as a file, kept
-- until it files a proposal naming the upload's token.
--
-- A short-lived technical row, not a catalog entity (like `sessions` it has
-- no soft-delete quartet): consumed by the filing that names it, deleted by
-- the staging purge once expired (`services::cover_uploads::UPLOAD_TTL`). No
-- foreign key to `api_keys`: a revoked key can no longer authenticate, its
-- rows expire by themselves, and the auto-purge order stays untouched.

CREATE TABLE cover_uploads (
    token CHAR(32) NOT NULL PRIMARY KEY,
    api_key_id BIGINT UNSIGNED NOT NULL,
    staged_file CHAR(36) NOT NULL,
    media_type VARCHAR(32) NOT NULL,
    original_bytes INT NOT NULL,
    width INT NOT NULL,
    height INT NOT NULL,
    bytes INT NOT NULL,
    fingerprint CHAR(64) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    KEY idx_cover_uploads_key (api_key_id),
    KEY idx_cover_uploads_created (created_at)
);
