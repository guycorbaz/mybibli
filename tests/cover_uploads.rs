//! Story 16-4 (#587, FR169) — an agent uploads a cover as a file, then names
//! it in `propose_cover`.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test cover_uploads

mod common;

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::mcp::*;
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use mybibli::models::cover_upload::CoverUploadModel;
use mybibli::services::cover::MAX_PROPOSED_COVER_BYTES;
use mybibli::services::cover_uploads::{MAX_LIVE_UPLOADS_PER_KEY, UPLOAD_TTL};
use serde_json::{Value, json};
use tower::ServiceExt;

const SRC: &str = "https://example.org/cover";
const BOUNDARY: &str = "XBOUNDARYX";

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(w, h, image::Rgb([40, 90, 160]))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn multipart(part_name: &str, content_type: Option<&str>, bytes: &[u8]) -> Vec<u8> {
    let mut body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{part_name}\"; filename=\"c.png\"\r\n"
    )
    .into_bytes();
    if let Some(t) = content_type {
        body.extend_from_slice(format!("Content-Type: {t}\r\n").as_bytes());
    }
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn post_upload(app: &Router, key: Option<&str>, body: Vec<u8>) -> (StatusCode, Value) {
    let mut req = Request::post("/api/v1/cover-uploads")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        );
    if let Some(key) = key {
        req = req.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

struct Fx {
    _dir: tempfile::TempDir,
    staging: std::path::PathBuf,
    app: Router,
    key: String,
}

async fn fx(pool: &DbPool) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let covers = dir.path().to_path_buf();
    let staging = mybibli::services::cover_staging::staging_dir_for(&covers);
    std::fs::create_dir_all(&staging).unwrap();
    Fx {
        app: router_with_covers(pool, &covers),
        staging,
        _dir: dir,
        key: seed_key(pool, ApiKeyScope::Propose).await,
    }
}

async fn upload(f: &Fx) -> Value {
    let (status, doc) = post_upload(
        &f.app,
        Some(&f.key),
        multipart("file", Some("image/png"), &png(600, 900)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{doc}");
    doc
}

async fn propose(app: &Router, key: &str, args: Value) -> Value {
    send_json(app, modern_call(key, "propose_cover", args)).await["result"].clone()
}

fn staged(f: &Fx) -> usize {
    std::fs::read_dir(&f.staging).unwrap().count()
}

async fn live_uploads(pool: &DbPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM cover_uploads")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn an_upload_is_described_then_filed_once(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Téléversé").await;
    let image = png(600, 900);
    let (status, doc) = post_upload(
        &f.app,
        Some(&f.key),
        multipart("file", Some("image/png"), &image),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{doc}");
    // What was received, before the re-encode.
    assert_eq!((doc["width"].as_u64(), doc["height"].as_u64()), (Some(600), Some(900)));
    assert_eq!(doc["bytes"].as_u64(), Some(image.len() as u64));
    assert_eq!(
        doc["sha256"].as_str().unwrap(),
        mybibli::services::cover_proposals::cover_fingerprint(&image)
    );
    assert_eq!(doc["media_type"], "image/png");
    assert!(doc["expires_at"].as_str().unwrap().ends_with('Z'));
    assert_eq!(staged(&f), 1);
    let token = doc["upload_token"].as_str().unwrap();

    let result = propose(
        &f.app,
        &f.key,
        json!({"title_id": title, "upload_token": token, "sources": [SRC]}),
    )
    .await;
    assert_eq!(result["isError"], json!(false), "{result}");
    let id = result["structuredContent"]["proposal_id"].as_u64().unwrap();
    assert_eq!(live_uploads(&pool).await, 0, "the token is consumed");
    // The item is the staged, re-encoded file, as for a base64 proposal.
    let (values,): (String,) = sqlx::query_as(
        "SELECT CAST(proposed_values AS CHAR) FROM proposal_items WHERE proposal_id = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let values: Value = serde_json::from_str(&values).unwrap();
    assert_eq!(values["media_type"], "image/png");
    assert_eq!(values["original_bytes"].as_u64(), Some(image.len() as u64));
    assert_eq!(values["width"].as_u64(), Some(400));
    assert!(f.staging.join(values["staged_file"].as_str().unwrap()).is_file());

    // Single use.
    let again = propose(
        &f.app,
        &f.key,
        json!({"title_id": title, "upload_token": token, "sources": [SRC]}),
    )
    .await;
    assert_eq!(again["isError"], json!(true));
    assert!(again.to_string().contains("no usable upload"), "{again}");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_type_may_be_left_to_the_signature_but_never_contradict_it(pool: DbPool) {
    let f = fx(&pool).await;
    let image = png(10, 10);
    for ct in [None, Some("application/octet-stream")] {
        let (status, doc) =
            post_upload(&f.app, Some(&f.key), multipart("file", ct, &image)).await;
        assert_eq!(status, StatusCode::CREATED, "{doc}");
        assert_eq!(doc["media_type"], "image/png");
    }
    let before = staged(&f);
    let (status, doc) =
        post_upload(&f.app, Some(&f.key), multipart("file", Some("image/jpeg"), &image)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(doc["reason"].as_str().unwrap().contains("image/png"), "{doc}");
    assert_eq!(staged(&f), before, "nothing kept");
}

#[sqlx::test(migrations = "./migrations")]
async fn damaged_or_missing_files_are_refused_and_nothing_is_kept(pool: DbPool) {
    let f = fx(&pool).await;
    let mut truncated = png(50, 50);
    truncated.truncate(truncated.len() / 2);
    let too_large = {
        let mut v = png(10, 10);
        v.resize(MAX_PROPOSED_COVER_BYTES + 1, 0);
        v
    };
    let cases: Vec<(Vec<u8>, StatusCode, &str)> = vec![
        (multipart("file", None, b""), StatusCode::BAD_REQUEST, "empty"),
        (multipart("file", None, b"not an image"), StatusCode::BAD_REQUEST, "signature"),
        (multipart("file", None, &truncated), StatusCode::BAD_REQUEST, "cannot be read"),
        (multipart("other", None, &png(5, 5)), StatusCode::BAD_REQUEST, "`file`"),
        (b"garbage".to_vec(), StatusCode::BAD_REQUEST, "multipart"),
        (multipart("file", None, &too_large), StatusCode::PAYLOAD_TOO_LARGE, "larger"),
    ];
    for (body, status, needle) in cases {
        let (got, doc) = post_upload(&f.app, Some(&f.key), body).await;
        assert_eq!(got, status, "{needle}: {doc}");
        assert!(doc["reason"].as_str().unwrap().contains(needle), "{needle}: {doc}");
    }
    assert_eq!(staged(&f), 0);
    assert_eq!(live_uploads(&pool).await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn only_a_propose_key_uploads_and_only_its_own_token_counts(pool: DbPool) {
    let f = fx(&pool).await;
    let body = || multipart("file", None, &png(5, 5));
    let (status, _) = post_upload(&f.app, None, body()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    for scope in [ApiKeyScope::Read, ApiKeyScope::Write] {
        let key = seed_key(&pool, scope).await;
        let (status, _) = post_upload(&f.app, Some(&key), body()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    // An upload leaves no anonymous session behind.
    let doc = upload(&f).await;
    let (sessions,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions WHERE user_id IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions, 0);

    let token = doc["upload_token"].as_str().unwrap();
    let title = seed_title(&pool, "Clé étrangère").await;
    let other = seed_key(&pool, ApiKeyScope::Propose).await;
    let args = json!({"title_id": title, "upload_token": token, "sources": [SRC]});
    let result = propose(&f.app, &other, args.clone()).await;
    assert_eq!(result["isError"], json!(true));
    assert!(result.to_string().contains("no usable upload"), "{result}");
    // The owner's token is untouched.
    let result = propose(&f.app, &f.key, args).await;
    assert_eq!(result["isError"], json!(false), "{result}");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_refused_filing_keeps_the_upload_and_an_expired_one_is_gone(pool: DbPool) {
    let f = fx(&pool).await;
    let doc = upload(&f).await;
    let token = doc["upload_token"].as_str().unwrap();
    let title = seed_title(&pool, "Réessai").await;

    let refused = propose(
        &f.app,
        &f.key,
        json!({"title_id": 999_999, "upload_token": token, "sources": [SRC]}),
    )
    .await;
    assert_eq!(refused["isError"], json!(true));
    assert_eq!(live_uploads(&pool).await, 1, "still usable after a refusal");

    // Expired: refused, and the staging purge removes the row.
    sqlx::query("UPDATE cover_uploads SET created_at = NOW() - INTERVAL ? SECOND")
        .bind(UPLOAD_TTL.as_secs() + 1)
        .execute(&pool)
        .await
        .unwrap();
    let expired = propose(
        &f.app,
        &f.key,
        json!({"title_id": title, "upload_token": token, "sources": [SRC]}),
    )
    .await;
    assert_eq!(expired["isError"], json!(true));
    assert!(expired.to_string().contains("no usable upload"), "{expired}");
    mybibli::services::cover_staging::purge(&pool, &f.staging).await.unwrap();
    assert_eq!(live_uploads(&pool).await, 0);
    // The file waits for the grace, like any unreferenced staged file.
    assert_eq!(staged(&f), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_key_holds_a_bounded_number_of_uploads(pool: DbPool) {
    let f = fx(&pool).await;
    let key_id: u64 = sqlx::query_scalar("SELECT id FROM api_keys ORDER BY id DESC LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    for i in 0..MAX_LIVE_UPLOADS_PER_KEY {
        sqlx::query(
            "INSERT INTO cover_uploads (token, api_key_id, staged_file, media_type, \
             original_bytes, width, height, bytes, fingerprint) \
             VALUES (?, ?, ?, 'image/png', 1, 1, 1, 1, ?)",
        )
        .bind(format!("{i:032x}"))
        .bind(key_id)
        .bind(format!("{i:032x}.jpg"))
        .bind(format!("{i:064x}"))
        .execute(&pool)
        .await
        .unwrap();
    }
    let (status, doc) = post_upload(&f.app, Some(&f.key), multipart("file", None, &png(5, 5))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(doc["reason"].as_str().unwrap().contains("at most"), "{doc}");
    assert_eq!(staged(&f), 0, "nothing kept");
    // Another key is not affected.
    let other = seed_key(&pool, ApiKeyScope::Propose).await;
    let (status, _) = post_upload(&f.app, Some(&other), multipart("file", None, &png(5, 5))).await;
    assert_eq!(status, StatusCode::CREATED);
}

// Rule #22 — consuming one token locks that row only.
#[sqlx::test(migrations = "./migrations")]
async fn locking_one_upload_does_not_block_another(pool: DbPool) {
    for (token, key) in [("a".repeat(32), 1_u64), ("b".repeat(32), 2_u64)] {
        sqlx::query(
            "INSERT INTO cover_uploads (token, api_key_id, staged_file, media_type, \
             original_bytes, width, height, bytes, fingerprint) \
             VALUES (?, ?, ?, 'image/png', 1, 1, 1, 1, ?)",
        )
        .bind(&token)
        .bind(key)
        .bind(format!("{token}.jpg"))
        .bind("f".repeat(64))
        .execute(&pool)
        .await
        .unwrap();
    }
    let ttl = UPLOAD_TTL.as_secs();
    let mut first = pool.begin().await.unwrap();
    assert!(CoverUploadModel::lock_live_in(&mut first, &"a".repeat(32), 1, ttl).await.unwrap());
    let mut second = pool.begin().await.unwrap();
    let locked = tokio::time::timeout(
        Duration::from_secs(5),
        CoverUploadModel::lock_live_in(&mut second, &"b".repeat(32), 2, ttl),
    )
    .await
    .expect("another upload must not wait on this lock");
    assert!(locked.unwrap());
    second.rollback().await.unwrap();
    // A second filing on the same token waits, then finds it consumed.
    CoverUploadModel::delete_in(&mut first, &"a".repeat(32)).await.unwrap();
    first.commit().await.unwrap();
    let mut third = pool.begin().await.unwrap();
    assert!(!CoverUploadModel::lock_live_in(&mut third, &"a".repeat(32), 1, ttl).await.unwrap());
}

/// A valid PNG of about `size` bytes (noise does not compress).
fn large_png(size: usize) -> Vec<u8> {
    let side = ((size / 3) as f64).sqrt().ceil() as u32;
    let mut state = 17_u32;
    let pixels = image::RgbImage::from_fn(side, side, |_, _| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let b = state.to_le_bytes();
        image::Rgb([b[3], b[2], b[1]])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    pixels.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

// The route's own limit, above axum's default 2 MiB.
#[sqlx::test(migrations = "./migrations")]
async fn the_largest_image_fits_and_a_larger_body_does_not(pool: DbPool) {
    let f = fx(&pool).await;
    // Exactly the largest image (trailing bytes after the PNG end): the body
    // is above axum's default 2 MiB limit, so this proves the route's own.
    let mut image = large_png(MAX_PROPOSED_COVER_BYTES - 40_000);
    assert!(image.len() <= MAX_PROPOSED_COVER_BYTES, "{}", image.len());
    image.resize(MAX_PROPOSED_COVER_BYTES, 0);
    let body = multipart("file", Some("image/png"), &image);
    assert!(body.len() > 2 * 1024 * 1024);
    let (status, doc) = post_upload(&f.app, Some(&f.key), body).await;
    assert_eq!(status, StatusCode::CREATED, "{doc}");
    image.resize(MAX_PROPOSED_COVER_BYTES + 128 * 1024, 0);
    let (status, _) = post_upload(&f.app, Some(&f.key), multipart("file", None, &image)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

fn age_file(path: &std::path::Path, by: Duration) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(std::time::SystemTime::now() - by).unwrap();
}

// Lifecycle: an expired upload leaves with the next upload of its key, or
// with the purge once past the grace; a live one stays.
#[sqlx::test(migrations = "./migrations")]
async fn expired_uploads_leave_the_disk(pool: DbPool) {
    use mybibli::services::cover_staging::{PURGE_GRACE, purge};
    let f = fx(&pool).await;
    upload(&f).await;
    assert_eq!(staged(&f), 1);
    sqlx::query("UPDATE cover_uploads SET created_at = NOW() - INTERVAL ? SECOND")
        .bind(UPLOAD_TTL.as_secs() + 1)
        .execute(&pool)
        .await
        .unwrap();
    // The next upload of the key replaces the expired row and file.
    upload(&f).await;
    assert_eq!(staged(&f), 1);
    assert_eq!(live_uploads(&pool).await, 1);

    let name = std::fs::read_dir(&f.staging).unwrap().next().unwrap().unwrap().path();
    age_file(&name, PURGE_GRACE + Duration::from_secs(60));
    // Expired and past the grace: the purge removes the row and the file.
    sqlx::query("UPDATE cover_uploads SET created_at = NOW() - INTERVAL ? SECOND")
        .bind(UPLOAD_TTL.as_secs() + 1)
        .execute(&pool)
        .await
        .unwrap();
    purge(&pool, &f.staging).await.unwrap();
    assert_eq!(live_uploads(&pool).await, 0);
    assert_eq!(staged(&f), 0, "past the grace, the file is gone");
}

// A proposal filed by token follows the proposal lifecycle: rejected, its
// staged file is discarded; a refused filing keeps the token for a retry.
#[sqlx::test(migrations = "./migrations")]
async fn a_rejected_upload_is_discarded_and_the_same_image_is_blocked(pool: DbPool) {
    use common::web::{Browser, seed_admin};
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Rejet par jeton").await;
    let token = |d: &Value| d["upload_token"].as_str().unwrap().to_string();
    let first = upload(&f).await;
    let result = propose(
        &f.app,
        &f.key,
        json!({"title_id": title, "upload_token": token(&first), "sources": [SRC]}),
    )
    .await;
    assert_eq!(result["isError"], json!(false), "{result}");
    let id = result["structuredContent"]["proposal_id"].as_u64().unwrap();

    seed_admin(&pool, "cu-admin").await;
    let librarian = Browser::login(&f.app, "cu-admin").await;
    let (version,): (i32,) = sqlx::query_as("SELECT version FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (status, _) = librarian
        .post_form(&f.app, &format!("/proposals/{id}/reject"), &format!("version={version}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(staged(&f), 0, "the staged file is discarded");

    // The same image again: blocked, and the token stays usable.
    let second = upload(&f).await;
    let refused = propose(
        &f.app,
        &f.key,
        json!({"title_id": title, "upload_token": token(&second), "sources": [SRC]}),
    )
    .await;
    assert_eq!(refused["isError"], json!(true));
    assert!(refused.to_string().contains("was rejected on"), "{refused}");
    assert_eq!(live_uploads(&pool).await, 1);
}

// A client that disconnects: the job holds on past the request, its file and
// row arrive together. The key row is held so the request is dropped exactly
// between the staging and the recording, the old orphan window.
#[sqlx::test(migrations = "./migrations")]
async fn a_dropped_request_leaves_no_orphan_file(pool: DbPool) {
    let f = fx(&pool).await;
    let key_id: u64 = sqlx::query_scalar("SELECT id FROM api_keys ORDER BY id DESC LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut lock = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM api_keys WHERE id = ? FOR UPDATE")
        .bind(key_id)
        .fetch_one(&mut *lock)
        .await
        .unwrap();

    let image = png(200, 300);
    let staging = f.staging.clone();
    let request = {
        let pool = pool.clone();
        tokio::spawn(async move {
            mybibli::services::cover_uploads::receive(&pool, Some(&staging), key_id, image, None).await
        })
    };
    // Wait for the file: the job is now blocked on the key row.
    let mut stored = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if std::fs::read_dir(&f.staging)
            .unwrap()
            .any(|e| e.unwrap().path().extension().is_some_and(|x| x == "jpg"))
        {
            stored = true;
            break;
        }
    }
    assert!(stored, "the job staged its file");
    request.abort();
    let _ = request.await;
    lock.rollback().await.unwrap();

    // The job goes on without its request: one file, one row.
    let mut settled = false;
    for _ in 0..100 {
        if live_uploads(&pool).await == 1 {
            settled = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(settled, "the job recorded its upload");
    assert_eq!(staged(&f), 1);
}
