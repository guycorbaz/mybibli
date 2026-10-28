-- Story 11-10 (FR129, FR141, FR145, AR29): expected tomes of a series.
--
-- A tome the library does not own yet: its position in the series, its
-- title and, optionally, its ISBN (always stored as ISBN-13 — an
-- ISBN-10 is converted by `services::expected_tomes`). Entered by hand
-- (story 11-11, origin `manual`) or by accepting an agent proposal
-- (stories 11-12 / 11-17, origin `proposal`).
--
-- When a title carrying that ISBN is catalogued, the title is attached
-- to the series at that position and the tome is "settled"
-- (`settled_title_id`). A tome counts as settled only while that title
-- is live (D-11-10-3).
--
--   * `series_id` ON DELETE CASCADE: the 30-day auto-purge (or a
--     Trash permanent delete) of a series takes its tomes along.
--   * `settled_title_id` ON DELETE SET NULL: purging a title never
--     blocks on a tome; the tome simply reads as unsettled again.
--   * `live_marker` (1 when live, NULL once soft-deleted — never
--     written by the app) backs the two unique keys: at most one live
--     tome per position and per ISBN in a series, while soft-deleted
--     rows never collide (NULLs are distinct in a UNIQUE index).
--
-- AR24 quartet (checked by tests/seed_drift.rs).

CREATE TABLE series_expected_tomes (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    series_id BIGINT UNSIGNED NOT NULL,
    position INT NOT NULL,
    title VARCHAR(500) NOT NULL,
    isbn VARCHAR(13) NULL DEFAULT NULL,
    origin ENUM('manual', 'proposal') NOT NULL DEFAULT 'manual',
    settled_title_id BIGINT UNSIGNED NULL DEFAULT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    live_marker TINYINT AS (IF(deleted_at IS NULL, 1, NULL)) STORED,
    UNIQUE KEY uq_expected_tomes_position (series_id, position, live_marker),
    UNIQUE KEY uq_expected_tomes_isbn (series_id, isbn, live_marker),
    INDEX idx_expected_tomes_isbn (isbn),
    INDEX idx_expected_tomes_settled_title (settled_title_id),
    INDEX idx_expected_tomes_deleted_at (deleted_at),
    CONSTRAINT fk_expected_tomes_series FOREIGN KEY (series_id) REFERENCES series (id)
        ON DELETE CASCADE,
    CONSTRAINT fk_expected_tomes_settled_title FOREIGN KEY (settled_title_id) REFERENCES titles (id)
        ON DELETE SET NULL,
    CONSTRAINT chk_expected_tomes_position CHECK (position >= 1)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
