//! Story 19-6 (#657, NFR49, AC5) — the backup round trip: seed every
//! table (UTF-8, binary, JSON, NULLs, the generated columns, the Trash,
//! covers), back up, load `database.sql` into a fresh schema on the same
//! server, extract the covers, then compare every table (`CHECKSUM
//! TABLE` + row count) and every cover byte by byte. A table added to the
//! schema without being dumped fails the comparison of the table sets.
//!
//! The fresh schema is `mybibli_rt_<random>`; it is dropped even when an
//! assertion fails (the body runs under `catch_unwind`).

use crate::common;

use std::collections::BTreeSet;

use common::backup::{Folders, assert_members_match, manifest_of, read_members};
use futures::FutureExt;
use mybibli::db::DbPool;
use mybibli::services::backup::write_backup;
use sqlx::{MySqlPool, Row};

/// Rows of every kind, every table non-empty.
const SEED: &str = r#"
INSERT INTO users (username, password_hash, role) VALUES ('rt_admin', '$argon2id$rt', 'admin');
SET @u = LAST_INSERT_ID();
INSERT INTO api_keys (label, key_hash, key_prefix, scope, created_by) VALUES ('rt key', 'hash', 'mbk_rt', 'propose', @u);
SET @k = LAST_INSERT_ID();
INSERT INTO genres (name) VALUES ('RT genre 🎭');
SET @g = LAST_INSERT_ID();
INSERT INTO titles (title, media_type, genre_id, publication_date, page_count, manually_edited_fields)
  VALUES ('Été 🌞 l''ombre', 'book', @g, '2020-02-29', 321, '["title","description"]');
SET @t = LAST_INSERT_ID();
INSERT INTO titles (title, media_type, genre_id, deleted_at) VALUES ('RT trashed title', 'bd', @g, '2026-01-02 03:04:05');
INSERT INTO contributors (name) VALUES ('Zoé Ålund');
SET @c = LAST_INSERT_ID();
INSERT INTO contributors (name, deleted_at) VALUES ('Zoé Ålund', '2026-03-04 05:06:07');
INSERT INTO contributor_roles (name) VALUES ('RT role');
SET @r = LAST_INSERT_ID();
INSERT INTO title_contributors (title_id, contributor_id, role_id) VALUES (@t, @c, @r);
INSERT INTO labels (name, color) VALUES ('RT label', 'teal');
SET @l = LAST_INSERT_ID();
INSERT INTO title_labels (title_id, label_id) VALUES (@t, @l);
INSERT INTO location_node_types (name) VALUES ('RT shelf');
INSERT INTO storage_locations (name, node_type, label) VALUES ('RT room', 'RT shelf', 'L9871');
SET @loc = LAST_INSERT_ID();
INSERT INTO volume_states (name, is_loanable) VALUES ('RT state', 1);
SET @vs = LAST_INSERT_ID();
INSERT INTO volumes (title_id, label, condition_state_id, location_id, purchase_price, purchase_currency,
                     current_value, current_value_currency, current_value_updated_at)
  VALUES (@t, 'V9871', @vs, @loc, 12.30, 'CHF', 0.05, 'CHF', '2026-11-25 10:00:00');
SET @v = LAST_INSERT_ID();
INSERT INTO volume_labels (volume_id, label_id) VALUES (@v, @l);
INSERT INTO borrowers (name, address) VALUES ('RT borrower', NULL);
SET @b = LAST_INSERT_ID();
INSERT INTO borrowers (name, email) VALUES ('RT borrower 2', NULL);
INSERT INTO loans (volume_id, borrower_id, loaned_at, previous_location_id) VALUES (@v, @b, '2026-11-01 08:00:00', @loc);
INSERT INTO series (name, series_type, total_volume_count) VALUES ('RT series', 'closed', 3);
SET @s = LAST_INSERT_ID();
INSERT INTO title_series (title_id, series_id, position_number) VALUES (@t, @s, 1);
INSERT INTO series_expected_tomes (series_id, position, title, isbn) VALUES (@s, 2, 'Tome 2 — à venir', NULL);
INSERT INTO series_expected_tomes (series_id, position, title, deleted_at) VALUES (@s, 3, 'Tome 3', '2026-05-05 05:05:05');
INSERT INTO metadata_cache (code, response) VALUES ('9780000000001', '{"title":"\\u00e9t\\u00e9 \\"q\\" 🎉","n":null}');
INSERT INTO metadata_fetch_retries (title_id, attempts, next_attempt_at) VALUES (@t, 2, '2026-12-01 00:00:00');
INSERT INTO pending_metadata_updates (title_id, session_token) VALUES (@t, 'rt-token');
INSERT INTO proposals (kind, api_key_id, sources, equivalence_key) VALUES ('metadata', @k, '[{"url":"https://example.org/é"}]', 'metadata:titles/1:x');
SET @p = LAST_INSERT_ID();
INSERT INTO proposal_items (proposal_id, target_table, target_id, target_version, proposed_values, original_values)
  VALUES (@p, 'titles', @t, 1, '{"line":"title","value":"Nouveau 🌙"}', NULL);
INSERT INTO saved_searches (name, q) VALUES ('RT search', 'été');
INSERT INTO sessions (token, user_id, data, csrf_token) VALUES ('rt-session-token', @u, '{"k":"v"}', 'csrf');
INSERT INTO mcp_skills (name, description, body, active, updated_by) VALUES ('rt-skill', 'One line', '# Body', 1, @u);
INSERT INTO wishlist_items (title, publication_year) VALUES ('RT wish', 1999);
INSERT INTO admin_audit (user_id, action, entity_type, entity_id, details) VALUES (@u, 'rt_action', 'titles', @t, '{"a":[1,2,{"b":"ü"}]}');
INSERT INTO cover_uploads (token, api_key_id, staged_file, media_type, original_bytes, width, height, bytes, fingerprint)
  VALUES ('0123456789abcdef0123456789abcdef', @k, '0123456789abcdef0123456789abcdef.jpg', 'image/jpeg', 10, 1, 1, 10, REPEAT('a', 64));
"#;

/// Text that QUOTE() must carry exactly: quotes, backslashes, NUL,
/// Control-Z, line ends, tabs, astral characters.
const TRICKY: &str = "Line 1\nLine 2\r\n\\ 'single' \"double\" \0 nul \x1a ctrl-z \t tab — ✓ 𝄞 😀";

async fn table_names(pool: &MySqlPool, schema: &str) -> BTreeSet<String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT CAST(TABLE_NAME AS CHAR) FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'",
    )
    .bind(schema)
    .fetch_all(pool)
    .await
    .unwrap();
    rows.into_iter().map(|(n,)| n).collect()
}

async fn checksum(pool: &MySqlPool, schema: &str, table: &str) -> (u64, i64) {
    let row = sqlx::query(&format!("CHECKSUM TABLE `{schema}`.`{table}`"))
        .fetch_one(pool)
        .await
        .unwrap();
    let sum = row
        .try_get::<Option<u64>, _>(1)
        .or_else(|_| row.try_get::<Option<i64>, _>(1).map(|v| v.map(|x| x as u64)))
        .unwrap()
        .unwrap_or_else(|| panic!("no checksum for {schema}.{table}"));
    let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM `{schema}`.`{table}`"))
        .fetch_one(pool)
        .await
        .unwrap();
    (sum, count)
}

/// `DATABASE_URL` with its database replaced.
fn url_for(schema: &str) -> String {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b.to_string(), format!("?{q}")),
        None => (url.clone(), String::new()),
    };
    let (server, _) = base.rsplit_once('/').expect("a database in DATABASE_URL");
    format!("{server}/{schema}{query}")
}

#[sqlx::test(migrations = "./migrations")]
async fn a_backup_loads_back_into_an_identical_database(pool: DbPool) {
    let target = format!("mybibli_rt_{}", mybibli::services::instance_id::generate());
    sqlx::query(&format!("CREATE DATABASE `{target}` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci"))
        .execute(&pool)
        .await
        .unwrap();
    let outcome = std::panic::AssertUnwindSafe(round_trip(&pool, &target))
        .catch_unwind()
        .await;
    sqlx::query(&format!("DROP DATABASE IF EXISTS `{target}`"))
        .execute(&pool)
        .await
        .unwrap();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn round_trip(pool: &DbPool, target: &str) {
    // 1. Seed.
    let mut conn = pool.acquire().await.unwrap();
    sqlx::raw_sql(SEED).execute(&mut *conn).await.unwrap();
    drop(conn);
    sqlx::query("UPDATE borrowers SET address = ? WHERE name = 'RT borrower'")
        .bind(TRICKY)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE titles SET description = ? WHERE title = 'RT trashed title'")
        .bind(TRICKY)
        .execute(pool)
        .await
        .unwrap();
    let source: String = sqlx::query_scalar("SELECT DATABASE()").fetch_one(pool).await.unwrap();
    let f = Folders::new();
    let cover_a: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
    let cover_b: Vec<u8> = vec![0xFF, 0xD8, 0x00, 0x27, 0x5C, 0x0A, 0xFF, 0xD9];
    std::fs::write(f.covers().join("1.jpg"), &cover_a).unwrap();
    std::fs::write(f.covers().join("42.jpg"), &cover_b).unwrap();
    std::fs::create_dir(f.covers().join(".staging")).unwrap();
    std::fs::write(f.covers().join(".staging/0123456789abcdef0123456789abcdef.jpg"), b"x").unwrap();

    // 2. Back up (this also creates the instance id, so `settings` and
    //    `data_repairs` hold it too).
    let written = write_backup(pool, &f.config()).await.expect("backup");

    let source_tables = table_names(pool, &source).await;
    let mut empty = Vec::new();
    for t in &source_tables {
        if checksum(pool, &source, t).await.1 == 0 {
            empty.push(t.clone());
        }
    }
    assert!(empty.is_empty(), "every table is seeded: {empty:?} are empty");

    // 3. Load database.sql into the fresh schema, extract the covers.
    let members = read_members(&f.backups().join(&written.file_name));
    let manifest = manifest_of(&members);
    assert_members_match(&members, &manifest);
    let sql = String::from_utf8(members[0].1.clone()).expect("database.sql is UTF-8");
    let target_pool = MySqlPool::connect(&url_for(target)).await.unwrap();
    sqlx::raw_sql(&sql).execute(&target_pool).await.expect("database.sql loads");
    target_pool.close().await;
    let extracted = f.root.path().join("extracted");
    std::fs::create_dir(&extracted).unwrap();
    for (name, body) in &members[1..members.len() - 1] {
        let file = name.strip_prefix("covers/").expect("only covers between");
        std::fs::write(extracted.join(file), body).unwrap();
    }

    // 4. Compare: the same tables (none forgotten), each identical.
    let manifest_tables: BTreeSet<String> = manifest.tables.keys().cloned().collect();
    assert_eq!(manifest_tables, source_tables, "every table of the schema is dumped");
    assert_eq!(table_names(pool, target).await, source_tables, "every table is loaded");
    for t in &source_tables {
        let src = checksum(pool, &source, t).await;
        let dst = checksum(pool, target, t).await;
        assert_eq!(src, dst, "table {t}: (checksum, rows) differ after the round trip");
        assert_eq!(manifest.tables.get(t), Some(&(src.1 as u64)), "manifest row count of {t}");
    }
    // The generated columns were recomputed by the server.
    let live: Vec<(Option<String>,)> = sqlx::query_as(&format!(
        "SELECT name_live FROM `{target}`.contributors WHERE name = 'Zoé Ålund' ORDER BY id"
    ))
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(live, vec![(Some("Zoé Ålund".to_string()),), (None,)]);
    let tricky: String = sqlx::query_scalar(&format!(
        "SELECT address FROM `{target}`.borrowers WHERE name = 'RT borrower'"
    ))
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(tricky, TRICKY);

    // Covers: the same files, byte for byte; nothing from .staging.
    let mut names: Vec<String> = std::fs::read_dir(&extracted)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["1.jpg", "42.jpg"]);
    assert_eq!(std::fs::read(extracted.join("1.jpg")).unwrap(), cover_a);
    assert_eq!(std::fs::read(extracted.join("42.jpg")).unwrap(), cover_b);
}
