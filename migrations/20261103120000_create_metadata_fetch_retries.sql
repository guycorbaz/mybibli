-- Story 13-2 (#583): retry queue for metadata fetches that ended throttled.
--
-- A scan-time (or manual re-download) metadata fetch whose provider chain
-- came back empty-handed because a provider answered 429/503 is recorded
-- here; `tasks::metadata_retry` looks the title up again later, following
-- the back-off schedule in that module.
--
-- `pending_metadata_updates` cannot serve as the queue: it is a per-session
-- delivery row, soft-deleted by `middleware::pending_updates` once its
-- outcome reaches the browser.
--
--   * one row per title at most (`uq_metadata_fetch_retries_title`). A
--     dequeued row is soft-deleted; re-enqueueing the same title revives it
--     (`INSERT ... ON DUPLICATE KEY UPDATE`) with a fresh schedule.
--   * `attempts` counts the background retries already run for the current
--     enqueue (0 = only the original fetch was throttled).
--   * `title_id` ON DELETE CASCADE: the 30-day auto-purge (or a Trash
--     permanent delete) of a title takes its queue row along.
--
-- AR24 quartet (checked by tests/seed_drift.rs).

CREATE TABLE metadata_fetch_retries (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    title_id BIGINT UNSIGNED NOT NULL,
    attempts INT NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    UNIQUE KEY uq_metadata_fetch_retries_title (title_id),
    INDEX idx_metadata_fetch_retries_due (deleted_at, next_attempt_at),
    CONSTRAINT fk_metadata_fetch_retries_title FOREIGN KEY (title_id) REFERENCES titles (id)
        ON DELETE CASCADE,
    CONSTRAINT chk_metadata_fetch_retries_attempts CHECK (attempts >= 0)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
