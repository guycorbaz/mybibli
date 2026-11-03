//! Story 13-4 (#578) — descriptions stored with the providers' HTML are
//! converted to plain text at startup.
//!
//! DB integration tests of the data step
//! (`services::title::convert_html_descriptions`) and of its run-once
//! wrapper (`repair_html_descriptions_once`, `data_repairs` marker).
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test html_description_cleanup

use mybibli::services::title::{
    DescriptionCleanup, HTML_DESCRIPTIONS_REPAIR, convert_html_descriptions,
    repair_html_descriptions_once,
};
use sqlx::MySqlPool;

async fn title_with(
    pool: &MySqlPool,
    name: &str,
    description: Option<&str>,
    edited: Option<&str>,
) -> u64 {
    let genre_id: u64 =
        sqlx::query_scalar("SELECT id FROM genres WHERE deleted_at IS NULL LIMIT 1")
            .fetch_one(pool)
            .await
            .expect("a seeded genre");
    sqlx::query(
        "INSERT INTO titles (title, media_type, genre_id, language, description, manually_edited_fields) \
         VALUES (?, 'bd', ?, 'fr', ?, ?)",
    )
    .bind(name)
    .bind(genre_id)
    .bind(description)
    .bind(edited)
    .execute(pool)
    .await
    .expect("insert title")
    .last_insert_id()
}

/// `(description, version)` of a title, deleted or not.
async fn description_of(pool: &MySqlPool, id: u64) -> (Option<String>, i32) {
    sqlx::query_as("SELECT description, version FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read title")
}

const TITEUF: &str = "<p><b>Grandis un peu, Titeuf !</b></p><p>La vie de Titeuf &eacute;volue.</p>";

#[sqlx::test(migrations = "./migrations")]
async fn html_descriptions_become_plain_text(pool: MySqlPool) {
    let html = title_with(&pool, "Titeuf", Some(TITEUF), None).await;
    let markup_only = title_with(&pool, "Empty", Some("<p><br></p>"), None).await;
    let entity_only = title_with(&pool, "Entity", Some("Tintin &amp; Milou"), None).await;
    let prose = title_with(&pool, "Prose", Some("a < b"), None).await;
    let plain = title_with(&pool, "Plain", Some("Nothing to do."), None).await;
    let none = title_with(&pool, "None", None, None).await;

    let (_, html_version) = description_of(&pool, html).await;
    let report = convert_html_descriptions(&pool).await.expect("convert");
    assert_eq!(
        report,
        DescriptionCleanup {
            converted: 3,
            unchanged: 1,
            ..DescriptionCleanup::default()
        }
    );

    let (desc, version) = description_of(&pool, html).await;
    assert_eq!(
        desc.as_deref(),
        Some("Grandis un peu, Titeuf !\n\nLa vie de Titeuf évolue.")
    );
    assert_eq!(version, html_version + 1, "optimistic-lock version bumped");
    assert_eq!(description_of(&pool, markup_only).await.0, None);
    assert_eq!(
        description_of(&pool, entity_only).await.0.as_deref(),
        Some("Tintin & Milou")
    );
    assert_eq!(
        description_of(&pool, prose).await.0.as_deref(),
        Some("a < b")
    );
    assert_eq!(
        description_of(&pool, plain).await,
        (Some("Nothing to do.".into()), 1)
    );
    assert_eq!(description_of(&pool, none).await.0, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_hand_edited_description_is_never_rewritten(pool: MySqlPool) {
    let edited = title_with(&pool, "Edited", Some(TITEUF), Some(r#"["description"]"#)).await;
    let other_field = title_with(&pool, "Other", Some(TITEUF), Some(r#"["publisher"]"#)).await;

    let report = convert_html_descriptions(&pool).await.expect("convert");
    assert_eq!(report.hand_edited, 1);
    assert_eq!(report.converted, 1);
    assert_eq!(
        description_of(&pool, edited).await,
        (Some(TITEUF.into()), 1)
    );
    assert_ne!(
        description_of(&pool, other_field).await.0.as_deref(),
        Some(TITEUF)
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_title_is_left_alone(pool: MySqlPool) {
    let deleted = title_with(&pool, "Deleted", Some(TITEUF), None).await;
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(deleted)
        .execute(&pool)
        .await
        .unwrap();
    let report = convert_html_descriptions(&pool).await.expect("convert");
    assert_eq!(report, DescriptionCleanup::default());
    assert_eq!(
        description_of(&pool, deleted).await.0.as_deref(),
        Some(TITEUF)
    );
}

/// Review M1 — the repair runs once per database: a description already
/// converted to `<b>x</b>` (from `&lt;b&gt;x&lt;/b&gt;`) is healthy text
/// the conversion would rewrite, so the second boot must not touch it.
#[sqlx::test(migrations = "./migrations")]
async fn the_repair_runs_once_and_leaves_converted_text_alone(pool: MySqlPool) {
    let id = title_with(&pool, "Escaped", Some("&lt;b&gt;x&lt;/b&gt;"), None).await;

    let first = repair_html_descriptions_once(&pool)
        .await
        .expect("first boot")
        .expect("a fresh database runs the repair");
    assert_eq!(first.converted, 1);
    let after_first = description_of(&pool, id).await;
    assert_eq!(after_first.0.as_deref(), Some("<b>x</b>"));

    for boot in 2..=3 {
        let again = repair_html_descriptions_once(&pool).await.expect("boot");
        assert_eq!(again, None, "boot {boot}: the marker is present, no-op");
        assert_eq!(
            description_of(&pool, id).await,
            after_first,
            "boot {boot}: no write, no version bump"
        );
    }
    let markers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM data_repairs WHERE name = ?")
        .bind(HTML_DESCRIPTIONS_REPAIR)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(markers, 1);
}

/// Cache rows cached before the providers converted their HTML are purged;
/// a plain one stays.
#[sqlx::test(migrations = "./migrations")]
async fn cache_rows_with_markup_descriptions_are_purged(pool: MySqlPool) {
    for (code, description) in [
        ("9780000000101", "<p>Cached <b>HTML</b></p>"),
        ("9780000000102", "Caf&eacute;"),
        ("9780000000103", "Plain cached text"),
    ] {
        sqlx::query("INSERT INTO metadata_cache (code, response) VALUES (?, ?)")
            .bind(code)
            .bind(serde_json::json!({"title": "T", "description": description}))
            .execute(&pool)
            .await
            .unwrap();
    }
    let report = convert_html_descriptions(&pool).await.expect("convert");
    assert_eq!(report.cache_purged, 2);
    let live: Vec<String> = sqlx::query_scalar(
        "SELECT code FROM metadata_cache WHERE deleted_at IS NULL ORDER BY code",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(live, vec!["9780000000103".to_string()]);
}
