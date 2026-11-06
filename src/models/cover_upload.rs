//! Story 16-4 (#587, FR169) — `cover_uploads`: a staged cover waiting for the
//! `propose_cover` call that names its token. Lifecycle and rules live in
//! `services::cover_uploads`.

use sqlx::{MySqlConnection, Row};

use crate::db::DbPool;
use crate::error::AppError;

/// One upload. Everything `propose_cover` needs to build its item without
/// reading the image again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverUpload {
    pub token: String,
    pub staged_file: String,
    pub media_type: String,
    pub original_bytes: i64,
    pub width: i64,
    pub height: i64,
    pub bytes: i64,
    pub fingerprint: String,
}

const COLUMNS: &str = "token, staged_file, media_type, CAST(original_bytes AS SIGNED) AS original_bytes, \
     CAST(width AS SIGNED) AS width, CAST(height AS SIGNED) AS height, \
     CAST(bytes AS SIGNED) AS bytes, fingerprint";

fn from_row(r: &sqlx::mysql::MySqlRow) -> Result<CoverUpload, sqlx::Error> {
    Ok(CoverUpload {
        token: r.try_get("token")?,
        staged_file: r.try_get("staged_file")?,
        media_type: r.try_get("media_type")?,
        original_bytes: r.try_get("original_bytes")?,
        width: r.try_get("width")?,
        height: r.try_get("height")?,
        bytes: r.try_get("bytes")?,
        fingerprint: r.try_get("fingerprint")?,
    })
}

pub struct CoverUploadModel;

impl CoverUploadModel {
    /// Record an upload (caller's transaction, key row locked).
    pub async fn create_in(
        conn: &mut MySqlConnection,
        api_key_id: u64,
        upload: &CoverUpload,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO cover_uploads (token, api_key_id, staged_file, media_type, \
             original_bytes, width, height, bytes, fingerprint) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&upload.token)
        .bind(api_key_id)
        .bind(&upload.staged_file)
        .bind(&upload.media_type)
        .bind(upload.original_bytes)
        .bind(upload.width)
        .bind(upload.height)
        .bind(upload.bytes)
        .bind(&upload.fingerprint)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Unexpired uploads a key holds.
    pub async fn count_live_in(
        conn: &mut MySqlConnection,
        api_key_id: u64,
        ttl_secs: u64,
    ) -> Result<i64, AppError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM cover_uploads \
             WHERE api_key_id = ? AND created_at > NOW() - INTERVAL ? SECOND",
        )
        .bind(api_key_id)
        .bind(ttl_secs)
        .fetch_one(&mut *conn)
        .await?;
        Ok(count)
    }

    /// This key's expired uploads, `(token, staged_file)` (under the key lock).
    pub async fn expired_of_key_in(
        conn: &mut MySqlConnection,
        api_key_id: u64,
        ttl_secs: u64,
    ) -> Result<Vec<(String, String)>, AppError> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT token, staged_file FROM cover_uploads \
             WHERE api_key_id = ? AND created_at <= NOW() - INTERVAL ? SECOND",
        )
        .bind(api_key_id)
        .bind(ttl_secs)
        .fetch_all(&mut *conn)
        .await?;
        Ok(rows)
    }

    /// The unexpired upload `token` of this key (plain read).
    pub async fn find_live(
        pool: &DbPool,
        token: &str,
        api_key_id: u64,
        ttl_secs: u64,
    ) -> Result<Option<CoverUpload>, AppError> {
        let sql = format!(
            "SELECT {COLUMNS} FROM cover_uploads \
             WHERE token = ? AND api_key_id = ? AND created_at > NOW() - INTERVAL ? SECOND"
        );
        let row = sqlx::query(&sql)
            .bind(token)
            .bind(api_key_id)
            .bind(ttl_secs)
            .fetch_optional(pool)
            .await?;
        row.as_ref().map(from_row).transpose().map_err(AppError::from)
    }

    /// Lock upload `token` by primary key, for the filing that consumes it;
    /// `false` when it is gone, expired or another key's (Rule #22).
    pub async fn lock_live_in(
        conn: &mut MySqlConnection,
        token: &str,
        api_key_id: u64,
        ttl_secs: u64,
    ) -> Result<bool, AppError> {
        let row = sqlx::query(
            "SELECT token FROM cover_uploads \
             WHERE token = ? AND api_key_id = ? AND created_at > NOW() - INTERVAL ? SECOND \
             FOR UPDATE",
        )
        .bind(token)
        .bind(api_key_id)
        .bind(ttl_secs)
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.is_some())
    }

    /// Consume an upload (in the filing's transaction).
    pub async fn delete_in(conn: &mut MySqlConnection, token: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM cover_uploads WHERE token = ?")
            .bind(token)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    /// Remove expired uploads; their files go with the staging purge.
    pub async fn purge_expired(pool: &DbPool, ttl_secs: u64) -> Result<u64, AppError> {
        let done = sqlx::query(
            "DELETE FROM cover_uploads WHERE created_at <= NOW() - INTERVAL ? SECOND",
        )
        .bind(ttl_secs)
        .execute(pool)
        .await?;
        Ok(done.rows_affected())
    }
}
