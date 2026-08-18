pub mod admin_audit;
pub mod api_key;
pub mod borrower;
pub mod contributor;
pub mod contributor_role;
pub mod genre;
pub mod label;
pub mod loan;
pub mod location;
pub mod location_node_type;
pub mod media_type;
pub mod metadata_cache;
pub mod saved_search;
pub mod series;
pub mod session;
pub mod title;
pub mod trash;
pub mod user;
pub mod volume;
pub mod volume_state;
pub mod wishlist;

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

    /// #20 — Windowed pagination entries (First | … | window | … | Last).
    ///
    /// Pre-fix the home template walked `1..=total_pages` and emitted one
    /// `<a>` per page. A 10 000-title catalog → 400 pages → 400 `<a>` in
    /// the DOM (slow render, poor UX, ridiculous tab order). A
    /// `?page=999999` in the URL would render an even bigger block.
    ///
    /// This helper returns at most `2 * (WINDOW_RADIUS=3) + 1` pages
    /// around the current page plus first / last anchors, with
    /// `Ellipsis` markers wherever a gap larger than one page exists.
    /// Bounded output: 11 entries max regardless of `total_pages`.
    /// Template iterates over the returned `Vec<PageEntry>` and emits an
    /// anchor for `Page(n)` or a literal `…` span for `Ellipsis`.
    pub fn windowed_pages(&self) -> Vec<PageEntry> {
        use std::collections::BTreeSet;
        const WINDOW_RADIUS: u32 = 3;
        let total = self.total_pages;
        if total == 0 {
            return Vec::new();
        }
        let current = self.page.clamp(1, total);

        // Always include first, last, and current ± radius. BTreeSet
        // both dedups and orders for the gap-detection sweep below.
        let mut pages = BTreeSet::new();
        pages.insert(1);
        pages.insert(total);
        let start = current.saturating_sub(WINDOW_RADIUS).max(1);
        let end = current.saturating_add(WINDOW_RADIUS).min(total);
        for p in start..=end {
            pages.insert(p);
        }

        let mut out = Vec::with_capacity(pages.len() + 2);
        let mut prev: Option<u32> = None;
        for p in &pages {
            if let Some(prev_p) = prev
                && *p > prev_p + 1
            {
                out.push(PageEntry::Ellipsis);
            }
            out.push(PageEntry::Page(*p));
            prev = Some(*p);
        }
        out
    }
}

/// #20 — one entry in the windowed pagination output.
///
/// `Page(n)` renders as an anchor in the template; `Ellipsis` renders as
/// a non-interactive `<span aria-hidden="true">…</span>` so screen
/// readers don't announce a gap as a clickable page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageEntry {
    Page(u32),
    Ellipsis,
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

    // ─── #20 — windowed pagination coverage ─────────────────────────

    /// Sub-7 page sets emit no ellipsis — the contiguous range fits.
    #[test]
    fn windowed_pages_small_total_is_contiguous() {
        let list: PaginatedList<u32> = PaginatedList::new(vec![], 3, 6 * 25, None, None, None);
        // 150 items / 25 per page = 6 pages, current = 3.
        assert_eq!(list.total_pages, 6);
        let pages = list.windowed_pages();
        assert_eq!(
            pages,
            vec![
                PageEntry::Page(1),
                PageEntry::Page(2),
                PageEntry::Page(3),
                PageEntry::Page(4),
                PageEntry::Page(5),
                PageEntry::Page(6),
            ],
        );
    }

    /// Large total with current in the middle: First | … | window | … | Last.
    /// Locks the bound: at most 11 entries (2 anchors + 7 window + 2
    /// ellipses).
    #[test]
    fn windowed_pages_large_total_emits_bounded_window() {
        // 10_000 items / 25 per page = 400 pages — the issue scenario.
        let list: PaginatedList<u32> =
            PaginatedList::new(vec![], 10, 10_000, None, None, None);
        assert_eq!(list.total_pages, 400);
        let pages = list.windowed_pages();
        assert_eq!(
            pages,
            vec![
                PageEntry::Page(1),
                PageEntry::Ellipsis,
                PageEntry::Page(7),
                PageEntry::Page(8),
                PageEntry::Page(9),
                PageEntry::Page(10),
                PageEntry::Page(11),
                PageEntry::Page(12),
                PageEntry::Page(13),
                PageEntry::Ellipsis,
                PageEntry::Page(400),
            ],
        );
        assert!(
            pages.len() <= 11,
            "windowed pagination must stay bounded; got {}",
            pages.len()
        );
    }

    /// Current near the start — the leading ellipsis is suppressed
    /// because the window touches page 1.
    #[test]
    fn windowed_pages_current_near_start() {
        let list: PaginatedList<u32> =
            PaginatedList::new(vec![], 2, 10_000, None, None, None);
        let pages = list.windowed_pages();
        // Pages 1..=5 contiguous, then ellipsis, then 400.
        assert_eq!(
            pages,
            vec![
                PageEntry::Page(1),
                PageEntry::Page(2),
                PageEntry::Page(3),
                PageEntry::Page(4),
                PageEntry::Page(5),
                PageEntry::Ellipsis,
                PageEntry::Page(400),
            ],
        );
    }

    /// Current near the end — trailing ellipsis suppressed for the same
    /// reason on the other side.
    #[test]
    fn windowed_pages_current_near_end() {
        let list: PaginatedList<u32> =
            PaginatedList::new(vec![], 399, 10_000, None, None, None);
        let pages = list.windowed_pages();
        assert_eq!(
            pages,
            vec![
                PageEntry::Page(1),
                PageEntry::Ellipsis,
                PageEntry::Page(396),
                PageEntry::Page(397),
                PageEntry::Page(398),
                PageEntry::Page(399),
                PageEntry::Page(400),
            ],
        );
    }

    /// `?page=999999` (way out of range) — the issue's second sub-item.
    /// The window must still be bounded. `current` is clamped to
    /// `total_pages`, so the result is the same as "current = last".
    #[test]
    fn windowed_pages_out_of_range_current_is_clamped() {
        let list: PaginatedList<u32> =
            PaginatedList::new(vec![], 999_999, 10_000, None, None, None);
        let pages = list.windowed_pages();
        assert_eq!(
            pages,
            vec![
                PageEntry::Page(1),
                PageEntry::Ellipsis,
                PageEntry::Page(397),
                PageEntry::Page(398),
                PageEntry::Page(399),
                PageEntry::Page(400),
            ],
        );
        assert!(
            pages.len() <= 11,
            "out-of-range current must still produce a bounded window; got {}",
            pages.len()
        );
    }

    /// total_pages == 0 (edge case: empty list with current = 1) — return
    /// an empty Vec so the template skips the nav entirely.
    #[test]
    fn windowed_pages_zero_total_returns_empty() {
        let mut list: PaginatedList<u32> = PaginatedList::new(vec![], 1, 0, None, None, None);
        list.total_pages = 0; // force the zero case the constructor avoids
        assert_eq!(list.windowed_pages(), Vec::<PageEntry>::new());
    }
}
