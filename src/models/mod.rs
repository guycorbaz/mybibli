pub mod borrower;
pub mod contributor;
pub mod genre;
pub mod loan;
pub mod location;
pub mod media_type;
pub mod metadata_cache;
pub mod series;
pub mod session;
pub mod title;
pub mod volume;
pub mod volume_state;

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
