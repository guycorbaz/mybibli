//! #374 / #30 sub-item 3 — Seed-drift audit.
//!
//! Replays every `migrations/*.sql` (including seed migrations) against a
//! fresh test database via `#[sqlx::test(migrations = "./migrations")]`
//! and checks the resulting schema + seed-row state for the invariants
//! that fresh installs depend on at first boot:
//!
//!   1. every entity table carries the soft-delete trio
//!      (`deleted_at`, `version`, `created_at`, `updated_at`);
//!   2. required seed rows are present (genres, contributor_roles with
//!      exactly one `is_primary = TRUE`, location_node_types);
//!   3. the SYSTEM user (`role = 'system'`) exists — audit attribution
//!      falls back if it doesn't (issue #68);
//!   4. no orphan FK in seeded `title_contributors`;
//!   5. `settings` is non-empty so the K/V cache has at least one slot.
//!
//! Failure mode this catches: a later migration adds a NOT NULL column
//! to a table that a seed row already populated → seed rows now have
//! NULLs (or the migration itself fails). Today such a regression
//! breaks fresh installs at first-boot; this audit surfaces it at PR
//! time.

use sqlx::Row;

const ENTITY_TABLES_WITH_SOFT_DELETE: &[&str] = &[
    "titles",
    "volumes",
    "borrowers",
    "loans",
    "contributors",
    "storage_locations",
    "series",
];

#[sqlx::test(migrations = "./migrations")]
async fn seed_drift_audit_passes_on_fresh_db(pool: sqlx::Pool<sqlx::MySql>) {
    // Check 1 — soft-delete trio columns on every entity table.
    for tbl in ENTITY_TABLES_WITH_SOFT_DELETE {
        let row = sqlx::query(
            "SELECT COUNT(*) AS n FROM information_schema.columns \
             WHERE table_schema = DATABASE() AND table_name = ? \
               AND column_name IN ('deleted_at', 'version', 'created_at', 'updated_at')",
        )
        .bind(tbl)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| panic!("information_schema query failed for {tbl}: {e}"));
        let n: i64 = row.try_get("n").unwrap();
        assert_eq!(
            n, 4,
            "table {tbl} must carry the soft-delete trio columns (deleted_at, version, created_at, updated_at); found {n}/4",
        );
    }

    // Check 2 — required seed rows.
    let role_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM contributor_roles WHERE deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .try_get("n")
    .unwrap();
    assert!(
        role_count >= 1,
        "contributor_roles must seed at least one row (got {role_count})",
    );

    let primary_role_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM contributor_roles \
         WHERE deleted_at IS NULL AND is_primary = TRUE",
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .try_get("n")
    .unwrap();
    assert_eq!(
        primary_role_count, 1,
        "exactly one contributor_role must carry is_primary = TRUE (found {primary_role_count}) \
         — the metadata-fetch chain depends on a single canonical primary role; see #19 + #371",
    );

    let genre_count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM genres WHERE deleted_at IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert!(
        genre_count >= 1,
        "genres must seed at least one row (got {genre_count}) — the default genre is required",
    );

    let loc_type_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM location_node_types WHERE deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .try_get("n")
    .unwrap();
    assert!(
        loc_type_count >= 1,
        "location_node_types must seed at least one row (got {loc_type_count})",
    );

    // Check 3 — SYSTEM user (issue #68).
    let system_count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM users WHERE role = 'system'")
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(
        system_count, 1,
        "users must contain exactly one role='system' row (found {system_count}) — \
         audit-attribution falls back if missing; see #68",
    );

    // Check 4 — no orphan FK in seeded title_contributors.
    let orphan_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM title_contributors tc \
         LEFT JOIN contributor_roles cr ON cr.id = tc.role_id AND cr.deleted_at IS NULL \
         WHERE tc.deleted_at IS NULL AND cr.id IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .try_get("n")
    .unwrap();
    assert_eq!(
        orphan_count, 0,
        "title_contributors carries {orphan_count} orphan rows (role_id missing or soft-deleted)",
    );

    // Check 5 — settings table is allocated.
    let settings_count: i64 = sqlx::query(
        "SELECT COUNT(DISTINCT setting_key) AS n FROM settings WHERE deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap()
    .try_get("n")
    .unwrap();
    assert!(
        settings_count >= 1,
        "settings must seed at least one row (got {settings_count})",
    );
}
