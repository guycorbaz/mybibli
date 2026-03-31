use crate::db::DbPool;
use crate::error::AppError;
use crate::models::contributor::TitleContributorModel;
use crate::models::genre::GenreModel;
use crate::models::title::{SearchResult, TitleModel};
use crate::models::volume::VolumeModel;
use crate::models::PaginatedList;

/// Detected code type from search input.
#[derive(Debug, PartialEq)]
pub enum CodeType {
    VCode(String),
    LCode(String),
    Isbn(String),
    Text,
}

/// Detect if the query is a code (V-code, L-code, ISBN) or plain text.
pub fn detect_code(query: &str) -> CodeType {
    let q = query.trim().to_uppercase();
    if q.len() == 5 && q.starts_with('V') && q[1..].chars().all(|c| c.is_ascii_digit()) {
        CodeType::VCode(q)
    } else if q.len() == 5 && q.starts_with('L') && q[1..].chars().all(|c| c.is_ascii_digit()) {
        CodeType::LCode(q)
    } else if (q.len() == 13 && q.chars().all(|c| c.is_ascii_digit()))
        || (q.len() == 10
            && q[..9].chars().all(|c| c.is_ascii_digit())
            && q.ends_with(|c: char| c.is_ascii_digit() || c == 'X'))
    {
        CodeType::Isbn(q)
    } else {
        CodeType::Text
    }
}

pub struct SearchService;

impl SearchService {
    /// Build a complete SearchResult from a TitleModel by looking up genre and contributor.
    async fn enrich_title(pool: &DbPool, title: &TitleModel) -> SearchResult {
        let genre_name = GenreModel::find_name_by_id(pool, title.genre_id)
            .await
            .unwrap_or_else(|e| { tracing::warn!(error = %e, title_id = title.id, "Failed to load genre for search result"); String::new() });
        let primary_contributor = TitleContributorModel::get_primary_contributor(pool, title.id)
            .await
            .unwrap_or_else(|e| { tracing::warn!(error = %e, title_id = title.id, "Failed to load contributor for search result"); None });
        let volume_count = VolumeModel::count_by_title(pool, title.id)
            .await
            .unwrap_or_else(|e| { tracing::warn!(error = %e, title_id = title.id, "Failed to count volumes for search result"); 0 });
        SearchResult {
            id: title.id,
            title: title.title.clone(),
            subtitle: title.subtitle.clone(),
            media_type: title.media_type.clone(),
            genre_name,
            primary_contributor,
            volume_count,
            cover_image_url: title.cover_image_url.clone(),
        }
    }
}

/// Result of a search that may be a redirect (L-code) or a result list.
pub enum SearchOutcome {
    Results(PaginatedList<SearchResult>),
    Redirect(String),
}

impl SearchService {
    /// Perform a search: detect code patterns first, then fall through to fulltext.
    pub async fn search(
        pool: &DbPool,
        query: &str,
        genre_id: Option<u64>,
        volume_state: Option<String>,
        sort: &Option<String>,
        dir: &Option<String>,
        page: u32,
    ) -> Result<SearchOutcome, AppError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(SearchOutcome::Results(PaginatedList::new(
                vec![],
                1,
                0,
                sort.clone(),
                dir.clone(),
                None,
            )));
        }

        match detect_code(trimmed) {
            CodeType::VCode(code) => {
                tracing::info!(code = %code, "V-code lookup");
                let result =
                    VolumeModel::find_by_label_with_title(pool, &code).await?;
                match result {
                    Some((_vol, title)) => {
                        let item = Self::enrich_title(pool, &title).await;
                        Ok(SearchOutcome::Results(PaginatedList::new(
                            vec![item],
                            1,
                            1,
                            None,
                            None,
                            None,
                        )))
                    }
                    None => {
                        // Fall through to text search
                        Self::fulltext_search(pool, trimmed, genre_id, volume_state, sort, dir, page)
                            .await
                    }
                }
            }
            CodeType::LCode(code) => {
                tracing::info!(code = %code, "L-code lookup");
                let location =
                    crate::models::location::LocationModel::find_by_label(pool, &code)
                        .await?;
                match location {
                    Some(loc) => Ok(SearchOutcome::Redirect(format!("/location/{}", loc.id))),
                    None => {
                        Self::fulltext_search(pool, trimmed, genre_id, volume_state, sort, dir, page)
                            .await
                    }
                }
            }
            CodeType::Isbn(isbn) => {
                tracing::info!(isbn = %isbn, "ISBN lookup");
                let title = TitleModel::find_by_isbn(pool, &isbn).await?;
                match title {
                    Some(t) => {
                        let item = Self::enrich_title(pool, &t).await;
                        Ok(SearchOutcome::Results(PaginatedList::new(
                            vec![item],
                            1,
                            1,
                            None,
                            None,
                            None,
                        )))
                    }
                    None => {
                        Self::fulltext_search(pool, trimmed, genre_id, volume_state, sort, dir, page)
                            .await
                    }
                }
            }
            CodeType::Text => {
                Self::fulltext_search(pool, trimmed, genre_id, volume_state, sort, dir, page).await
            }
        }
    }

    async fn fulltext_search(
        pool: &DbPool,
        query: &str,
        genre_id: Option<u64>,
        volume_state: Option<String>,
        sort: &Option<String>,
        dir: &Option<String>,
        page: u32,
    ) -> Result<SearchOutcome, AppError> {
        let results =
            TitleModel::active_search(pool, query, genre_id, volume_state, sort, dir, page)
                .await?;
        Ok(SearchOutcome::Results(results))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_vcode() {
        assert_eq!(detect_code("V0042"), CodeType::VCode("V0042".to_string()));
        assert_eq!(detect_code("v0001"), CodeType::VCode("V0001".to_string()));
        assert_eq!(detect_code(" V0042 "), CodeType::VCode("V0042".to_string()));
    }

    #[test]
    fn test_detect_lcode() {
        assert_eq!(detect_code("L0003"), CodeType::LCode("L0003".to_string()));
        assert_eq!(detect_code("l0001"), CodeType::LCode("L0001".to_string()));
    }

    #[test]
    fn test_detect_isbn13() {
        assert_eq!(
            detect_code("9782070360246"),
            CodeType::Isbn("9782070360246".to_string())
        );
    }

    #[test]
    fn test_detect_isbn10() {
        assert_eq!(
            detect_code("207036024X"),
            CodeType::Isbn("207036024X".to_string())
        );
        assert_eq!(
            detect_code("0201633612"),
            CodeType::Isbn("0201633612".to_string())
        );
    }

    #[test]
    fn test_detect_text() {
        assert_eq!(detect_code("desert tartares"), CodeType::Text);
        assert_eq!(detect_code("tintin"), CodeType::Text);
        assert_eq!(detect_code("V00"), CodeType::Text); // Too short for V-code
        assert_eq!(detect_code("V00001"), CodeType::Text); // Too long for V-code
        assert_eq!(detect_code("VABCD"), CodeType::Text); // Non-digits
    }

    #[test]
    fn test_detect_empty() {
        assert_eq!(detect_code(""), CodeType::Text);
        assert_eq!(detect_code("   "), CodeType::Text);
    }

    #[test]
    fn test_detect_injection_attempts() {
        assert_eq!(detect_code("DROP TABLE"), CodeType::Text);
        assert_eq!(detect_code("1; DROP TABLE--"), CodeType::Text);
    }
}
