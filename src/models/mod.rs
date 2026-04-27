pub mod admin_audit;
pub mod borrower;
pub mod contributor;
pub mod contributor_role;
pub mod genre;
pub mod loan;
pub mod location;
pub mod location_node_type;
pub mod media_type;
pub mod metadata_cache;
pub mod series;
pub mod session;
pub mod title;
pub mod trash;
pub mod user;
pub mod volume;
pub mod volume_state;

/// Outcome of inserting a reference-data row (story 8-4). When the unique
/// `name` constraint collides with a soft-deleted row, the model layer
/// transparently reactivates the row (clears `deleted_at`) so admins can
/// recreate a previously deleted entry without surfacing a "name taken"
/// error. The handler picks the user-facing FeedbackEntry copy based on
/// which variant comes back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    Created(u64),
    Reactivated(u64),
}

impl CreateOutcome {
    pub fn id(&self) -> u64 {
        match self {
            CreateOutcome::Created(id) | CreateOutcome::Reactivated(id) => *id,
        }
    }
}

/// String marker carried by `AppError::Conflict` from a reference-data
/// model's `create()` or `rename()` when a UNIQUE-name collision against an
/// active (non-soft-deleted) row is detected. The reference-data handler's
/// `map_create_or_rename_conflict` translates this marker into the localized
/// `error.reference_data.name_taken` message. Story 8-4 P13 — replaces the
/// scattered `"name_taken"` literal so a future model can't silently deviate
/// (which would leak the internal token into the user-facing feedback).
pub const CONFLICT_NAME_TAKEN: &str = "name_taken";

/// Outcome of a guarded soft-delete attempt (story 8-4 P1).
///
/// `Deleted` — the row was soft-deleted atomically (count + UPDATE in one tx
/// with `SELECT … FOR UPDATE` on the ref row, closing the TOCTOU window where
/// a concurrent INSERT could attach to a row that was just counted as zero).
/// `InUse(count)` — the row is referenced by `count` active rows; soft-delete
/// was rolled back. The handler renders a localized 409 with the count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    InUse(i64),
}

/// Fixed page size for all paginated list views.
pub const DEFAULT_PAGE_SIZE: u32 = 25;

/// Generic paginated list for any entity type.
#[derive(Debug, Clone)]
pub struct PaginatedList<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub total_pages: u32,
    pub total_items: u64,
    pub sort: Option<String>,
    pub dir: Option<String>,
    pub filter: Option<String>,
}

impl<T> PaginatedList<T> {
    pub fn new(
        items: Vec<T>,
        page: u32,
        total_items: u64,
        sort: Option<String>,
        dir: Option<String>,
        filter: Option<String>,
    ) -> Self {
        let total_pages = if total_items == 0 {
            1
        } else {
            ((total_items as f64) / (DEFAULT_PAGE_SIZE as f64)).ceil() as u32
        };
        PaginatedList {
            items,
            page,
            total_pages,
            total_items,
            sort,
            dir,
            filter,
        }
    }

    pub fn has_previous(&self) -> bool {
        self.page > 1
    }

    pub fn has_next(&self) -> bool {
        self.page < self.total_pages
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paginated_list_single_page() {
        let list: PaginatedList<u32> = PaginatedList::new(vec![1, 2, 3], 1, 3, None, None, None);
        assert_eq!(list.total_pages, 1);
        assert!(!list.has_previous());
        assert!(!list.has_next());
    }

    #[test]
    fn test_paginated_list_multiple_pages() {
        let list: PaginatedList<u32> = PaginatedList::new(
            vec![1; 25],
            1,
            60,
            Some("title".to_string()),
            Some("asc".to_string()),
            None,
        );
        assert_eq!(list.total_pages, 3);
        assert!(!list.has_previous());
        assert!(list.has_next());
    }

    #[test]
    fn test_paginated_list_middle_page() {
        let list: PaginatedList<u32> =
            PaginatedList::new(vec![1; 25], 2, 60, None, None, Some("genre:3".to_string()));
        assert_eq!(list.total_pages, 3);
        assert!(list.has_previous());
        assert!(list.has_next());
    }

    #[test]
    fn test_paginated_list_last_page() {
        let list: PaginatedList<u32> = PaginatedList::new(vec![1; 10], 3, 60, None, None, None);
        assert_eq!(list.total_pages, 3);
        assert!(list.has_previous());
        assert!(!list.has_next());
    }

    #[test]
    fn test_paginated_list_zero_items() {
        let list: PaginatedList<u32> = PaginatedList::new(vec![], 1, 0, None, None, None);
        assert_eq!(list.total_pages, 1);
        assert!(!list.has_previous());
        assert!(!list.has_next());
    }

    #[test]
    fn test_paginated_list_exactly_25_items() {
        let list: PaginatedList<u32> = PaginatedList::new(vec![1; 25], 1, 25, None, None, None);
        assert_eq!(list.total_pages, 1);
    }

    #[test]
    fn test_paginated_list_26_items() {
        let list: PaginatedList<u32> = PaginatedList::new(vec![1; 25], 1, 26, None, None, None);
        assert_eq!(list.total_pages, 2);
    }
}
