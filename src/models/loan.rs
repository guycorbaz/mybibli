use chrono::NaiveDateTime;
use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{PaginatedList, DEFAULT_PAGE_SIZE};

#[derive(Debug, Clone)]
pub struct LoanModel {
    pub id: u64,
    pub volume_id: u64,
    pub borrower_id: u64,
    pub loaned_at: NaiveDateTime,
    pub returned_at: Option<NaiveDateTime>,
    pub previous_location_id: Option<u64>,
    pub version: i32,
}

/// Loan with joined details for list display.
#[derive(Debug, Clone)]
pub struct LoanWithDetails {
    pub id: u64,
    pub volume_id: u64,
    pub borrower_id: u64,
    pub borrower_name: String,
    pub volume_label: String,
    pub title_name: String,
    pub loaned_at: NaiveDateTime,
    pub duration_days: i64,
}

/// Sort column whitelist for loan list.
const LOAN_SORT_COLUMNS: &[&str] = &["borrower", "title", "date", "duration"];
const SORT_DIRS: &[&str] = &["asc", "desc"];

fn validated_loan_sort(sort: &Option<String>) -> &str {
    match sort {
        Some(s) if LOAN_SORT_COLUMNS.contains(&s.as_str()) => s.as_str(),
        _ => "date",
    }
}

fn validated_dir(dir: &Option<String>) -> &str {
    match dir {
        Some(d) if SORT_DIRS.contains(&d.as_str()) => d.as_str(),
        _ => "desc",
    }
}

fn map_loan_sort_column(sort: &str) -> &str {
    match sort {
        "borrower" => "b.name",
        "title" => "t.title",
        "date" => "l.loaned_at",
        "duration" => "duration_days",
        _ => "l.loaned_at",
    }
}

impl LoanModel {
    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<LoanModel>, AppError> {
        let row = sqlx::query(
            r#"SELECT id, volume_id, borrower_id, loaned_at, returned_at,
                      previous_location_id, version
               FROM loans WHERE id = ? AND deleted_at IS NULL"#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(LoanModel {
                id: r.try_get("id")?,
                volume_id: r.try_get("volume_id")?,
                borrower_id: r.try_get("borrower_id")?,
                loaned_at: r.try_get("loaned_at")?,
                returned_at: r.try_get("returned_at")?,
                previous_location_id: r.try_get("previous_location_id")?,
                version: r.try_get("version")?,
            })),
            None => Ok(None),
        }
    }

    /// Paginated list of active loans (returned_at IS NULL) with borrower/volume/title details.
    pub async fn list_active(
        pool: &DbPool,
        page: u32,
        sort: &Option<String>,
        dir: &Option<String>,
    ) -> Result<PaginatedList<LoanWithDetails>, AppError> {
        let offset = (page.saturating_sub(1)) * DEFAULT_PAGE_SIZE;
        let sort_col = validated_loan_sort(sort);
        let sort_dir = validated_dir(dir);
        let sql_col = map_loan_sort_column(sort_col);

        let count_row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM loans l \
             WHERE l.returned_at IS NULL AND l.deleted_at IS NULL",
        )
        .fetch_one(pool)
        .await?;

        let data_sql = format!(
            r#"SELECT l.id, l.volume_id, l.borrower_id, l.loaned_at,
                      b.name AS borrower_name,
                      v.label AS volume_label,
                      t.title AS title_name,
                      DATEDIFF(NOW(), l.loaned_at) AS duration_days
               FROM loans l
               JOIN borrowers b ON l.borrower_id = b.id AND b.deleted_at IS NULL
               JOIN volumes v ON l.volume_id = v.id AND v.deleted_at IS NULL
               JOIN titles t ON v.title_id = t.id AND t.deleted_at IS NULL
               WHERE l.returned_at IS NULL AND l.deleted_at IS NULL
               ORDER BY {} {}
               LIMIT ? OFFSET ?"#,
            sql_col, sort_dir
        );

        let rows = sqlx::query(&data_sql)
        .bind(DEFAULT_PAGE_SIZE)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        let items: Vec<LoanWithDetails> = rows
            .iter()
            .map(|r| {
                Ok(LoanWithDetails {
                    id: r.try_get("id")?,
                    volume_id: r.try_get("volume_id")?,
                    borrower_id: r.try_get("borrower_id")?,
                    borrower_name: r.try_get("borrower_name")?,
                    volume_label: r.try_get("volume_label")?,
                    title_name: r.try_get("title_name")?,
                    loaned_at: r.try_get("loaned_at")?,
                    duration_days: r.try_get::<i64, _>("duration_days").unwrap_or(0),
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;

        Ok(PaginatedList::new(
            items,
            page,
            count_row.0 as u64,
            Some(sort_col.to_string()),
            Some(sort_dir.to_string()),
            None,
        ))
    }

    /// Check if a volume currently has an active loan.
    pub async fn find_active_by_volume(pool: &DbPool, volume_id: u64) -> Result<Option<LoanModel>, AppError> {
        let row = sqlx::query(
            r#"SELECT id, volume_id, borrower_id, loaned_at, returned_at,
                      previous_location_id, version
               FROM loans
               WHERE volume_id = ? AND returned_at IS NULL AND deleted_at IS NULL"#,
        )
        .bind(volume_id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(LoanModel {
                id: r.try_get("id")?,
                volume_id: r.try_get("volume_id")?,
                borrower_id: r.try_get("borrower_id")?,
                loaned_at: r.try_get("loaned_at")?,
                returned_at: r.try_get("returned_at")?,
                previous_location_id: r.try_get("previous_location_id")?,
                version: r.try_get("version")?,
            })),
            None => Ok(None),
        }
    }

    /// Create a new loan.
    pub async fn create(
        pool: &DbPool,
        volume_id: u64,
        borrower_id: u64,
        previous_location_id: Option<u64>,
    ) -> Result<LoanModel, AppError> {
        tracing::info!(volume_id = volume_id, borrower_id = borrower_id, "Creating loan");

        let result = sqlx::query(
            "INSERT INTO loans (volume_id, borrower_id, previous_location_id) VALUES (?, ?, ?)",
        )
        .bind(volume_id)
        .bind(borrower_id)
        .bind(previous_location_id)
        .execute(pool)
        .await?;

        let id = result.last_insert_id();
        LoanModel::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve created loan".to_string()))
    }

    /// Find an active loan by volume label (for scan-to-find on /loans page).
    pub async fn find_active_by_volume_label(
        pool: &DbPool,
        label: &str,
    ) -> Result<Option<LoanWithDetails>, AppError> {
        let row = sqlx::query(
            r#"SELECT l.id, l.volume_id, l.borrower_id, l.loaned_at,
                      b.name AS borrower_name,
                      v.label AS volume_label,
                      t.title AS title_name,
                      DATEDIFF(NOW(), l.loaned_at) AS duration_days
               FROM loans l
               JOIN volumes v ON l.volume_id = v.id AND v.deleted_at IS NULL
               JOIN borrowers b ON l.borrower_id = b.id AND b.deleted_at IS NULL
               JOIN titles t ON v.title_id = t.id AND t.deleted_at IS NULL
               WHERE v.label = ? AND l.returned_at IS NULL AND l.deleted_at IS NULL"#,
        )
        .bind(label)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(LoanWithDetails {
                id: r.try_get("id")?,
                volume_id: r.try_get("volume_id")?,
                borrower_id: r.try_get("borrower_id")?,
                borrower_name: r.try_get("borrower_name")?,
                volume_label: r.try_get("volume_label")?,
                title_name: r.try_get("title_name")?,
                loaned_at: r.try_get("loaned_at")?,
                duration_days: r.try_get::<i64, _>("duration_days").unwrap_or(0),
            })),
            None => Ok(None),
        }
    }

    /// List active loans for a specific borrower (no pagination — typically few per borrower).
    pub async fn list_active_by_borrower(
        pool: &DbPool,
        borrower_id: u64,
    ) -> Result<Vec<LoanWithDetails>, AppError> {
        let rows = sqlx::query(
            r#"SELECT l.id, l.volume_id, l.borrower_id, l.loaned_at,
                      b.name AS borrower_name,
                      v.label AS volume_label,
                      t.title AS title_name,
                      DATEDIFF(NOW(), l.loaned_at) AS duration_days
               FROM loans l
               JOIN borrowers b ON l.borrower_id = b.id AND b.deleted_at IS NULL
               JOIN volumes v ON l.volume_id = v.id AND v.deleted_at IS NULL
               JOIN titles t ON v.title_id = t.id AND t.deleted_at IS NULL
               WHERE l.borrower_id = ? AND l.returned_at IS NULL AND l.deleted_at IS NULL
               ORDER BY l.loaned_at DESC"#,
        )
        .bind(borrower_id)
        .fetch_all(pool)
        .await?;

        let items: Vec<LoanWithDetails> = rows
            .iter()
            .map(|r| {
                Ok(LoanWithDetails {
                    id: r.try_get("id")?,
                    volume_id: r.try_get("volume_id")?,
                    borrower_id: r.try_get("borrower_id")?,
                    borrower_name: r.try_get("borrower_name")?,
                    volume_label: r.try_get("volume_label")?,
                    title_name: r.try_get("title_name")?,
                    loaned_at: r.try_get("loaned_at")?,
                    duration_days: r.try_get::<i64, _>("duration_days").unwrap_or(0),
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;

        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn test_loan_model_struct() {
        let loan = LoanModel {
            id: 1,
            volume_id: 10,
            borrower_id: 20,
            loaned_at: NaiveDate::from_ymd_opt(2026, 4, 1)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            returned_at: None,
            previous_location_id: Some(5),
            version: 1,
        };
        assert_eq!(loan.id, 1);
        assert_eq!(loan.volume_id, 10);
        assert!(loan.returned_at.is_none());
        assert_eq!(loan.previous_location_id, Some(5));
    }

    #[test]
    fn test_loan_model_returned() {
        let loan = LoanModel {
            id: 2,
            volume_id: 11,
            borrower_id: 21,
            loaned_at: NaiveDate::from_ymd_opt(2026, 3, 1)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            returned_at: Some(
                NaiveDate::from_ymd_opt(2026, 4, 1)
                    .unwrap()
                    .and_hms_opt(10, 0, 0)
                    .unwrap(),
            ),
            previous_location_id: None,
            version: 1,
        };
        assert!(loan.returned_at.is_some());
        assert!(loan.previous_location_id.is_none());
    }

    #[test]
    fn test_validated_loan_sort_valid() {
        assert_eq!(validated_loan_sort(&Some("borrower".to_string())), "borrower");
        assert_eq!(validated_loan_sort(&Some("title".to_string())), "title");
        assert_eq!(validated_loan_sort(&Some("date".to_string())), "date");
        assert_eq!(validated_loan_sort(&Some("duration".to_string())), "duration");
    }

    #[test]
    fn test_validated_loan_sort_invalid() {
        assert_eq!(validated_loan_sort(&Some("invalid".to_string())), "date");
        assert_eq!(validated_loan_sort(&None), "date");
    }

    #[test]
    fn test_validated_dir() {
        assert_eq!(validated_dir(&Some("asc".to_string())), "asc");
        assert_eq!(validated_dir(&Some("desc".to_string())), "desc");
        assert_eq!(validated_dir(&Some("invalid".to_string())), "desc");
        assert_eq!(validated_dir(&None), "desc");
    }

    #[test]
    fn test_map_loan_sort_column() {
        assert_eq!(map_loan_sort_column("borrower"), "b.name");
        assert_eq!(map_loan_sort_column("title"), "t.title");
        assert_eq!(map_loan_sort_column("date"), "l.loaned_at");
        assert_eq!(map_loan_sort_column("duration"), "duration_days");
        assert_eq!(map_loan_sort_column("unknown"), "l.loaned_at");
    }

    #[test]
    fn test_loan_with_details_struct() {
        let details = LoanWithDetails {
            id: 1,
            volume_id: 10,
            borrower_id: 20,
            borrower_name: "Jean Dupont".to_string(),
            volume_label: "V0042".to_string(),
            title_name: "Les Misérables".to_string(),
            loaned_at: NaiveDate::from_ymd_opt(2026, 4, 1)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            duration_days: 5,
        };
        assert_eq!(details.borrower_name, "Jean Dupont");
        assert_eq!(details.volume_label, "V0042");
        assert_eq!(details.title_name, "Les Misérables");
        assert_eq!(details.duration_days, 5);
    }
}
