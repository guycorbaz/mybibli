//! Story 11-9 (AR29) — agent proposals and their items.
//!
//! Row access only: validation, transition rules and staleness live in
//! `services::proposals`. Functions that a later story must run inside a
//! larger transaction (filing, staleness, accept-and-apply in 11-17)
//! take `&mut MySqlConnection`; a `PoolConnection` or a `Transaction`
//! derefs to it.
//!
//! MariaDB gotchas (CLAUDE.md): JSON columns are read with
//! `CAST(col AS CHAR)`, TIMESTAMP columns with `CAST(col AS DATETIME)`
//! (dynamic queries).

use std::collections::HashMap;

use chrono::NaiveDateTime;
use sqlx::{MySqlConnection, Row};

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::DEFAULT_PAGE_SIZE;

/// What a proposal suggests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProposalKind {
    SeriesCreate,
    SeriesAttach,
    Metadata,
    Cover,
}

impl ProposalKind {
    pub const ALL: [ProposalKind; 4] = [
        ProposalKind::SeriesCreate,
        ProposalKind::SeriesAttach,
        ProposalKind::Metadata,
        ProposalKind::Cover,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            ProposalKind::SeriesCreate => "series_create",
            ProposalKind::SeriesAttach => "series_attach",
            ProposalKind::Metadata => "metadata",
            ProposalKind::Cover => "cover",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// Lifecycle state of a proposal. `Pending` is the only non-final state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProposalStatus {
    Pending,
    Accepted,
    Rejected,
    Stale,
}

impl ProposalStatus {
    pub const ALL: [ProposalStatus; 4] = [
        ProposalStatus::Pending,
        ProposalStatus::Accepted,
        ProposalStatus::Rejected,
        ProposalStatus::Stale,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            ProposalStatus::Pending => "pending",
            ProposalStatus::Accepted => "accepted",
            ProposalStatus::Rejected => "rejected",
            ProposalStatus::Stale => "stale",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// `true` for `accepted`, `rejected` and `stale`: nothing leaves them.
    pub fn is_final(&self) -> bool {
        !matches!(self, ProposalStatus::Pending)
    }

    /// The transition table: only `pending` → one of the three final
    /// states. Every other pair — including `pending` → `pending` — is
    /// refused.
    pub fn can_transition_to(&self, to: ProposalStatus) -> bool {
        matches!(self, ProposalStatus::Pending) && to.is_final()
    }
}

/// Table an item may target. The SQL identifier comes only from this
/// enum, never from input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProposalTargetTable {
    Titles,
    Series,
}

impl ProposalTargetTable {
    pub const ALL: [ProposalTargetTable; 2] =
        [ProposalTargetTable::Titles, ProposalTargetTable::Series];

    pub fn as_str(&self) -> &'static str {
        match self {
            ProposalTargetTable::Titles => "titles",
            ProposalTargetTable::Series => "series",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// An existing entity an item acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProposalTarget {
    pub table: ProposalTargetTable,
    pub id: u64,
}

/// State of a target row as read now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetRowState {
    pub version: i32,
    pub deleted: bool,
}

/// One proposal row.
#[derive(Debug, Clone)]
pub struct ProposalRow {
    pub id: u64,
    pub kind: ProposalKind,
    pub status: ProposalStatus,
    pub api_key_id: u64,
    pub api_key_label: Option<String>,
    /// Raw JSON text of the `sources` column.
    pub sources_json: String,
    pub decided_by: Option<u64>,
    pub decided_by_username: Option<String>,
    pub decided_at: Option<NaiveDateTime>,
    pub rejection_reason: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub version: i32,
}

/// One item row.
#[derive(Debug, Clone)]
pub struct ProposalItemRow {
    pub id: u64,
    pub proposal_id: u64,
    pub target: Option<ProposalTarget>,
    pub target_version: Option<i32>,
    /// Raw JSON text of the `proposed_values` column.
    pub proposed_values_json: String,
    pub version: i32,
}

/// One line of the proposals list.
#[derive(Debug, Clone)]
pub struct ProposalSummaryRow {
    pub id: u64,
    pub kind: ProposalKind,
    pub status: ProposalStatus,
    pub api_key_id: u64,
    pub api_key_label: Option<String>,
    pub item_count: i64,
    pub created_at: NaiveDateTime,
    pub decided_at: Option<NaiveDateTime>,
}

/// List filters; every `None` means "any".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProposalFilter {
    pub status: Option<ProposalStatus>,
    pub kind: Option<ProposalKind>,
    pub api_key_id: Option<u64>,
}

/// An item to insert (target version already read by the service).
#[derive(Debug, Clone)]
pub struct NewItemRow<'a> {
    pub target: Option<(ProposalTarget, i32)>,
    pub proposed_values_json: &'a str,
}

/// What a guarded status update sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusUpdate {
    pub to: ProposalStatus,
    pub decided_by: Option<u64>,
    pub rejection_reason: Option<String>,
}

fn parse_enum<T>(raw: &str, column: &str, parse: fn(&str) -> Option<T>) -> Result<T, AppError> {
    parse(raw).ok_or_else(|| AppError::Internal(format!("unexpected {column} value: {raw}")))
}

// The `api_keys` and `users` joins deliberately carry no
// `deleted_at IS NULL`: a proposal keeps showing the label of the key
// that filed it after the key is revoked or deleted, and the name of the
// user who decided it after that user is deactivated (AC4, AC5). The row
// itself only disappears when purged (`decided_by` is then SET NULL).
// The COALESCE is CAST to CHAR: on a server whose default charset is not
// utf8mb4 (CI's MariaDB), `users.username` and `proposals.decided_by_name`
// carry different collations and the bare COALESCE decodes as VARBINARY.
const PROPOSAL_SELECT: &str = "SELECT p.id, p.kind, p.status, p.api_key_id, k.label AS api_key_label, \
     CAST(p.sources AS CHAR) AS sources, CAST(p.decided_by AS SIGNED) AS decided_by, \
     CAST(COALESCE(u.username, p.decided_by_name) AS CHAR) AS decided_by_username, \
     CAST(p.decided_at AS DATETIME) AS decided_at, p.rejection_reason, \
     CAST(p.created_at AS DATETIME) AS created_at, CAST(p.updated_at AS DATETIME) AS updated_at, \
     p.version \
     FROM proposals p \
     LEFT JOIN api_keys k ON k.id = p.api_key_id \
     LEFT JOIN users u ON u.id = p.decided_by";

const ITEM_SELECT: &str = "SELECT id, proposal_id, target_table, CAST(target_id AS SIGNED) AS target_id, target_version, \
     CAST(proposed_values AS CHAR) AS proposed_values, version \
     FROM proposal_items";

pub struct ProposalModel;

impl ProposalModel {
    fn proposal_from_row(r: &sqlx::mysql::MySqlRow) -> Result<ProposalRow, AppError> {
        let kind: String = r.try_get("kind")?;
        let status: String = r.try_get("status")?;
        Ok(ProposalRow {
            id: r.try_get("id")?,
            kind: parse_enum(&kind, "proposals.kind", ProposalKind::parse)?,
            status: parse_enum(&status, "proposals.status", ProposalStatus::parse)?,
            api_key_id: r.try_get("api_key_id")?,
            api_key_label: r.try_get("api_key_label")?,
            sources_json: r.try_get("sources")?,
            decided_by: r.try_get::<Option<i64>, _>("decided_by")?.map(|v| v as u64),
            decided_by_username: r.try_get("decided_by_username")?,
            decided_at: r.try_get("decided_at")?,
            rejection_reason: r.try_get("rejection_reason")?,
            created_at: r.try_get("created_at")?,
            updated_at: r.try_get("updated_at")?,
            version: r.try_get("version")?,
        })
    }

    fn item_from_row(r: &sqlx::mysql::MySqlRow) -> Result<ProposalItemRow, AppError> {
        let table: Option<String> = r.try_get("target_table")?;
        let target_id: Option<u64> = r.try_get::<Option<i64>, _>("target_id")?.map(|v| v as u64);
        let target = match (table, target_id) {
            (Some(t), Some(id)) => Some(ProposalTarget {
                table: parse_enum(
                    &t,
                    "proposal_items.target_table",
                    ProposalTargetTable::parse,
                )?,
                id,
            }),
            _ => None,
        };
        Ok(ProposalItemRow {
            id: r.try_get("id")?,
            proposal_id: r.try_get("proposal_id")?,
            target,
            target_version: r.try_get("target_version")?,
            proposed_values_json: r.try_get("proposed_values")?,
            version: r.try_get("version")?,
        })
    }

    /// Insert a pending proposal; returns its id.
    pub async fn insert_proposal(
        conn: &mut MySqlConnection,
        kind: ProposalKind,
        api_key_id: u64,
        sources_json: &str,
    ) -> Result<u64, AppError> {
        let res = sqlx::query(
            "INSERT INTO proposals (kind, status, api_key_id, sources) VALUES (?, 'pending', ?, ?)",
        )
        .bind(kind.as_str())
        .bind(api_key_id)
        .bind(sources_json)
        .execute(&mut *conn)
        .await?;
        Ok(res.last_insert_id())
    }

    /// Insert one item of a proposal; returns its id.
    pub async fn insert_item(
        conn: &mut MySqlConnection,
        proposal_id: u64,
        item: &NewItemRow<'_>,
    ) -> Result<u64, AppError> {
        let (table, id, version) = match item.target {
            Some((t, v)) => (Some(t.table.as_str()), Some(t.id), Some(v)),
            None => (None, None, None),
        };
        let res = sqlx::query(
            "INSERT INTO proposal_items (proposal_id, target_table, target_id, target_version, proposed_values) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(proposal_id)
        .bind(table)
        .bind(id)
        .bind(version)
        .bind(item.proposed_values_json)
        .execute(&mut *conn)
        .await?;
        Ok(res.last_insert_id())
    }

    /// Current state of the given rows of one target table, by id. Rows
    /// that no longer exist (hard-purged) are absent from the map. With
    /// `live_only`, soft-deleted rows are absent too. With `lock`, the
    /// read is a locking read (`FOR UPDATE`): it sees the latest committed
    /// rows rather than the transaction's snapshot, and holds them until
    /// the transaction ends, so a staleness check followed by an apply in
    /// the same transaction (story 11-17) cannot miss a concurrent edit.
    pub async fn target_states(
        conn: &mut MySqlConnection,
        table: ProposalTargetTable,
        ids: &[u64],
        live_only: bool,
        lock: bool,
    ) -> Result<HashMap<u64, TargetRowState>, AppError> {
        let mut out = HashMap::new();
        if ids.is_empty() {
            return Ok(out);
        }
        let placeholders = vec!["?"; ids.len()].join(", ");
        let live = if live_only {
            " AND deleted_at IS NULL"
        } else {
            ""
        };
        let lock = if lock { " FOR UPDATE" } else { "" };
        // `table` comes from the enum, never from input.
        let sql = format!(
            "SELECT id, version, CAST(deleted_at IS NOT NULL AS SIGNED) AS is_deleted FROM {} WHERE id IN ({placeholders}){live}{lock}",
            table.as_str()
        );
        let mut q = sqlx::query(&sql);
        for id in ids {
            q = q.bind(id);
        }
        for r in q.fetch_all(&mut *conn).await? {
            let deleted: i64 = r.try_get("is_deleted")?;
            out.insert(
                r.try_get("id")?,
                TargetRowState {
                    version: r.try_get("version")?,
                    deleted: deleted != 0,
                },
            );
        }
        Ok(out)
    }

    /// A live proposal by id.
    pub async fn find(
        conn: &mut MySqlConnection,
        id: u64,
    ) -> Result<Option<ProposalRow>, AppError> {
        let sql = format!("{PROPOSAL_SELECT} WHERE p.id = ? AND p.deleted_at IS NULL");
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
        row.as_ref().map(Self::proposal_from_row).transpose()
    }

    /// Live items of a proposal, in filing order.
    ///
    /// With `lock` (a decision is being made), the read is a locking
    /// read (`LOCK IN SHARE MODE`): it sees items removed or added after
    /// the caller's snapshot and keeps them from changing until the
    /// transaction ends.
    pub async fn items(
        conn: &mut MySqlConnection,
        proposal_id: u64,
        lock: bool,
    ) -> Result<Vec<ProposalItemRow>, AppError> {
        let lock = if lock { " LOCK IN SHARE MODE" } else { "" };
        let sql =
            format!("{ITEM_SELECT} WHERE proposal_id = ? AND deleted_at IS NULL ORDER BY id{lock}");
        let rows = sqlx::query(&sql)
            .bind(proposal_id)
            .fetch_all(&mut *conn)
            .await?;
        rows.iter().map(Self::item_from_row).collect()
    }

    /// Lock a live proposal row (`FOR UPDATE`) and return its status.
    pub async fn lock_status(
        conn: &mut MySqlConnection,
        id: u64,
    ) -> Result<Option<ProposalStatus>, AppError> {
        let status: Option<String> = sqlx::query_scalar(
            "SELECT status FROM proposals WHERE id = ? AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
        status
            .map(|s| parse_enum(&s, "proposals.status", ProposalStatus::parse))
            .transpose()
    }

    /// Move a live `pending` proposal to a final state. The status guard
    /// is part of the `WHERE`, so of two concurrent deciders only one
    /// updates a row. Returns the number of rows updated (0 or 1).
    pub async fn update_status_from_pending(
        conn: &mut MySqlConnection,
        id: u64,
        update: &StatusUpdate,
    ) -> Result<u64, AppError> {
        let res = sqlx::query(
            "UPDATE proposals SET status = ?, decided_by = ?, \
             decided_by_name = (SELECT username FROM users WHERE id = ?), decided_at = NOW(), \
             rejection_reason = ?, version = version + 1 \
             WHERE id = ? AND status = 'pending' AND deleted_at IS NULL",
        )
        .bind(update.to.as_str())
        .bind(update.decided_by)
        .bind(update.decided_by)
        .bind(update.rejection_reason.as_deref())
        .bind(id)
        .execute(&mut *conn)
        .await?;
        Ok(res.rows_affected())
    }

    /// Whether the key may file proposals: it exists, is neither revoked
    /// nor deleted, and has the `propose` scope. The row is share-locked
    /// until the filing transaction ends.
    ///
    /// **Lock order for story 11-14 (revocation):** lock the `api_keys`
    /// row first (`UPDATE api_keys … ` or `SELECT … FOR UPDATE`), then
    /// mark the key's pending proposals stale. A revocation that touched
    /// `proposals` first could deadlock with an in-flight filing (which
    /// holds this share lock and then inserts into `proposals`); in the
    /// prescribed order it simply waits for the filing to commit and
    /// then sees the new proposal.
    pub async fn lock_filing_key(
        conn: &mut MySqlConnection,
        api_key_id: u64,
    ) -> Result<bool, AppError> {
        let found: Option<u64> = sqlx::query_scalar(
            "SELECT id FROM api_keys WHERE id = ? AND scope = 'propose' \
             AND revoked_at IS NULL AND deleted_at IS NULL LOCK IN SHARE MODE",
        )
        .bind(api_key_id)
        .fetch_optional(&mut *conn)
        .await?;
        Ok(found.is_some())
    }

    /// Whether `user_id` is an active user (the login predicate:
    /// `active = TRUE AND deleted_at IS NULL`, story 8-3). A locking read:
    /// a deactivation committed after the caller's snapshot is seen, and
    /// one still in flight waits for the decision.
    pub async fn is_active_user(
        conn: &mut MySqlConnection,
        user_id: u64,
    ) -> Result<bool, AppError> {
        let found: Option<u64> = sqlx::query_scalar(
            "SELECT id FROM users WHERE id = ? AND active = TRUE AND deleted_at IS NULL \
             LOCK IN SHARE MODE",
        )
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await?;
        Ok(found.is_some())
    }

    /// Story 11-12 — the oldest live pending proposal with a live item
    /// targeting `table`/`id`, if any.
    pub async fn pending_targeting(
        pool: &DbPool,
        table: ProposalTargetTable,
        id: u64,
    ) -> Result<Option<u64>, AppError> {
        let found: Option<u64> = sqlx::query_scalar(
            "SELECT p.id FROM proposals p \
             JOIN proposal_items i ON i.proposal_id = p.id AND i.deleted_at IS NULL \
             WHERE p.deleted_at IS NULL AND p.status = 'pending' \
               AND i.target_table = ? AND i.target_id = ? \
             ORDER BY p.id ASC LIMIT 1",
        )
        .bind(table.as_str())
        .bind(id)
        .fetch_optional(pool)
        .await?;
        Ok(found)
    }

    /// Story 11-12 — the oldest live pending `series_create` proposal
    /// whose series line (`"line": "series"`, no target) proposes `name`,
    /// compared case- and accent-insensitively. Both sides are converted
    /// to utf8mb4 explicitly so the server's default charset (CI) cannot
    /// change the comparison.
    pub async fn pending_series_create_named(
        pool: &DbPool,
        name: &str,
    ) -> Result<Option<u64>, AppError> {
        let found: Option<u64> = sqlx::query_scalar(
            "SELECT p.id FROM proposals p \
             JOIN proposal_items i ON i.proposal_id = p.id AND i.deleted_at IS NULL \
             WHERE p.deleted_at IS NULL AND p.status = 'pending' AND p.kind = 'series_create' \
               AND i.target_table IS NULL \
               AND JSON_UNQUOTE(JSON_EXTRACT(i.proposed_values, '$.line')) = 'series' \
               AND CONVERT(JSON_UNQUOTE(JSON_EXTRACT(i.proposed_values, '$.name')) USING utf8mb4) \
                   COLLATE utf8mb4_unicode_ci = CONVERT(? USING utf8mb4) COLLATE utf8mb4_unicode_ci \
             ORDER BY p.id ASC LIMIT 1",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;
        Ok(found)
    }

    /// One page of live proposals, newest first.
    pub async fn list(
        pool: &DbPool,
        filter: &ProposalFilter,
        page: u32,
    ) -> Result<(Vec<ProposalSummaryRow>, u64), AppError> {
        let mut where_sql = String::from("p.deleted_at IS NULL");
        if filter.status.is_some() {
            where_sql.push_str(" AND p.status = ?");
        }
        if filter.kind.is_some() {
            where_sql.push_str(" AND p.kind = ?");
        }
        if filter.api_key_id.is_some() {
            where_sql.push_str(" AND p.api_key_id = ?");
        }

        let count_sql = format!("SELECT COUNT(*) FROM proposals p WHERE {where_sql}");
        let list_sql = format!(
            "SELECT p.id, p.kind, p.status, p.api_key_id, k.label AS api_key_label, \
             (SELECT COUNT(*) FROM proposal_items i WHERE i.proposal_id = p.id AND i.deleted_at IS NULL) AS item_count, \
             CAST(p.created_at AS DATETIME) AS created_at, CAST(p.decided_at AS DATETIME) AS decided_at \
             FROM proposals p LEFT JOIN api_keys k ON k.id = p.api_key_id \
             WHERE {where_sql} ORDER BY p.created_at DESC, p.id DESC LIMIT ? OFFSET ?"
        );

        let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
        let mut list_q = sqlx::query(&list_sql);
        if let Some(s) = filter.status {
            count_q = count_q.bind(s.as_str());
            list_q = list_q.bind(s.as_str());
        }
        if let Some(k) = filter.kind {
            count_q = count_q.bind(k.as_str());
            list_q = list_q.bind(k.as_str());
        }
        if let Some(key) = filter.api_key_id {
            count_q = count_q.bind(key);
            list_q = list_q.bind(key);
        }
        // One transaction, one snapshot: the total matches the page.
        let mut tx = pool.begin().await?;
        let total = count_q.fetch_one(&mut *tx).await?;
        let offset = u64::from(page.max(1) - 1) * u64::from(DEFAULT_PAGE_SIZE);
        let rows = list_q
            .bind(DEFAULT_PAGE_SIZE)
            .bind(offset)
            .fetch_all(&mut *tx)
            .await?;
        tx.commit().await?;

        let items = rows
            .iter()
            .map(|r| {
                let kind: String = r.try_get("kind")?;
                let status: String = r.try_get("status")?;
                Ok(ProposalSummaryRow {
                    id: r.try_get("id")?,
                    kind: parse_enum(&kind, "proposals.kind", ProposalKind::parse)?,
                    status: parse_enum(&status, "proposals.status", ProposalStatus::parse)?,
                    api_key_id: r.try_get("api_key_id")?,
                    api_key_label: r.try_get("api_key_label")?,
                    item_count: r.try_get("item_count")?,
                    created_at: r.try_get("created_at")?,
                    decided_at: r.try_get("decided_at")?,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        Ok((items, total.max(0) as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_round_trips() {
        for k in ProposalKind::ALL {
            assert_eq!(ProposalKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(ProposalKind::parse("Metadata"), None);
        assert_eq!(ProposalKind::parse(""), None);
    }

    #[test]
    fn status_round_trips() {
        for s in ProposalStatus::ALL {
            assert_eq!(ProposalStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(ProposalStatus::parse("PENDING"), None);
    }

    #[test]
    fn target_table_round_trips() {
        for t in ProposalTargetTable::ALL {
            assert_eq!(ProposalTargetTable::parse(t.as_str()), Some(t));
        }
        assert_eq!(ProposalTargetTable::parse("users"), None);
    }

    #[test]
    fn transition_table_only_allows_pending_to_final() {
        use ProposalStatus::*;
        for from in ProposalStatus::ALL {
            for to in ProposalStatus::ALL {
                let expected = from == Pending && to != Pending;
                assert_eq!(
                    from.can_transition_to(to),
                    expected,
                    "{} -> {}",
                    from.as_str(),
                    to.as_str()
                );
            }
        }
    }

    #[test]
    fn only_pending_is_not_final() {
        assert!(!ProposalStatus::Pending.is_final());
        assert!(ProposalStatus::Accepted.is_final());
        assert!(ProposalStatus::Rejected.is_final());
        assert!(ProposalStatus::Stale.is_final());
    }
}
