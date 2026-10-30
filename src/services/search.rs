use crate::db::DbPool;
use crate::error::AppError;
use crate::models::contributor::TitleContributorModel;
use crate::models::genre::GenreModel;
use crate::models::title::{SearchResult, TitleModel};
use crate::models::title_search::{IncompleteTitleRow, MetadataField};
use crate::models::volume::VolumeModel;
use crate::models::{DEFAULT_PAGE_SIZE, PaginatedList};

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
    } else if let Some(isbn13) = crate::utils::isbn10_to_isbn13(&q) {
        // Story 11-1 (#497): titles are stored under their ISBN-13.
        CodeType::Isbn(isbn13)
    } else if (q.len() == 13 && q.chars().all(|c| c.is_ascii_digit()))
        || (q.len() == 10
            && q.as_bytes()[..9].iter().all(u8::is_ascii_digit)
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
            publication_date: title.publication_date,
            dewey_code: title.dewey_code.clone(),
        }
    }
}

/// Story 11-7 — a catalog search as an MCP agent issues it.
///
/// Unlike [`SearchService::search`] (the web UI), it never redirects
/// (an L-code is plain text), browses the whole catalog on an empty
/// query, and applies the filters to exact ISBN / volume-label hits too.
#[derive(Debug, Clone, Default)]
pub struct CatalogSearch {
    /// Free text, an ISBN-10/13 or a volume label (`V0042`).
    pub query: String,
    /// Exact media type (`book`, `bd`, …), already validated.
    pub media_type: Option<String>,
    /// Genre id, already resolved from its name.
    pub genre_id: Option<u64>,
    /// 1-based page.
    pub page: u32,
    /// Story 11-8 — only titles without a cover (NULL or empty
    /// `cover_image_url`, the web "no cover" filter).
    pub no_cover_only: bool,
}

/// Story 11-7 — outcome of resolving a genre name typed by an agent.
#[derive(Debug, PartialEq, Eq)]
pub enum GenreLookup {
    Found(u64),
    /// No live genre has that name; carries the live names so the agent
    /// can correct its call.
    Unknown(Vec<String>),
}

/// Result of a search that may be a redirect (L-code) or a result list.
pub enum SearchOutcome {
    Results(PaginatedList<SearchResult>),
    Redirect(String),
}

impl SearchService {
    /// Perform a search: detect code patterns first, then fall through to fulltext.
    #[allow(clippy::too_many_arguments)]
    pub async fn search(
        pool: &DbPool,
        query: &str,
        genre_id: Option<u64>,
        volume_state: Option<String>,
        sort: &Option<String>,
        dir: &Option<String>,
        page: u32,
        // CR #279 — title without any active volume.
        no_volumes_only: bool,
        // CR #355 — title without a cover image.
        no_cover_only: bool,
    ) -> Result<SearchOutcome, AppError> {
        let trimmed = query.trim();
        // Browse-without-query path: empty query + no filter → no results.
        // Empty query WITH a genre/state filter falls through to `fulltext_search`
        // (pill-driven browse, e.g. clicking "BD" on the home page).
        if trimmed.is_empty()
            && genre_id.is_none()
            && volume_state.is_none()
            && !no_volumes_only
            && !no_cover_only
        {
            return Ok(SearchOutcome::Results(PaginatedList::new(
                vec![],
                1,
                0,
                sort.clone(),
                dir.clone(),
                None,
            )));
        }

        // Empty query + filter set → skip code detection, go straight to fulltext_search.
        if trimmed.is_empty() {
            return Self::fulltext_search(
                pool,
                trimmed,
                genre_id,
                volume_state,
                sort,
                dir,
                page,
                no_volumes_only,
                no_cover_only,
            )
            .await;
        }

        // Story 12-1 (#502): an ISBN typed with separators is looked up
        // bare; every fallback keeps the query as typed.
        match detect_code(&crate::utils::compact_isbn(trimmed)) {
            CodeType::VCode(code) => {
                tracing::info!(code = %code, "V-code lookup");
                let result = VolumeModel::find_by_label_with_title(pool, &code).await?;
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
                        Self::fulltext_search(
                            pool,
                            trimmed,
                            genre_id,
                            volume_state,
                            sort,
                            dir,
                            page,
                            no_volumes_only,
                            no_cover_only,
                        )
                        .await
                    }
                }
            }
            CodeType::LCode(code) => {
                tracing::info!(code = %code, "L-code lookup");
                let location =
                    crate::models::location::LocationModel::find_by_label(pool, &code).await?;
                match location {
                    Some(loc) => Ok(SearchOutcome::Redirect(format!("/location/{}", loc.id))),
                    None => {
                        Self::fulltext_search(
                            pool,
                            trimmed,
                            genre_id,
                            volume_state,
                            sort,
                            dir,
                            page,
                            no_volumes_only,
                            no_cover_only,
                        )
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
                        Self::fulltext_search(
                            pool,
                            trimmed,
                            genre_id,
                            volume_state,
                            sort,
                            dir,
                            page,
                            no_volumes_only,
                            no_cover_only,
                        )
                        .await
                    }
                }
            }
            CodeType::Text => {
                Self::fulltext_search(
                    pool,
                    trimmed,
                    genre_id,
                    volume_state,
                    sort,
                    dir,
                    page,
                    no_volumes_only,
                    no_cover_only,
                )
                .await
            }
        }
    }

    /// Story 11-7 — resolve a genre name among the live genres (the
    /// column collation decides: case- and accent-insensitive).
    pub async fn resolve_genre(pool: &DbPool, name: &str) -> Result<GenreLookup, AppError> {
        if let Some(id) = GenreModel::find_active_id_by_name(pool, name.trim()).await? {
            return Ok(GenreLookup::Found(id));
        }
        let known = GenreModel::list_active(pool).await?;
        Ok(GenreLookup::Unknown(
            known.into_iter().map(|g| g.name).collect(),
        ))
    }

    /// Story 11-7 — the agent's catalog search (see [`CatalogSearch`]).
    /// Pages hold `DEFAULT_PAGE_SIZE` items, sorted by title.
    pub async fn search_catalog(
        pool: &DbPool,
        search: &CatalogSearch,
    ) -> Result<PaginatedList<SearchResult>, AppError> {
        let page = search.page.max(1);
        let trimmed = search.query.trim();
        let exact: Option<Vec<TitleModel>> = match detect_code(&crate::utils::compact_isbn(trimmed)) {
            CodeType::VCode(code) => VolumeModel::find_by_label_with_title(pool, &code)
                .await?
                .map(|(_volume, title)| vec![title]),
            CodeType::Isbn(isbn) => {
                Some(TitleModel::list_by_isbn(pool, &isbn).await?).filter(|t| !t.is_empty())
            }
            CodeType::LCode(_) | CodeType::Text => None,
        };
        // An identifier that exists answers exactly — never a fuzzy text
        // match on the digits, even when the filters exclude it.
        if let Some(titles) = exact {
            let hits: Vec<TitleModel> = titles
                .into_iter()
                .filter(|t| catalog_filters_match(search, t))
                .collect();
            let total = hits.len() as u64;
            let mut items = Vec::new();
            for title in hits
                .iter()
                .skip((page - 1).saturating_mul(DEFAULT_PAGE_SIZE) as usize)
                .take(DEFAULT_PAGE_SIZE as usize)
            {
                items.push(Self::enrich_title(pool, title).await);
            }
            return Ok(PaginatedList::new(items, page, total, None, None, None));
        }
        TitleModel::active_search(
            pool,
            trimmed,
            search.genre_id,
            None,
            &None,
            &None,
            page,
            false,
            search.no_cover_only,
            search.media_type.as_deref(),
        )
        .await
    }

    /// Story 11-8 — one page of live titles missing `only` (or any
    /// tracked field), for the MCP agent (see
    /// [`TitleModel::list_incomplete_metadata`]).
    pub async fn list_incomplete_metadata(
        pool: &DbPool,
        only: Option<MetadataField>,
        media_type: Option<&str>,
        genre_id: Option<u64>,
        page: u32,
    ) -> Result<PaginatedList<IncompleteTitleRow>, AppError> {
        TitleModel::list_incomplete_metadata(pool, only, media_type, genre_id, page.max(1)).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn fulltext_search(
        pool: &DbPool,
        query: &str,
        genre_id: Option<u64>,
        volume_state: Option<String>,
        sort: &Option<String>,
        dir: &Option<String>,
        page: u32,
        no_volumes_only: bool,
        no_cover_only: bool,
    ) -> Result<SearchOutcome, AppError> {
        let results = TitleModel::active_search(
            pool,
            query,
            genre_id,
            volume_state,
            sort,
            dir,
            page,
            no_volumes_only,
            no_cover_only,
            None,
        )
        .await?;
        Ok(SearchOutcome::Results(results))
    }
}

/// Whether an exact hit satisfies the agent's filters.
fn catalog_filters_match(search: &CatalogSearch, title: &TitleModel) -> bool {
    search
        .media_type
        .as_deref()
        .is_none_or(|m| m == title.media_type)
        && search.genre_id.is_none_or(|g| g == title.genre_id)
        && (!search.no_cover_only
            || !crate::services::title_detail::has_cover(title.cover_image_url.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(media_type: &str, genre_id: u64) -> TitleModel {
        TitleModel {
            id: 1,
            title: "T".into(),
            original_title: None,
            subtitle: None,
            statement_of_responsibility: None,
            description: None,
            general_note: None,
            language: "fr".into(),
            media_type: media_type.into(),
            publication_date: None,
            publisher: None,
            edition_statement: None,
            collection_title: None,
            collection_number: None,
            isbn: None,
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id,
            dewey_code: None,
            page_count: None,
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            manually_edited_fields: None,
            version: 1,
        }
    }

    #[test]
    fn catalog_filters_apply_to_exact_hits() {
        let t = title("bd", 3);
        let any = CatalogSearch::default();
        assert!(catalog_filters_match(&any, &t));
        let bd3 = CatalogSearch {
            media_type: Some("bd".into()),
            genre_id: Some(3),
            ..Default::default()
        };
        assert!(catalog_filters_match(&bd3, &t));
        let book = CatalogSearch {
            media_type: Some("book".into()),
            ..Default::default()
        };
        assert!(!catalog_filters_match(&book, &t));
        let genre4 = CatalogSearch {
            genre_id: Some(4),
            ..Default::default()
        };
        assert!(!catalog_filters_match(&genre4, &t));
        let no_cover = CatalogSearch {
            no_cover_only: true,
            ..Default::default()
        };
        assert!(catalog_filters_match(&no_cover, &t));
        let mut covered = title("bd", 3);
        covered.cover_image_url = Some("/covers/1.jpg".into());
        assert!(!catalog_filters_match(&no_cover, &covered));
    }

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

    // Story 12-1 (#502): the web search compacts before `detect_code`.
    #[test]
    fn test_detect_compacted_isbn_with_separators() {
        let detect = |q: &str| detect_code(&crate::utils::compact_isbn(q));
        assert_eq!(
            detect("978-2-07-036024-6"),
            CodeType::Isbn("9782070360246".to_string())
        );
        assert_eq!(
            detect("0-201-63361-2"),
            CodeType::Isbn("9780201633610".to_string())
        );
        assert_eq!(detect("Spider-Man 3"), CodeType::Text);
    }

    #[test]
    fn test_detect_isbn10() {
        // Bad ISBN-10 checksum: the shape is still looked up raw, then
        // falls through to fulltext (pre-11-1 behaviour, D-11-1-5).
        assert_eq!(
            detect_code("207036024X"),
            CodeType::Isbn("207036024X".to_string())
        );
        // Story 11-1 — a valid ISBN-10 is searched under its ISBN-13.
        assert_eq!(
            detect_code("0201633612"),
            CodeType::Isbn("9780201633610".to_string())
        );
        assert_eq!(
            detect_code(" 2205001213 "),
            CodeType::Isbn("9782205001211".to_string())
        );
        assert_eq!(
            detect_code("080442957x"),
            CodeType::Isbn("9780804429573".to_string())
        );
        // 10 bytes with a multi-byte char straddling byte 9: no panic.
        assert_eq!(detect_code("22050012é"), CodeType::Text);
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
