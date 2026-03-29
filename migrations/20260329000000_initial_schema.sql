-- Initial schema for mybibli
-- All tables use utf8mb4 charset (set at server level)
-- Common columns on every entity table: id, created_at, updated_at, deleted_at, version

-- Reference tables (admin-configurable lists)

CREATE TABLE genres (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_genres_deleted_at (deleted_at)
);

CREATE TABLE volume_states (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    is_loanable BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_volume_states_deleted_at (deleted_at)
);

CREATE TABLE contributor_roles (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_contributor_roles_deleted_at (deleted_at)
);

CREATE TABLE location_node_types (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_location_node_types_deleted_at (deleted_at)
);

-- Entity tables

CREATE TABLE titles (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    title VARCHAR(500) NOT NULL,
    subtitle VARCHAR(500) NULL,
    description TEXT NULL,
    language VARCHAR(10) NOT NULL DEFAULT 'fr',
    media_type ENUM('book', 'bd', 'cd', 'dvd', 'magazine', 'report') NOT NULL,
    publication_date DATE NULL,
    publisher VARCHAR(255) NULL,
    isbn VARCHAR(13) NULL,
    issn VARCHAR(8) NULL,
    upc VARCHAR(13) NULL,
    cover_image_url VARCHAR(1000) NULL,
    genre_id BIGINT UNSIGNED NOT NULL,
    dewey_code VARCHAR(15) NULL,
    page_count INT NULL,
    track_count INT NULL,
    total_duration INT NULL,
    age_rating VARCHAR(10) NULL,
    issue_number INT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_titles_deleted_at (deleted_at),
    INDEX idx_titles_genre_id (genre_id),
    INDEX idx_titles_media_type (media_type),
    CONSTRAINT fk_titles_genre FOREIGN KEY (genre_id) REFERENCES genres(id)
);

CREATE TABLE contributors (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    biography TEXT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_contributors_deleted_at (deleted_at)
);

CREATE TABLE title_contributors (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    title_id BIGINT UNSIGNED NOT NULL,
    contributor_id BIGINT UNSIGNED NOT NULL,
    role_id BIGINT UNSIGNED NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    UNIQUE KEY uq_title_contributor_role (title_id, contributor_id, role_id),
    INDEX idx_title_contributors_deleted_at (deleted_at),
    INDEX idx_title_contributors_title (title_id),
    INDEX idx_title_contributors_contributor (contributor_id),
    CONSTRAINT fk_tc_title FOREIGN KEY (title_id) REFERENCES titles(id),
    CONSTRAINT fk_tc_contributor FOREIGN KEY (contributor_id) REFERENCES contributors(id),
    CONSTRAINT fk_tc_role FOREIGN KEY (role_id) REFERENCES contributor_roles(id)
);

CREATE TABLE storage_locations (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    parent_id BIGINT UNSIGNED NULL,
    name VARCHAR(255) NOT NULL,
    node_type VARCHAR(50) NOT NULL,
    label CHAR(5) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    UNIQUE KEY uq_storage_locations_label (label),
    INDEX idx_storage_locations_deleted_at (deleted_at),
    INDEX idx_storage_locations_parent (parent_id),
    CONSTRAINT fk_storage_locations_parent FOREIGN KEY (parent_id) REFERENCES storage_locations(id)
);

CREATE TABLE volumes (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    title_id BIGINT UNSIGNED NOT NULL,
    label CHAR(5) NOT NULL,
    condition_state_id BIGINT UNSIGNED NULL,
    edition_comment VARCHAR(255) NULL,
    location_id BIGINT UNSIGNED NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    UNIQUE KEY uq_volumes_label (label),
    INDEX idx_volumes_deleted_at (deleted_at),
    INDEX idx_volumes_title (title_id),
    INDEX idx_volumes_location (location_id),
    CONSTRAINT fk_volumes_title FOREIGN KEY (title_id) REFERENCES titles(id),
    CONSTRAINT fk_volumes_condition FOREIGN KEY (condition_state_id) REFERENCES volume_states(id),
    CONSTRAINT fk_volumes_location FOREIGN KEY (location_id) REFERENCES storage_locations(id)
);

CREATE TABLE borrowers (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    address TEXT NULL,
    email VARCHAR(255) NULL,
    phone VARCHAR(50) NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_borrowers_deleted_at (deleted_at)
);

CREATE TABLE loans (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    volume_id BIGINT UNSIGNED NOT NULL,
    borrower_id BIGINT UNSIGNED NOT NULL,
    loaned_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    returned_at TIMESTAMP NULL,
    previous_location_id BIGINT UNSIGNED NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_loans_deleted_at (deleted_at),
    INDEX idx_loans_volume (volume_id),
    INDEX idx_loans_borrower (borrower_id),
    CONSTRAINT fk_loans_volume FOREIGN KEY (volume_id) REFERENCES volumes(id),
    CONSTRAINT fk_loans_borrower FOREIGN KEY (borrower_id) REFERENCES borrowers(id),
    CONSTRAINT fk_loans_prev_location FOREIGN KEY (previous_location_id) REFERENCES storage_locations(id)
);

CREATE TABLE series (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT NULL,
    series_type ENUM('open', 'closed') NOT NULL DEFAULT 'open',
    total_volume_count INT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_series_deleted_at (deleted_at)
);

CREATE TABLE title_series (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    title_id BIGINT UNSIGNED NOT NULL,
    series_id BIGINT UNSIGNED NOT NULL,
    position_number INT NOT NULL,
    is_omnibus BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    UNIQUE KEY uq_title_series_position (title_id, series_id, position_number),
    INDEX idx_title_series_deleted_at (deleted_at),
    INDEX idx_title_series_title (title_id),
    INDEX idx_title_series_series (series_id),
    CONSTRAINT fk_ts_title FOREIGN KEY (title_id) REFERENCES titles(id),
    CONSTRAINT fk_ts_series FOREIGN KEY (series_id) REFERENCES series(id)
);

CREATE TABLE users (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    username VARCHAR(255) NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    role ENUM('librarian', 'admin') NOT NULL DEFAULT 'librarian',
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    UNIQUE KEY uq_users_username (username),
    INDEX idx_users_deleted_at (deleted_at)
);

CREATE TABLE sessions (
    token VARCHAR(44) NOT NULL PRIMARY KEY,
    user_id BIGINT UNSIGNED NULL,
    data JSON NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    last_activity TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_sessions_user (user_id),
    INDEX idx_sessions_last_activity (last_activity),
    INDEX idx_sessions_deleted_at (deleted_at),
    CONSTRAINT fk_sessions_user FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE TABLE metadata_cache (
    code VARCHAR(13) NOT NULL PRIMARY KEY,
    response JSON NOT NULL,
    fetched_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_metadata_cache_deleted_at (deleted_at)
);

CREATE TABLE pending_metadata_updates (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    title_id BIGINT UNSIGNED NOT NULL,
    session_token VARCHAR(44) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    resolved_at TIMESTAMP NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_pending_session_resolved (session_token, resolved_at),
    INDEX idx_pending_metadata_updates_deleted_at (deleted_at),
    CONSTRAINT fk_pending_title FOREIGN KEY (title_id) REFERENCES titles(id)
);

CREATE TABLE settings (
    setting_key VARCHAR(255) NOT NULL PRIMARY KEY,
    setting_value TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_settings_deleted_at (deleted_at)
);

-- Seed default settings
INSERT INTO settings (setting_key, setting_value) VALUES
    ('overdue_loan_threshold_days', '30'),
    ('session_inactivity_timeout_hours', '4'),
    ('session_warning_minutes_before_expiry', '5'),
    ('scanner_burst_threshold_ms', '50'),
    ('search_debounce_delay_ms', '300'),
    ('metadata_fetch_timeout_seconds', '30');
