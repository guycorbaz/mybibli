//! Story 11-18 (FR138, FR140, FR144, FR146, #533) — accepting a `cover`
//! proposal.
//!
//! The item (shape: module doc of [`crate::services::cover_proposals`],
//! D-11-14-5) names a staged JPEG. Acceptance runs in three steps:
//!
//! 1. **before** the transaction, the staged file must exist (no file
//!    I/O inside a decision transaction, D-11-9-12) — else the
//!    acceptance is refused ([`AcceptError::CoverMissing`], #533) and the
//!    proposal stays pending: the librarian rejects it;
//! 2. **one transaction** (shared steps in
//!    [`crate::services::acceptance`]): lock, `version`, items,
//!    [`ProposalService::check_staleness_in`] (title deleted — a moved
//!    `version` no longer counts for a cover, story 12-2, #542), then
//!    [`crate::services::proposals::title_moved`] (re-identified title),
//!    then the live `cover_image_url` compared with the `current` value
//!    recorded at filing — every cover write (upload, fetch, accepted
//!    proposal) stores a cache-busted URL, so any change of image changes
//!    it — and another cover proposal accepted since filing (its image is
//!    installed only after its commit); either marks the proposal
//!    `stale`. Otherwise the decision is recorded, `cover_image_url`
//!    marked as manually edited (FR144 — a background cover fetch landing
//!    before the promotion leaves the title alone) and `titles.version`
//!    bumped — which outdates no metadata proposal any more;
//! 3. **after** the commit, [`cover_staging::promote`] moves the file to
//!    `covers/{title_id}.jpg` (replacing the current cover), then
//!    `cover_image_url` is set with a cache-busting suffix. If the
//!    promotion fails (the file vanished meanwhile, an I/O error), the
//!    cover is left as it was and [`AppliedCover::installed`] is `false`:
//!    the proposal is accepted, the librarian is told the image could not
//!    be installed. (The staging purge keeps an accepted proposal's file
//!    for an hour, D-11-14-8, so only an outside deletion races here.)

use serde_json::Value;
use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::proposal::{
    ProposalItemRow, ProposalKind, ProposalModel, ProposalStatus, ProposalTargetTable,
};
use crate::models::title::TitleModel;
use crate::services::acceptance::{
    AcceptConflict, AcceptError, AcceptOutcome, CoverDirs, StaleCause, in_retried_transaction,
    lock_pending_in, mark_stale_in, mark_title_moved_in,
};
use crate::services::cover_staging;
use crate::services::proposals::{Decision, ProposalService, Transition};

/// A stored cover proposal, parsed back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCover {
    pub title_id: u64,
    /// Checked with [`cover_staging::is_staged_name`].
    pub staged_file: String,
    /// `cover_image_url` at filing.
    pub current: Option<String>,
}

/// Parse the stored items of a cover proposal (pure — unit tested):
/// exactly one `"line": "cover"` item targeting a title, with a
/// well-formed staged name.
pub fn parse_items(items: &[ProposalItemRow]) -> Result<StoredCover, String> {
    let [row] = items else {
        return Err(format!("{} items, one expected", items.len()));
    };
    let title_id = match row.target {
        Some(t) if t.table == ProposalTargetTable::Titles => t.id,
        _ => return Err(format!("item {}: a cover targets a title", row.id)),
    };
    let v: Value = serde_json::from_str(&row.proposed_values_json)
        .map_err(|e| format!("item {}: invalid JSON: {e}", row.id))?;
    if v.get("line").and_then(Value::as_str) != Some("cover") {
        return Err(format!("item {}: not a cover line", row.id));
    }
    let staged_file = v
        .get("staged_file")
        .and_then(Value::as_str)
        .filter(|n| cover_staging::is_staged_name(n))
        .ok_or_else(|| format!("item {}: no valid staged file", row.id))?
        .to_string();
    let current = match v.get("current") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(format!("item {}: current cover", row.id)),
    };
    Ok(StoredCover {
        title_id,
        staged_file,
        current,
    })
}

/// Story 12-2 (review): another cover proposal for the title accepted
/// since this one was filed — its image may not be installed yet.
///
/// Lock order: this share-locks other proposals' rows **after** the
/// title row, the reverse of proposal → items → title. Two simultaneous
/// decisions on one title may then deadlock; `in_retried_transaction`
/// retries on 40001 and the loser re-runs behind the title lock (correct
/// outcome; with three or more simultaneous decisions the retry budget
/// can run out — single-librarian load, accepted, review round 3).
async fn cover_accepted_since_filing(
    conn: &mut MySqlConnection,
    id: u64,
    title_id: u64,
) -> Result<bool, AppError> {
    let Some(filed_at) = ProposalModel::find(conn, id).await?.map(|p| p.created_at) else {
        return Ok(false);
    };
    ProposalModel::cover_accepted_since(conn, title_id, filed_at, id).await
}

/// Whether the live cover differs from the one recorded at filing
/// (pure); an empty URL counts as no cover.
pub fn cover_changed(stored: Option<&str>, live: Option<&str>) -> bool {
    fn norm(v: Option<&str>) -> Option<&str> {
        v.filter(|s| !s.trim().is_empty())
    }
    norm(stored) != norm(live)
}

/// The URL written for a promoted cover: the public path plus a
/// cache-busting suffix, so browsers drop the previous image.
pub fn cache_busted(path: &str, unix_time: i64) -> String {
    format!("{path}?v={unix_time}")
}

/// What a cover acceptance applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedCover {
    pub title_id: u64,
    pub title: String,
    /// Whether the title had a cover that was replaced.
    pub replaced: bool,
    /// Whether the image is now the title's cover (promotion and URL
    /// write succeeded after the commit).
    pub installed: bool,
    staged_file: String,
    /// Whether the cover was already marked as set by hand before this
    /// acceptance (a failed promotion then keeps the mark).
    was_hand_set: bool,
}

impl AppliedCover {
    /// A value for message tests (the staged name is private).
    #[cfg(test)]
    pub fn for_test(title_id: u64, title: &str, replaced: bool, installed: bool) -> Self {
        AppliedCover {
            title_id,
            title: title.to_string(),
            replaced,
            installed,
            staged_file: String::new(),
            was_hand_set: false,
        }
    }
}

/// Accept cover proposal `id` on behalf of `user_id` (see the module
/// doc for the three steps).
pub async fn accept(
    pool: &DbPool,
    dirs: CoverDirs<'_>,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome<AppliedCover>, AcceptError> {
    // Step 1 — the staged file, checked on a plain read. Only a pending
    // cover proposal is checked: anything else gets its own refusal from
    // the transaction (already decided, wrong kind…).
    {
        let mut conn = pool.acquire().await?;
        if let Some(p) = ProposalModel::find(&mut conn, id).await?
            && p.kind == ProposalKind::Cover
            && p.status == ProposalStatus::Pending
        {
            let items = ProposalModel::items(&mut conn, id, false).await?;
            let stored = parse_items(&items).map_err(AcceptError::Invalid)?;
            let present = match dirs.staging_dir {
                Some(dir) => tokio::fs::try_exists(dir.join(&stored.staged_file))
                    .await
                    .unwrap_or(false),
                None => false,
            };
            if !present {
                tracing::warn!(
                    proposal_id = id,
                    "Cover acceptance refused: staged image missing"
                );
                return Err(AcceptError::CoverMissing);
            }
        }
    }

    // Step 2 — the decision.
    let outcome = in_retried_transaction(pool, id, user_id, "cover", |conn| {
        Box::pin(accept_in(conn, id, user_id, version))
    })
    .await?;

    // Step 3 — promotion and URL, after the commit.
    let AcceptOutcome::Accepted(mut applied) = outcome else {
        return Ok(outcome);
    };
    applied.installed = install(pool, dirs, &applied).await;
    Ok(AcceptOutcome::Accepted(applied))
}

async fn accept_in(
    conn: &mut MySqlConnection,
    id: u64,
    user_id: u64,
    version: i32,
) -> Result<AcceptOutcome<AppliedCover>, AcceptError> {
    let (_, items) = lock_pending_in(conn, id, version, &[ProposalKind::Cover]).await?;
    let stored = parse_items(&items).map_err(AcceptError::Invalid)?;
    let check = ProposalService::check_staleness_in(conn, id).await?;
    if check.status != ProposalStatus::Pending {
        return Ok(AcceptOutcome::Stale(StaleCause::Targets(
            check.stale_targets,
        )));
    }
    let title = TitleModel::find_by_id_in(conn, stored.title_id, true)
        .await?
        .ok_or_else(|| {
            AcceptError::Apply(AppError::Internal(
                "proposal title gone although not stale".to_string(),
            ))
        })?;
    if crate::services::proposals::title_moved(&items, &title) {
        return mark_title_moved_in(conn, id, &items).await;
    }
    if cover_changed(stored.current.as_deref(), title.cover_image_url.as_deref())
        || cover_accepted_since_filing(conn, id, title.id).await?
    {
        return mark_stale_in(conn, id, AcceptConflict::CoverChanged).await;
    }
    match ProposalService::transition_in(conn, id, &Decision::Accept { by: user_id }).await? {
        Transition::Decided(_) => {}
        // Unreachable: the same locks are held since the check above.
        Transition::WentStale(targets) => {
            return Ok(AcceptOutcome::Stale(StaleCause::Targets(targets)));
        }
    }
    TitleModel::mark_cover_accepted_in(
        conn,
        title.id,
        &title.manually_edited_with(&["cover_image_url"]),
    )
    .await?;
    Ok(AcceptOutcome::Accepted(AppliedCover {
        title_id: title.id,
        // "Changed from no cover" = the title had one.
        replaced: cover_changed(None, title.cover_image_url.as_deref()),
        title: title.title.clone(),
        installed: false,
        staged_file: stored.staged_file,
        was_hand_set: title
            .parsed_manually_edited_fields()
            .iter()
            .any(|f| f == "cover_image_url"),
    }))
}

/// Promote the staged file and record its URL; `false` (logged) when
/// either fails. A failed promotion leaves the files and the URL as they
/// were and takes back the "set by hand" mark the decision added (so
/// automatic cover fetches keep working for the title); a URL write that
/// fails after a successful promotion leaves the new image in
/// `covers/{id}.jpg` under the old URL (logged — D-11-18-10).
async fn install(pool: &DbPool, dirs: CoverDirs<'_>, applied: &AppliedCover) -> bool {
    let promoted = match dirs.staging_dir {
        None => Err("staging unavailable".to_string()),
        Some(staging) => cover_staging::promote(
            staging,
            dirs.covers_dir,
            &applied.staged_file,
            applied.title_id,
        )
        .await
        .map_err(|e| e.to_string()),
    };
    let path = match promoted {
        Ok(path) => path,
        Err(e) => {
            tracing::error!(title_id = applied.title_id, error = %e, "Accepted cover not installed");
            if !applied.was_hand_set
                && let Err(e) =
                    TitleModel::unmark_manually_edited(pool, applied.title_id, "cover_image_url")
                        .await
            {
                tracing::warn!(title_id = applied.title_id, error = %e, "Hand-set cover mark not removed");
            }
            return false;
        }
    };
    let url = cache_busted(&path, chrono::Utc::now().timestamp());
    match TitleModel::set_cover_url(pool, applied.title_id, &url).await {
        Ok(true) => true,
        Ok(false) => {
            tracing::warn!(
                title_id = applied.title_id,
                "Accepted cover promoted but the title was deleted meanwhile"
            );
            false
        }
        Err(e) => {
            tracing::error!(
                title_id = applied.title_id,
                error = %e,
                "Accepted cover promoted but its URL not recorded"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::proposal::ProposalTarget;
    use serde_json::json;

    const NAME: &str = "0123456789abcdef0123456789abcdef.jpg";

    fn row(id: u64, v: Value) -> ProposalItemRow {
        ProposalItemRow {
            id,
            proposal_id: 1,
            target: Some(ProposalTarget {
                table: ProposalTargetTable::Titles,
                id: 4,
            }),
            target_version: Some(1),
            proposed_values_json: v.to_string(),
            version: 1,
        }
    }

    #[test]
    fn parses_the_cover_item() {
        let p = parse_items(&[row(
            1,
            json!({"line":"cover","staged_file":NAME,"replaces_existing":true,"current":"/covers/4.jpg?v=1"}),
        )])
        .unwrap();
        assert_eq!(p.title_id, 4);
        assert_eq!(p.staged_file, NAME);
        assert_eq!(p.current.as_deref(), Some("/covers/4.jpg?v=1"));
        let none = parse_items(&[row(
            1,
            json!({"line":"cover","staged_file":NAME,"current":null}),
        )])
        .unwrap();
        assert_eq!(none.current, None);
    }

    #[test]
    fn refuses_bad_shapes() {
        let ok = json!({"line":"cover","staged_file":NAME,"current":null});
        let cases: Vec<Vec<ProposalItemRow>> = vec![
            vec![],
            vec![row(1, ok.clone()), row(2, ok.clone())],
            vec![row(1, json!({"line":"field"}))],
            vec![row(1, json!({"line":"cover","staged_file":"../x.jpg"}))],
            vec![row(1, json!({"line":"cover"}))],
            vec![row(
                1,
                json!({"line":"cover","staged_file":NAME,"current":3}),
            )],
            vec![ProposalItemRow {
                target: None,
                ..row(1, ok.clone())
            }],
        ];
        for (i, items) in cases.iter().enumerate() {
            assert!(parse_items(items).is_err(), "case {i}");
        }
    }

    #[test]
    fn cover_change_and_url() {
        assert!(!cover_changed(None, None));
        assert!(!cover_changed(None, Some("")));
        assert!(!cover_changed(
            Some("/covers/4.jpg?v=1"),
            Some("/covers/4.jpg?v=1")
        ));
        assert!(cover_changed(
            Some("/covers/4.jpg?v=1"),
            Some("/covers/4.jpg?v=2")
        ));
        assert!(cover_changed(None, Some("/covers/4.jpg")));
        assert!(cover_changed(Some("/covers/4.jpg"), None));
        assert_eq!(cache_busted("/covers/4.jpg", 17), "/covers/4.jpg?v=17");
    }
}
