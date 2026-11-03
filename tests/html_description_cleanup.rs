//! Story 13-4 (#578) — descriptions stored with the providers' HTML are
//! converted to plain text at startup.
//!
//! DB integration tests of the data step
//! (`services::title::convert_html_descriptions`).
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test html_description_cleanup

use mybibli::services::title::{DescriptionCleanup, convert_html_descriptions};
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

#[sqlx::test(migrations = "./migrations")]
async fn the_cleanup_is_idempotent(pool: MySqlPool) {
    let id = title_with(&pool, "Titeuf", Some(TITEUF), None).await;
    // Its plain text still holds `&` and `;`: selected again next run.
    let amp = title_with(&pool, "Amp", Some("<p>Tom &amp; Jerry; R&amp;D</p>"), None).await;
    let first = convert_html_descriptions(&pool).await.expect("first run");
    assert_eq!(first.converted, 2);
    let after_first = (
        description_of(&pool, id).await,
        description_of(&pool, amp).await,
    );
    assert_eq!(after_first.1.0.as_deref(), Some("Tom & Jerry; R&D"));

    let second = convert_html_descriptions(&pool).await.expect("second run");
    assert_eq!(
        second,
        DescriptionCleanup {
            unchanged: 1,
            ..DescriptionCleanup::default()
        }
    );
    assert_eq!(
        (
            description_of(&pool, id).await,
            description_of(&pool, amp).await
        ),
        after_first,
        "no write, no version bump"
    );
}
