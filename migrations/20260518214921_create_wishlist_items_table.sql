-- CR #242: wishlist_items table.
--
-- Separate from `titles` on purpose: a wishlist entry tolerates partial
-- metadata (free-form title with no ISBN, no cover, no author) and stays
-- out of the catalog counters / soft-delete / audit flows of the owned
-- collection. The auto-link on catalog scan joins on `isbn`.

CREATE TABLE IF NOT EXISTS wishlist_items (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
  isbn VARCHAR(20) NULL COMMENT 'EAN-13 / ISBN-10 / ISBN-13 — nullable for free-form entries',
  title VARCHAR(500) NOT NULL,
  authors TEXT NULL COMMENT 'Free-form joined string; promotion to a proper join table is a future CR',
  publisher VARCHAR(200) NULL,
  publication_year SMALLINT UNSIGNED NULL,
  cover_image_url VARCHAR(500) NULL COMMENT 'Local path /covers-wishlist/N.jpg or NULL when no cover',
  notes TEXT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  deleted_at DATETIME NULL,
  version INT NOT NULL DEFAULT 0,

  KEY idx_wishlist_items_isbn (isbn),
  KEY idx_wishlist_items_deleted_at (deleted_at),
  KEY idx_wishlist_items_created_at (created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
