-- Story 8-7: Create admin_audit table for audit trail
-- Records who deleted what and when for compliance + debugging

CREATE TABLE IF NOT EXISTS admin_audit (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
  user_id BIGINT UNSIGNED NOT NULL,
  action VARCHAR(64) NOT NULL COMMENT 'e.g., "permanent_delete_from_trash", "auto_purge"',
  entity_type VARCHAR(64) COMMENT 'Table name (nullable for system actions like auto_purge)',
  entity_id BIGINT UNSIGNED COMMENT 'ID of deleted row (nullable for system actions)',
  timestamp DATETIME NOT NULL DEFAULT NOW(),
  details JSON COMMENT 'Optional metadata: item name, affected rows, etc.',

  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
  KEY idx_user_timestamp (user_id, timestamp),
  KEY idx_action_timestamp (action, timestamp)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
