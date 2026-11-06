//! Story 15-5 (#564) — a volume's amounts as the value proposals read and
//! write them: integer cents (D2 of the story), never floats.
//!
//! The hand edit (`VolumeModel::update_value`, CR #243) is unchanged; the
//! proposal path writes through [`VolumeValueModel::apply_in`] inside the
//! acceptance transaction.

use sqlx::{MySqlConnection, Row};

use crate::error::AppError;

/// An amount with its currency (ISO 4217 code as stored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Amount {
    pub cents: i64,
    pub currency: Option<String>,
}

/// What a value proposal needs to know about a live volume of a live
/// title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeValueState {
    pub id: u64,
    pub title_id: u64,
    pub label: String,
    pub version: i32,
    pub purchase_price: Option<Amount>,
    pub current_value: Option<Amount>,
    pub current_value_updated_at: Option<chrono::NaiveDateTime>,
}

/// Cents as the decimal string a `DECIMAL(10,2)` column takes
/// (`1850` → `"18.50"`; pure — unit tested). Amounts are positive.
pub fn cents_to_decimal(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

fn amount(cents: Option<i64>, currency: Option<String>) -> Option<Amount> {
    cents.map(|cents| Amount { cents, currency })
}

pub struct VolumeValueModel;

impl VolumeValueModel {
    /// The volume's amounts; `None` when the volume or its title is
    /// deleted. `for_update` locks the volume row **alone**, by primary
    /// key (acceptance; lock order proposal → items → volume — review
    /// round 1: a locking read joined to `titles` also locked the title
    /// row); the title's liveness is then a plain read.
    pub async fn find_in(
        conn: &mut MySqlConnection,
        id: u64,
        for_update: bool,
    ) -> Result<Option<VolumeValueState>, AppError> {
        let sql = format!(
            "SELECT v.id, v.title_id, CAST(v.label AS CHAR) AS label, v.version, \
                    CAST(ROUND(v.purchase_price * 100) AS SIGNED) AS purchase_cents, \
                    CAST(v.purchase_currency AS CHAR) AS purchase_currency, \
                    CAST(ROUND(v.current_value * 100) AS SIGNED) AS value_cents, \
                    CAST(v.current_value_currency AS CHAR) AS value_currency, \
                    CAST(v.current_value_updated_at AS DATETIME) AS value_updated_at \
             FROM volumes v WHERE v.id = ? AND v.deleted_at IS NULL{}",
            if for_update { " FOR UPDATE" } else { "" }
        );
        let Some(r) = sqlx::query(&sql).bind(id).fetch_optional(&mut *conn).await? else {
            return Ok(None);
        };
        let title_id: u64 = r.try_get("title_id")?;
        let title_live: Option<i32> =
            sqlx::query_scalar("SELECT 1 FROM titles WHERE id = ? AND deleted_at IS NULL")
                .bind(title_id)
                .fetch_optional(&mut *conn)
                .await?;
        if title_live.is_none() {
            return Ok(None);
        }
        Ok(Some(VolumeValueState {
            id: r.try_get("id")?,
            title_id: r.try_get("title_id")?,
            label: r.try_get("label")?,
            version: r.try_get("version")?,
            purchase_price: amount(r.try_get("purchase_cents")?, r.try_get("purchase_currency")?),
            current_value: amount(r.try_get("value_cents")?, r.try_get("value_currency")?),
            current_value_updated_at: r.try_get("value_updated_at")?,
        }))
    }

    /// What the review screen shows of a live volume: its condition name
    /// and edition note, beside [`Self::find_in`]'s amounts (plain read).
    pub async fn review_facts_in(
        conn: &mut MySqlConnection,
        id: u64,
    ) -> Result<Option<(VolumeValueState, Option<String>, Option<String>)>, AppError> {
        let Some(state) = Self::find_in(conn, id, false).await? else {
            return Ok(None);
        };
        let row = sqlx::query(
            "SELECT CAST(vs.name AS CHAR) AS condition_name, \
                    CAST(v.edition_comment AS CHAR) AS edition_comment \
             FROM volumes v \
             LEFT JOIN volume_states vs ON vs.id = v.condition_state_id AND vs.deleted_at IS NULL \
             WHERE v.id = ? AND v.deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
        let (condition, comment) = match row {
            Some(r) => (r.try_get("condition_name")?, r.try_get("edition_comment")?),
            None => (None, None),
        };
        Ok(Some((state, condition, comment)))
    }

    /// Story 15-5 (D8) — the amounts and edition note of a title's live
    /// volumes, by volume id (the MCP `get_title` read path).
    pub async fn for_title(
        pool: &crate::db::DbPool,
        title_id: u64,
    ) -> Result<std::collections::HashMap<u64, (VolumeValueState, Option<String>)>, AppError> {
        let rows = sqlx::query(
            "SELECT v.id, v.title_id, CAST(v.label AS CHAR) AS label, v.version, \
                    CAST(ROUND(v.purchase_price * 100) AS SIGNED) AS purchase_cents, \
                    CAST(v.purchase_currency AS CHAR) AS purchase_currency, \
                    CAST(ROUND(v.current_value * 100) AS SIGNED) AS value_cents, \
                    CAST(v.current_value_currency AS CHAR) AS value_currency, \
                    CAST(v.current_value_updated_at AS DATETIME) AS value_updated_at, \
                    CAST(v.edition_comment AS CHAR) AS edition_comment \
             FROM volumes v WHERE v.title_id = ? AND v.deleted_at IS NULL",
        )
        .bind(title_id)
        .fetch_all(pool)
        .await?;
        rows.iter()
            .map(|r| {
                let state = VolumeValueState {
                    id: r.try_get("id")?,
                    title_id: r.try_get("title_id")?,
                    label: r.try_get("label")?,
                    version: r.try_get("version")?,
                    purchase_price: amount(
                        r.try_get("purchase_cents")?,
                        r.try_get("purchase_currency")?,
                    ),
                    current_value: amount(r.try_get("value_cents")?, r.try_get("value_currency")?),
                    current_value_updated_at: r.try_get("value_updated_at")?,
                };
                Ok((state.id, (state, r.try_get("edition_comment")?)))
            })
            .collect()
    }

    /// Write an accepted value, the list price as the purchase price, or
    /// both (story 16-3); a value stamps `current_value_updated_at`. Bumps
    /// `version`. Call with the row locked by [`Self::find_in`]; returns the
    /// rows affected (1, or 0 when the volume is gone).
    pub async fn apply_in(
        conn: &mut MySqlConnection,
        id: u64,
        value: Option<(i64, &str)>,
        purchase: Option<(i64, &str)>,
    ) -> Result<u64, AppError> {
        let mut sets = Vec::new();
        if value.is_some() {
            sets.push(
                "current_value = CAST(? AS DECIMAL(10,2)), current_value_currency = ?, \
                 current_value_updated_at = NOW()",
            );
        }
        if purchase.is_some() {
            sets.push("purchase_price = CAST(? AS DECIMAL(10,2)), purchase_currency = ?");
        }
        if sets.is_empty() {
            return Ok(0);
        }
        let sql = format!(
            "UPDATE volumes SET {}, version = version + 1, updated_at = NOW() \
             WHERE id = ? AND deleted_at IS NULL",
            sets.join(", ")
        );
        let mut query = sqlx::query(&sql);
        for (cents, currency) in [value, purchase].into_iter().flatten() {
            query = query.bind(cents_to_decimal(cents)).bind(currency);
        }
        let result = query.bind(id).execute(&mut *conn).await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cents_read_as_a_two_decimal_string() {
        assert_eq!(cents_to_decimal(1850), "18.50");
        assert_eq!(cents_to_decimal(5), "0.05");
        assert_eq!(cents_to_decimal(100), "1.00");
        assert_eq!(cents_to_decimal(9_999_999_999), "99999999.99");
    }
}
