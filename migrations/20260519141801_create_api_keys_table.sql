-- CR #241: api_keys table.
--
-- Single-tenant LAN deployment exposes a JSON HTTP API for external
-- callers (an AI classifier, custom scripts). Auth is per-key:
-- argon2 hash of a random suffix, plaintext shown once at creation.
-- `scope` is a coarse two-level enum (read / write) — write implies
-- read at the route layer.
--
-- Key plaintext format: `mybibli_<scope_short>_<random40>`
--   - scope_short = "ro" for read, "rw" for write.
--   - random40 = 40 chars from a URL-safe charset.
-- Total length ≈ 50 chars, ≈ 240 bits of entropy in the random part.

CREATE TABLE IF NOT EXISTS api_keys (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
  label VARCHAR(120) NOT NULL COMMENT 'Free-form admin-facing name (e.g. "Claude classifier")',
  key_hash VARCHAR(255) NOT NULL COMMENT 'Argon2 hash of the plaintext key (matches users.password_hash sizing)',
  key_prefix VARCHAR(20) NOT NULL COMMENT 'First 12 chars of the plaintext (e.g. "mybibli_ro_a") for admin-UI display',
  scope ENUM('read', 'write') NOT NULL,
  created_by BIGINT UNSIGNED NULL COMMENT 'Admin who issued the key; NULL after the admin row gets purged',
  last_used_at DATETIME NULL,
  revoked_at DATETIME NULL COMMENT 'Soft-revoke; key no longer valid but the row stays for audit',
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  deleted_at DATETIME NULL COMMENT 'Hard-delete via Trash (separate from revoked_at)',
  version INT NOT NULL DEFAULT 0,

  FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE SET NULL,
  KEY idx_api_keys_active (deleted_at, revoked_at),
  KEY idx_api_keys_prefix (key_prefix)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
