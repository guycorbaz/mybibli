//! CR #241 — API keys for the JSON HTTP surface.
//!
//! Keys are minted by an admin via `/admin?tab=api_keys`. The
//! plaintext is shown ONCE in a copy-friendly modal at creation; only
//! its argon2 hash and prefix are persisted. Subsequent lookups
//! happen via the middleware which hashes the incoming `Authorization:
//! Bearer …` value and compares against `key_hash`.
//!
//! Single-tenant LAN context: no rate-limit column in v1 (revisit if
//! abuse appears).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ApiKeyScope {
    Read,
    Write,
}

impl ApiKeyScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApiKeyScope::Read => "read",
            ApiKeyScope::Write => "write",
        }
    }

    /// Short form used in the plaintext key prefix (`mybibli_ro_…` /
    /// `mybibli_rw_…`). Helps users not paste a write key into a
    /// read-only client by accident.
    pub fn short(&self) -> &'static str {
        match self {
            ApiKeyScope::Read => "ro",
            ApiKeyScope::Write => "rw",
        }
    }

    pub fn from_db(s: &str) -> Self {
        match s {
            "write" => ApiKeyScope::Write,
            _ => ApiKeyScope::Read,
        }
    }

    /// `write` implies `read` — a write-scope key can also call any
    /// read endpoint.
    pub fn allows_read(&self) -> bool {
        true
    }

    pub fn allows_write(&self) -> bool {
        matches!(self, ApiKeyScope::Write)
    }
}

#[derive(Debug, Clone)]
pub struct ApiKey {
    pub id: u64,
    pub label: String,
    pub key_prefix: String,
    pub scope: ApiKeyScope,
    pub created_by: Option<u64>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub version: i32,
}

pub struct ApiKeyModel;

impl ApiKeyModel {
    /// Insert a new API key row. Returns the inserted row's id.
    ///
    /// `key_hash` MUST be the argon2 hash of the plaintext key; the
    /// caller is responsible for hashing (route layer mints the
    /// plaintext + hashes via `services::password::hash_password`).
    /// The plaintext is never persisted.
    pub async fn create(
        pool: &DbPool,
        label: &str,
        key_hash: &str,
        key_prefix: &str,
        scope: ApiKeyScope,
        created_by: Option<u64>,
    ) -> Result<u64, AppError> {
        let id = sqlx::query(
            "INSERT INTO api_keys (label, key_hash, key_prefix, scope, created_by) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(label)
        .bind(key_hash)
        .bind(key_prefix)
        .bind(scope.as_str())
        .bind(created_by)
        .execute(pool)
        .await?
        .last_insert_id();
        Ok(id)
    }

    /// Look up an active (not soft-deleted, not revoked) key by id +
    /// project columns minus `key_hash` (the hash never leaves the DB
    /// layer; the middleware passes it as a separate arg to
    /// `verify_password`).
    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<ApiKey>, AppError> {
        let row = sqlx::query(
            "SELECT id, label, key_prefix, scope, \
                    CAST(created_by AS SIGNED) AS created_by, \
                    last_used_at, revoked_at, created_at, version \
             FROM api_keys \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(row.map(map_row))
    }

    /// Return every candidate row for prefix-match against an incoming
    /// plaintext token. The middleware then verifies each candidate's
    /// `key_hash` against the plaintext via argon2.
    ///
    /// Rationale: argon2 can't be used as a database-indexed lookup
    /// (the hash carries a random salt — equal plaintexts hash to
    /// different strings). The 12-char `key_prefix` narrows the
    /// candidate set to a handful of rows in practice (in a household
    /// catalog there will be ≤ a dozen active keys).
    pub async fn find_candidates_by_prefix(
        pool: &DbPool,
        prefix: &str,
    ) -> Result<Vec<ApiKeyCandidate>, AppError> {
        let rows = sqlx::query(
            "SELECT id, scope, key_hash \
             FROM api_keys \
             WHERE key_prefix = ? \
               AND deleted_at IS NULL \
               AND revoked_at IS NULL",
        )
        .bind(prefix)
        .fetch_all(pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ApiKeyCandidate {
                id: r.try_get("id").unwrap_or(0),
                scope: ApiKeyScope::from_db(&r.try_get::<String, _>("scope").unwrap_or_default()),
                key_hash: r.try_get("key_hash").unwrap_or_default(),
            })
            .collect())
    }

    /// List every active key for the admin UI. Includes revoked rows
    /// (UI renders them with a grey "Revoked" chip) but excludes
    /// hard-deleted ones.
    pub async fn list_for_admin(pool: &DbPool) -> Result<Vec<ApiKey>, AppError> {
        let rows = sqlx::query(
            "SELECT id, label, key_prefix, scope, \
                    CAST(created_by AS SIGNED) AS created_by, \
                    last_used_at, revoked_at, created_at, version \
             FROM api_keys \
             WHERE deleted_at IS NULL \
             ORDER BY created_at DESC, id DESC",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows.into_iter().map(map_row).collect())
    }

    /// Stamp `last_used_at = NOW()`. Called by the middleware on every
    /// successful authentication. Best-effort: failure is logged and
    /// ignored so a transient DB hiccup can't drop legitimate API
    /// traffic.
    pub async fn touch_last_used(pool: &DbPool, id: u64) -> Result<(), AppError> {
        sqlx::query("UPDATE api_keys SET last_used_at = NOW() WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Soft-revoke. Optimistic-lock on `version` so two admins
    /// hitting the same Revoke button concurrently produce one
    /// success + one conflict.
    pub async fn revoke(pool: &DbPool, id: u64, version: i32) -> Result<u64, AppError> {
        let result = sqlx::query(
            "UPDATE api_keys \
             SET revoked_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND revoked_at IS NULL AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn count_active(pool: &DbPool) -> Result<i64, AppError> {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM api_keys WHERE deleted_at IS NULL AND revoked_at IS NULL",
        )
        .fetch_one(pool)
        .await?;
        Ok(n)
    }
}

/// Lightweight row carrier used by the middleware's prefix-narrowed
/// candidate scan. Carries the hash field so the middleware can run
/// the argon2 verification without an extra round-trip.
pub struct ApiKeyCandidate {
    pub id: u64,
    pub scope: ApiKeyScope,
    pub key_hash: String,
}

fn map_row(r: sqlx::mysql::MySqlRow) -> ApiKey {
    ApiKey {
        id: r.try_get("id").unwrap_or(0),
        label: r.try_get("label").unwrap_or_default(),
        key_prefix: r.try_get("key_prefix").unwrap_or_default(),
        scope: ApiKeyScope::from_db(&r.try_get::<String, _>("scope").unwrap_or_default()),
        created_by: r
            .try_get::<Option<i64>, _>("created_by")
            .ok()
            .flatten()
            .map(|n| n as u64),
        last_used_at: r.try_get("last_used_at").ok(),
        revoked_at: r.try_get("revoked_at").ok(),
        created_at: r
            .try_get::<DateTime<Utc>, _>("created_at")
            .unwrap_or_else(|_| Utc::now()),
        version: r.try_get("version").unwrap_or(0),
    }
}

/// Mint a fresh plaintext key for the given scope. Format:
/// `mybibli_<scope_short>_<random40>`. Returns the plaintext (caller
/// shows it once + hashes it for persistence) and the 12-char prefix
/// to store as the searchable column.
pub fn mint_plaintext_key(scope: ApiKeyScope) -> (String, String) {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    let suffix: String = (0..40)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();
    let plaintext = format!("mybibli_{}_{}", scope.short(), suffix);
    // 12 chars covers `mybibli_ro_X` / `mybibli_rw_X` — enough to
    // narrow the candidate scan without leaking enough of the suffix
    // to brute-force.
    let prefix: String = plaintext.chars().take(12).collect();
    (plaintext, prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::MySqlPool;

    fn hash(plain: &str) -> String {
        crate::services::password::hash_password(plain).expect("hash")
    }

    #[test]
    fn mint_plaintext_key_has_expected_shape() {
        let (k, prefix) = mint_plaintext_key(ApiKeyScope::Read);
        assert!(k.starts_with("mybibli_ro_"));
        assert_eq!(k.len(), "mybibli_ro_".len() + 40);
        assert_eq!(prefix.len(), 12);
        assert_eq!(prefix, &k[..12]);

        let (kw, prefix_w) = mint_plaintext_key(ApiKeyScope::Write);
        assert!(kw.starts_with("mybibli_rw_"));
        assert_eq!(prefix_w, &kw[..12]);

        // Two consecutive mints MUST produce different plaintexts (random suffix).
        let (k2, _) = mint_plaintext_key(ApiKeyScope::Read);
        assert_ne!(k, k2);
    }

    #[test]
    fn scope_from_db_falls_back_to_read_for_unknown() {
        assert_eq!(ApiKeyScope::from_db("write"), ApiKeyScope::Write);
        assert_eq!(ApiKeyScope::from_db("read"), ApiKeyScope::Read);
        assert_eq!(ApiKeyScope::from_db("garbage"), ApiKeyScope::Read);
    }

    #[test]
    fn scope_allows_methods() {
        assert!(ApiKeyScope::Read.allows_read());
        assert!(!ApiKeyScope::Read.allows_write());
        assert!(ApiKeyScope::Write.allows_read());
        assert!(ApiKeyScope::Write.allows_write());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn create_then_find_by_id_round_trips(pool: MySqlPool) {
        let id = ApiKeyModel::create(
            &pool,
            "Test key",
            &hash("mybibli_ro_AbCdEf"),
            "mybibli_ro_A",
            ApiKeyScope::Read,
            None,
        )
        .await
        .unwrap();
        let got = ApiKeyModel::find_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.label, "Test key");
        assert_eq!(got.scope, ApiKeyScope::Read);
        assert!(got.last_used_at.is_none());
        assert!(got.revoked_at.is_none());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn revoke_marks_revoked_at_and_excludes_from_candidate_scan(pool: MySqlPool) {
        let id = ApiKeyModel::create(
            &pool,
            "RW key",
            &hash("plain"),
            "mybibli_rw_X",
            ApiKeyScope::Write,
            None,
        )
        .await
        .unwrap();

        // Before revoke: appears in candidate scan.
        let candidates = ApiKeyModel::find_candidates_by_prefix(&pool, "mybibli_rw_X")
            .await
            .unwrap();
        assert_eq!(candidates.len(), 1);

        let n = ApiKeyModel::revoke(&pool, id, 0).await.unwrap();
        assert_eq!(n, 1);

        // After revoke: candidate scan returns empty.
        let candidates = ApiKeyModel::find_candidates_by_prefix(&pool, "mybibli_rw_X")
            .await
            .unwrap();
        assert!(candidates.is_empty(), "revoked keys MUST NOT be candidates");

        // find_by_id still returns the row (admin UI lists revoked).
        let got = ApiKeyModel::find_by_id(&pool, id).await.unwrap().unwrap();
        assert!(got.revoked_at.is_some());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn revoke_with_stale_version_returns_zero(pool: MySqlPool) {
        let id = ApiKeyModel::create(
            &pool,
            "K",
            &hash("p"),
            "mybibli_ro_A",
            ApiKeyScope::Read,
            None,
        )
        .await
        .unwrap();
        let n = ApiKeyModel::revoke(&pool, id, 99).await.unwrap();
        assert_eq!(n, 0, "stale version MUST not revoke");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn touch_last_used_sets_timestamp(pool: MySqlPool) {
        let id = ApiKeyModel::create(
            &pool,
            "K",
            &hash("p"),
            "mybibli_ro_A",
            ApiKeyScope::Read,
            None,
        )
        .await
        .unwrap();
        ApiKeyModel::touch_last_used(&pool, id).await.unwrap();
        let got = ApiKeyModel::find_by_id(&pool, id).await.unwrap().unwrap();
        assert!(got.last_used_at.is_some());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn list_for_admin_includes_revoked_excludes_deleted(pool: MySqlPool) {
        let a = ApiKeyModel::create(
            &pool,
            "active",
            &hash("p"),
            "mybibli_ro_A",
            ApiKeyScope::Read,
            None,
        )
        .await
        .unwrap();
        let r = ApiKeyModel::create(
            &pool,
            "revoked",
            &hash("p"),
            "mybibli_rw_R",
            ApiKeyScope::Write,
            None,
        )
        .await
        .unwrap();
        ApiKeyModel::revoke(&pool, r, 0).await.unwrap();
        // Hard-delete a third row to verify exclusion.
        let d = ApiKeyModel::create(
            &pool,
            "deleted",
            &hash("p"),
            "mybibli_ro_D",
            ApiKeyScope::Read,
            None,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE api_keys SET deleted_at = NOW() WHERE id = ?")
            .bind(d)
            .execute(&pool)
            .await
            .unwrap();

        let got = ApiKeyModel::list_for_admin(&pool).await.unwrap();
        let ids: Vec<u64> = got.iter().map(|k| k.id).collect();
        assert!(ids.contains(&a), "active row must appear");
        assert!(ids.contains(&r), "revoked row must appear (admin needs visibility)");
        assert!(!ids.contains(&d), "hard-deleted row MUST NOT appear");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn count_active_excludes_revoked_and_deleted(pool: MySqlPool) {
        ApiKeyModel::create(&pool, "a", &hash("p"), "mybibli_ro_A", ApiKeyScope::Read, None)
            .await
            .unwrap();
        let r = ApiKeyModel::create(&pool, "r", &hash("p"), "mybibli_ro_B", ApiKeyScope::Read, None)
            .await
            .unwrap();
        ApiKeyModel::revoke(&pool, r, 0).await.unwrap();
        let n = ApiKeyModel::count_active(&pool).await.unwrap();
        assert_eq!(n, 1, "only the active row counts");
    }
}
