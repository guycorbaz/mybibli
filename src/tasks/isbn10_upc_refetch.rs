//! Story 13-3 (#584) — metadata of the titles converted at startup by
//! [`crate::services::title::convert_isbn10_upc_titles`] is looked up again
//! under their new ISBN.
//!
//! One background task walks the list sequentially, with the admin-configured
//! inter-title pause (`bulk_refetch_delay_ms`, #419) between two lookups, so a
//! large conversion cannot start a 503 storm. Each title goes through the
//! scan wrapper [`super::metadata_fetch::fetch_metadata_chain`] with
//! `force_refresh = true`: `manually_edited_fields` is respected as on any
//! fetch, and a throttled lookup is queued for the retry task (story 13-2).

use std::time::Duration;

use crate::AppState;
use crate::models::media_type::{CodeType, MediaType};
use crate::services::title::ConvertedTitle;

/// Pause before the first lookup, so the re-fetch does not compete with
/// the first requests after boot.
const WARM_UP_SECS: u64 = 30;

/// Spawn the re-fetch task; nothing is spawned for an empty list.
pub fn spawn(state: AppState, titles: Vec<ConvertedTitle>) {
    if titles.is_empty() {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(WARM_UP_SECS)).await;
        run(&state, titles).await;
    });
}

/// Look each title up again, one after the other. Callable on its own.
pub async fn run(state: &AppState, titles: Vec<ConvertedTitle>) {
    let count = titles.len();
    tracing::info!(count, "ISBN-10 stored as UPC: re-fetching converted titles");
    for (i, title) in titles.into_iter().enumerate() {
        if i > 0 {
            let delay_ms = state.bulk_refetch_delay_ms();
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
        let timeout_secs = state
            .settings
            .read()
            .map(|s| s.metadata_fetch_timeout_secs)
            .unwrap_or(30);
        let media_type = title
            .media_type
            .parse::<MediaType>()
            .unwrap_or(MediaType::Book);
        crate::tasks::metadata_fetch::fetch_metadata_chain(
            state.pool.clone(),
            title.title_id,
            title.isbn13,
            CodeType::Isbn,
            media_type,
            state.registry.clone(),
            timeout_secs,
            state.metadata_chain_provider_timeouts(),
            state.http_client.clone(),
            state.covers_dir.clone(),
            true,
        )
        .await;
    }
    tracing::info!(
        count,
        "ISBN-10 stored as UPC: re-fetch of converted titles done"
    );
}
