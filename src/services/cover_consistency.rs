//! Story 19-9 (#658, D1–D13, T1–T7) — covers folder / database consistency.
//!
//! - **Shapes (Rule #20, AC1).** An own cover file is exactly what the app
//!   writes, `format!("{id}.jpg")` for an id ≥ 1 ([`own_cover_id`]: no
//!   leading zero, fits a `u64`), a regular file at the folder root. A
//!   local cover address is `/covers/<id>.jpg`, optionally followed by
//!   `?v=<digits>` ([`local_cover_id`]). Everything else — `.staging/`,
//!   temporary files, links, directories, other names, other addresses —
//!   is counted, never listed for a change.
//! - **The plan is pure** ([`plan`], T2) over a folder listing, one read of
//!   every title (Trash included, T3 — a deliberate exception to
//!   `deleted_at IS NULL`, like the backup dump) and the titles with a
//!   pending cover proposal. An orphan is an own file whose id is no
//!   title's (live or in the Trash) and that no title's address names; a
//!   missing cover is a title whose address is a local cover of its own id
//!   with no entry of that name in the folder. A file of an existing title
//!   that its address does not use is left alone and not reported (D3).
//! - **D12:** when more than half of the local covers are missing, the
//!   folder looks unmounted: the check offers no repair and the repair
//!   refuses.
//! - **The repair** ([`repair`], T4, T7) acts only on what the check listed
//!   (the items the modal posts), re-checked against a fresh plan and then
//!   item by item right before the change: an orphan is deleted only if no
//!   title row has its id or names it and its size and modification time
//!   are the listed ones; an address is cleared with a guarded `UPDATE`
//!   (listed value, no `version` / `updated_at` change, so no series
//!   proposal is outdated) only while the file is still missing. It holds
//!   the backup guard (`try_lock_owned`, D11) in a task spawned in the
//!   request's maintenance scope, writes under `maintenance::write_permit`,
//!   and ends with one `admin_audit` row `cover_repair` (D10).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::services::backup::BackupConfig;
use crate::services::backup::covers::unix_nanos;

/// D2: rows shown per list.
pub const LIST_LIMIT: usize = 50;
/// D10: the audit action of a repair.
pub const AUDIT_ACTION: &str = "cover_repair";

/// The id of an own cover file name: exactly `format!("{id}.jpg")`, id ≥ 1.
pub fn own_cover_id(name: &str) -> Option<u64> {
    canonical_id(name.strip_suffix(".jpg")?)
}

/// Canonical decimal digits of an id ≥ 1 (no sign, no leading zero, fits).
fn canonical_id(digits: &str) -> Option<u64> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || digits.starts_with('0') {
        return None;
    }
    let id: u64 = digits.parse().ok()?;
    (id >= 1 && id.to_string() == digits).then_some(id)
}

/// The id a local cover address names: `/covers/<id>.jpg`, optionally
/// followed by `?v=<digits>` (the cache-busted form).
pub fn local_cover_id(url: &str) -> Option<u64> {
    let rest = url.strip_prefix("/covers/")?;
    let (file, query) = match rest.split_once('?') {
        Some((file, query)) => (file, Some(query)),
        None => (rest, None),
    };
    if let Some(query) = query {
        let v = query.strip_prefix("v=")?;
        if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    own_cover_id(file)
}

/// Review pass 1 (L8): every file id an address names — each
/// `/covers/<id>.jpg` in it followed by the end, `?` or `#` (positive
/// shape). A file named this way is never an orphan, whatever the rest of
/// the address (an absolute URL, a fragment); only [`local_cover_id`]
/// decides what may be cleared.
pub fn named_cover_ids(url: &str) -> Vec<u64> {
    let mut ids = Vec::new();
    let mut rest = url;
    while let Some(at) = rest.find("/covers/") {
        let after = &rest[at + "/covers/".len()..];
        let digits_len = after.bytes().take_while(u8::is_ascii_digit).count();
        let (digits, tail) = after.split_at(digits_len);
        if let Some(end) = tail.strip_prefix(".jpg")
            && (end.is_empty() || end.starts_with('?') || end.starts_with('#'))
            && let Some(id) = canonical_id(digits)
        {
            ids.push(id);
        }
        rest = after;
    }
    ids
}

/// One own cover file of the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverFile {
    pub id: u64,
    pub size: u64,
    /// Nanoseconds since the Unix epoch (negative before it).
    pub modified_ns: i128,
}

impl CoverFile {
    pub fn name(&self) -> String {
        format!("{}.jpg", self.id)
    }
}

/// What the folder holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderListing {
    /// The own cover files (regular files with an own name).
    pub files: Vec<CoverFile>,
    /// Every other entry (counted, D2).
    pub others: u64,
    /// The ids of every entry with an own name, whatever its kind: a link
    /// or a directory named `9.jpg` is not "missing" (left alone).
    pub present_ids: HashSet<u64>,
}

/// One own regular file from its metadata. Review pass 1 (L6): a file
/// deleted since `read_dir` listed it is skipped (`None`), not a failed
/// check; any other error fails it.
fn cover_file(id: u64, meta: std::io::Result<std::fs::Metadata>) -> std::io::Result<Option<CoverFile>> {
    let meta = match meta {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(Some(CoverFile { id, size: meta.len(), modified_ns: unix_nanos(meta.modified()?) }))
}

/// List `dir` (blocking). A missing folder is an empty listing (D12 then
/// says it looks unmounted when the library uses covers).
pub fn list_folder(dir: &Path) -> std::io::Result<FolderListing> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FolderListing::default()),
        Err(e) => return Err(e),
    };
    let mut listing = FolderListing::default();
    for entry in entries {
        let entry = entry?;
        let id = entry.file_name().to_str().and_then(own_cover_id);
        if let Some(id) = id {
            listing.present_ids.insert(id);
        }
        // `DirEntry::file_type` / `metadata` do not follow symbolic links.
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        match id {
            Some(id) if is_file => match cover_file(id, entry.metadata())? {
                Some(file) => listing.files.push(file),
                None => {
                    listing.present_ids.remove(&id);
                }
            },
            _ => listing.others += 1,
        }
    }
    listing.files.sort_by_key(|f| f.id);
    Ok(listing)
}

/// One title as the plan needs it (live or in the Trash).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleCover {
    pub id: u64,
    pub title: String,
    pub cover_image_url: Option<String>,
    pub in_trash: bool,
    /// `manually_edited_fields` holds `cover_image_url` (D5).
    pub hand_set: bool,
    /// An ISBN, ISSN or UPC to look the cover up with (D6).
    pub has_code: bool,
}

/// What happens to a title whose cover file is missing (D4–D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingAction {
    /// Live, with a code, not set by hand: cleared and queued (D6).
    ClearAndQueue,
    /// In the Trash: cleared, nothing queued (D4).
    ClearInTrash,
    /// Set by hand: cleared, the mark kept, nothing queued (D5).
    ClearHandSet,
    /// No ISBN, ISSN or UPC: cleared, nothing to download (D6).
    ClearNoCode,
    /// A cover proposal is pending: left alone (D7).
    LeavePending,
}

impl MissingAction {
    pub fn clears(self) -> bool {
        !matches!(self, MissingAction::LeavePending)
    }

    pub fn queues(self) -> bool {
        matches!(self, MissingAction::ClearAndQueue)
    }
}

/// A title whose cover file is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingCover {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub action: MissingAction,
}

/// What a check found (pure result of [`plan`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverPlan {
    /// Newest first.
    pub orphans: Vec<CoverFile>,
    /// By title, then id.
    pub missing: Vec<MissingCover>,
    /// Own cover files checked.
    pub files_checked: u64,
    /// Other entries of the folder (D2).
    pub others: u64,
    /// Titles whose cover address has another form (D2).
    pub remote: u64,
    /// Titles whose address is a local cover of their own id (D12).
    pub local_total: u64,
    /// Of those, the ones whose file is missing (D12).
    pub local_missing: u64,
}

impl CoverPlan {
    /// D12: more than half of the local covers are missing.
    pub fn looks_unmounted(&self) -> bool {
        self.local_total > 0 && self.local_missing.saturating_mul(2) > self.local_total
    }

    /// Nothing listed at all.
    pub fn is_match(&self) -> bool {
        self.orphans.is_empty() && self.missing.is_empty()
    }

    /// Something a repair would change.
    pub fn has_repair(&self) -> bool {
        !self.orphans.is_empty() || self.missing.iter().any(|m| m.action.clears())
    }
}

fn action_for(t: &TitleCover, pending: &HashSet<u64>) -> MissingAction {
    if pending.contains(&t.id) {
        MissingAction::LeavePending
    } else if t.in_trash {
        MissingAction::ClearInTrash
    } else if t.hand_set {
        MissingAction::ClearHandSet
    } else if !t.has_code {
        MissingAction::ClearNoCode
    } else {
        MissingAction::ClearAndQueue
    }
}

/// T2 — the plan (pure).
pub fn plan(listing: &FolderListing, titles: &[TitleCover], pending_cover_titles: &HashSet<u64>) -> CoverPlan {
    let title_ids: HashSet<u64> = titles.iter().map(|t| t.id).collect();
    // A file named by ANY title's local address is never an orphan.
    let named_ids: HashSet<u64> = titles
        .iter()
        .filter_map(|t| t.cover_image_url.as_deref())
        .flat_map(named_cover_ids)
        .collect();
    let mut out = CoverPlan {
        files_checked: listing.files.len() as u64,
        others: listing.others,
        ..CoverPlan::default()
    };
    out.orphans = listing
        .files
        .iter()
        .filter(|f| !title_ids.contains(&f.id) && !named_ids.contains(&f.id))
        .cloned()
        .collect();
    out.orphans.sort_by(|a, b| b.modified_ns.cmp(&a.modified_ns).then(a.id.cmp(&b.id)));
    for t in titles {
        let Some(url) = t.cover_image_url.as_deref().filter(|u| !u.is_empty()) else {
            continue;
        };
        if local_cover_id(url) != Some(t.id) {
            out.remote += 1;
            continue;
        }
        out.local_total += 1;
        if listing.present_ids.contains(&t.id) {
            continue;
        }
        out.local_missing += 1;
        out.missing.push(MissingCover {
            id: t.id,
            title: t.title.clone(),
            url: url.to_string(),
            action: action_for(t, pending_cover_titles),
        });
    }
    out.missing.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    out
}

fn is_hand_set(manually_edited_fields: Option<&str>) -> bool {
    manually_edited_fields
        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
        .is_some_and(|fields| fields.iter().any(|f| f == "cover_image_url"))
}

/// T3 — every title, Trash included (deliberate exception to
/// `deleted_at IS NULL`).
pub async fn read_titles(pool: &DbPool) -> Result<Vec<TitleCover>, AppError> {
    let rows = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, title, cover_image_url, \
                CAST(deleted_at IS NOT NULL AS SIGNED) AS in_trash, \
                CAST(manually_edited_fields AS CHAR) AS manually_edited_fields, \
                CAST((isbn IS NOT NULL OR issn IS NOT NULL OR upc IS NOT NULL) AS SIGNED) AS has_code \
         FROM titles",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|r| {
            let id: i64 = r.try_get("id")?;
            let mef: Option<String> = r.try_get("manually_edited_fields")?;
            Ok(TitleCover {
                id: u64::try_from(id).unwrap_or(0),
                title: r.try_get("title")?,
                cover_image_url: r.try_get("cover_image_url")?,
                in_trash: r.try_get::<i64, _>("in_trash")? != 0,
                hand_set: is_hand_set(mef.as_deref()),
                has_code: r.try_get::<i64, _>("has_code")? != 0,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(AppError::from)
}

/// The titles with a live pending cover proposal (a plain read, no lock).
pub async fn pending_cover_titles(pool: &DbPool) -> Result<HashSet<u64>, AppError> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT CAST(i.target_id AS SIGNED) FROM proposals p \
         JOIN proposal_items i ON i.proposal_id = p.id AND i.deleted_at IS NULL \
         WHERE p.deleted_at IS NULL AND p.status = 'pending' AND p.kind = 'cover' \
           AND i.target_table = 'titles'",
    )
    .fetch_all(pool)
    .await?;
    Ok(ids.into_iter().filter_map(|id| u64::try_from(id).ok()).collect())
}

/// Why a check or a repair did not run.
#[derive(Debug)]
pub enum CoverCheckError {
    /// The folder cannot be listed (D2).
    Unreadable,
    Failed(AppError),
}

impl From<AppError> for CoverCheckError {
    fn from(e: AppError) -> Self {
        CoverCheckError::Failed(e)
    }
}

/// The check: list the folder, read the library, plan. Changes nothing.
pub async fn check(pool: &DbPool, covers_dir: &Path) -> Result<CoverPlan, CoverCheckError> {
    let dir = covers_dir.to_path_buf();
    let listing = tokio::task::spawn_blocking(move || list_folder(&dir))
        .await
        .map_err(|e| AppError::Internal(format!("cover listing task: {e}")))?;
    let listing = match listing {
        Ok(listing) => listing,
        Err(e) => {
            tracing::warn!(error = %e, dir = %covers_dir.display(), "Cover check: the covers folder cannot be read");
            return Err(CoverCheckError::Unreadable);
        }
    };
    let titles = read_titles(pool).await?;
    let pending = pending_cover_titles(pool).await?;
    Ok(plan(&listing, &titles, &pending))
}

/// An orphan as the modal posts it (T5): `<id>:<size>:<modified ns>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrphanItem {
    pub id: u64,
    pub size: u64,
    pub modified_ns: i128,
}

impl OrphanItem {
    pub fn of(f: &CoverFile) -> Self {
        OrphanItem { id: f.id, size: f.size, modified_ns: f.modified_ns }
    }

    pub fn encode(&self) -> String {
        format!("{}:{}:{}", self.id, self.size, self.modified_ns)
    }

    /// Positive shape only: anything else is `None` (dropped).
    pub fn parse(raw: &str) -> Option<Self> {
        let mut parts = raw.split(':');
        let id = canonical_id(parts.next()?)?;
        let size = parts.next()?;
        let ns = parts.next()?;
        if parts.next().is_some() || size.is_empty() || !size.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let digits = ns.strip_prefix('-').unwrap_or(ns);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some(OrphanItem { id, size: size.parse().ok()?, modified_ns: ns.parse().ok()? })
    }
}

/// A missing cover as the modal posts it (T5): `<id>:<listed address>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MissingItem {
    pub id: u64,
    pub url: String,
}

impl MissingItem {
    pub fn encode(&self) -> String {
        format!("{}:{}", self.id, self.url)
    }

    /// Positive shape only: the address must be a local cover of the id.
    pub fn parse(raw: &str) -> Option<Self> {
        let (id, url) = raw.split_once(':')?;
        let id = canonical_id(id)?;
        (local_cover_id(url) == Some(id)).then(|| MissingItem { id, url: url.to_string() })
    }
}

/// What the Admin confirms: the listed items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepairItems {
    pub orphans: Vec<OrphanItem>,
    pub missing: Vec<MissingItem>,
}

impl RepairItems {
    /// Everything a plan would change (what the check lists).
    pub fn of_plan(plan: &CoverPlan) -> Self {
        RepairItems {
            orphans: plan.orphans.iter().map(OrphanItem::of).collect(),
            missing: plan
                .missing
                .iter()
                .filter(|m| m.action.clears())
                .map(|m| MissingItem { id: m.id, url: m.url.clone() })
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.orphans.len() + self.missing.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The listed items that a fresh plan still holds unchanged, with the
/// action of each missing cover (pure).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Confirmed {
    pub orphans: Vec<OrphanItem>,
    pub missing: Vec<(MissingItem, MissingAction)>,
}

impl Confirmed {
    pub fn queued(&self) -> usize {
        self.missing.iter().filter(|(_, a)| a.queues()).count()
    }

    pub fn items(&self) -> RepairItems {
        RepairItems {
            orphans: self.orphans.clone(),
            missing: self.missing.iter().map(|(m, _)| m.clone()).collect(),
        }
    }
}

/// Intersect the listed items with a fresh plan (pure, deduplicated).
pub fn confirm(plan: &CoverPlan, listed: &RepairItems) -> Confirmed {
    let fresh_orphans: HashSet<OrphanItem> = plan.orphans.iter().map(OrphanItem::of).collect();
    let mut seen_o = HashSet::new();
    let orphans = listed
        .orphans
        .iter()
        .filter(|o| fresh_orphans.contains(o) && seen_o.insert(o.id))
        .cloned()
        .collect();
    let mut seen_m = HashSet::new();
    let missing = listed
        .missing
        .iter()
        .filter_map(|m| {
            let fresh = plan.missing.iter().find(|f| f.id == m.id && f.url == m.url)?;
            (fresh.action.clears() && seen_m.insert(m.id)).then(|| (m.clone(), fresh.action))
        })
        .collect();
    Confirmed { orphans, missing }
}

/// Who repairs (the audit row's author, #70).
#[derive(Debug, Clone)]
pub struct Actor {
    pub user_id: u64,
    pub username: String,
    pub role: String,
}

/// What a repair did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RepairDone {
    pub deleted: u64,
    pub cleared: u64,
    pub queued: u64,
    /// Listed items that had changed since the check (left alone).
    pub skipped: u64,
}

/// Why a repair did not run.
#[derive(Debug)]
pub enum RepairError {
    /// The backup guard is held (D11): a backup or a restore runs.
    Busy,
    /// D12, applied again on a fresh plan.
    Unmounted(CoverPlan),
    Check(CoverCheckError),
}

/// Whether a file at `path` is still the listed orphan (blocking).
fn still_listed_file(path: &Path, item: &OrphanItem) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            meta.file_type().is_file()
                && meta.len() == item.size
                && meta.modified().map(unix_nanos).ok() == Some(item.modified_ns)
        }
        Err(_) => false,
    }
}

/// No entry at all named `path` (blocking): the cover is still missing.
fn absent(path: &Path) -> bool {
    matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, AppError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Internal(format!("cover repair file task: {e}")))
}

/// Review pass 1 (L4): orphans are re-checked by chunks of this many ids.
pub const OWNED_CHUNK: usize = 500;

/// T4 / review pass 1 (L4, L8): of `ids`, those a title row holds or an
/// address names — one `IN (…)` read and one read of the addresses holding
/// `/covers/`, matched in Rust with [`named_cover_ids`].
pub async fn owned_ids(pool: &DbPool, ids: &[u64]) -> Result<HashSet<u64>, AppError> {
    let mut owned = HashSet::new();
    if ids.is_empty() {
        return Ok(owned);
    }
    let placeholders = vec!["?"; ids.len()].join(", ");
    let sql = format!("SELECT CAST(id AS SIGNED) FROM titles WHERE id IN ({placeholders})");
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    for id in ids {
        q = q.bind(*id);
    }
    owned.extend(q.fetch_all(pool).await?.into_iter().filter_map(|id| u64::try_from(id).ok()));
    let wanted: HashSet<u64> = ids.iter().copied().collect();
    let urls: Vec<String> = sqlx::query_scalar(
        "SELECT cover_image_url FROM titles WHERE cover_image_url LIKE '%/covers/%'",
    )
    .fetch_all(pool)
    .await?;
    owned.extend(urls.iter().flat_map(|u| named_cover_ids(u)).filter(|id| wanted.contains(id)));
    Ok(owned)
}

/// Review pass 1 (L7): a live pending cover proposal on the title (a plain
/// read from the target index).
pub async fn has_pending_cover_proposal(pool: &DbPool, title_id: u64) -> Result<bool, AppError> {
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM proposal_items i FORCE INDEX (idx_proposal_items_target) \
         STRAIGHT_JOIN proposals p ON p.id = i.proposal_id \
         WHERE i.target_table = 'titles' AND i.target_id = ? AND i.deleted_at IS NULL \
           AND p.deleted_at IS NULL AND p.status = 'pending' AND p.kind = 'cover' \
         LIMIT 1",
    )
    .bind(title_id)
    .fetch_optional(pool)
    .await?;
    Ok(found.is_some())
}

/// Clear the listed address (guarded, no `version` / `updated_at` change)
/// and queue a lookup when `queue`; `false` when the row changed.
pub async fn clear_address(pool: &DbPool, item: &MissingItem, queue: bool) -> Result<bool, AppError> {
    let mut tx = pool.begin().await?;
    let done = sqlx::query(
        "UPDATE titles SET cover_image_url = NULL, updated_at = updated_at \
         WHERE id = ? AND cover_image_url = ?",
    )
    .bind(item.id)
    .bind(&item.url)
    .execute(&mut *tx)
    .await?;
    if done.rows_affected() != 1 {
        return Ok(false);
    }
    if queue {
        crate::models::metadata_retry::MetadataRetryModel::enqueue_fresh(&mut *tx, item.id).await?;
    }
    tx.commit().await?;
    Ok(true)
}

/// A file appeared right after the clear (a cover being installed): put
/// the address back unless something else wrote one meanwhile, and take
/// the lookup the clear queued off the queue (review pass 1, L3).
pub async fn restore_address(pool: &DbPool, item: &MissingItem, queued: bool) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE titles SET cover_image_url = ?, updated_at = updated_at \
         WHERE id = ? AND cover_image_url IS NULL",
    )
    .bind(&item.url)
    .bind(item.id)
    .execute(pool)
    .await?;
    if queued {
        crate::models::metadata_retry::MetadataRetryModel::dequeue(pool, item.id).await?;
    }
    Ok(())
}

/// The changes, item by item (T4). Review pass 1 (L2): an error stops the
/// loop and is returned with the counts reached so far.
async fn apply(pool: &DbPool, covers_dir: &Path, confirmed: &Confirmed, done: &mut RepairDone) -> Result<(), AppError> {
    for chunk in confirmed.orphans.chunks(OWNED_CHUNK) {
        let ids: Vec<u64> = chunk.iter().map(|o| o.id).collect();
        let owned = owned_ids(pool, &ids).await?;
        for item in chunk {
            let Some(_permit) = crate::services::maintenance::write_permit() else {
                tracing::info!("Cover repair stopped: a restore is starting");
                return Ok(());
            };
            if owned.contains(&item.id) {
                continue;
            }
            let path: PathBuf = covers_dir.join(format!("{}.jpg", item.id));
            let it = item.clone();
            let removed = blocking(move || {
                if !still_listed_file(&path, &it) {
                    return Ok(false);
                }
                std::fs::remove_file(&path).map(|()| true)
            })
            .await?;
            match removed {
                Ok(true) => {
                    done.deleted += 1;
                    tracing::info!(file = %format!("{}.jpg", item.id), "Cover repair: orphan cover file deleted");
                }
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %e, id = item.id, "Cover repair: an orphan cover file could not be deleted"),
            }
        }
    }
    for (item, action) in &confirmed.missing {
        let Some(_permit) = crate::services::maintenance::write_permit() else {
            tracing::info!("Cover repair stopped: a restore is starting");
            return Ok(());
        };
        let path = covers_dir.join(format!("{}.jpg", item.id));
        let p = path.clone();
        if !blocking(move || absent(&p)).await? {
            continue;
        }
        if has_pending_cover_proposal(pool, item.id).await? {
            continue;
        }
        let queue = action.queues();
        if !clear_address(pool, item, queue).await? {
            continue;
        }
        let p = path.clone();
        if !blocking(move || absent(&p)).await? {
            // A cover was installed meanwhile (file first, address next).
            if let Err(e) = restore_address(pool, item, queue).await {
                tracing::error!(error = %e, id = item.id, url = %item.url, "Cover repair: a cleared address could not be put back");
                return Err(e);
            }
            continue;
        }
        done.cleared += 1;
        if queue {
            done.queued += 1;
        }
    }
    Ok(())
}

/// D10 — the audit row (best effort: the changes are done). Review pass 1
/// (L2): a repair that ended on an error records it with its counts.
async fn audit(pool: &DbPool, actor: &Actor, done: &RepairDone, error: Option<&str>) {
    let mut details = serde_json::json!({
        "deleted": done.deleted,
        "cleared": done.cleared,
        "queued": done.queued,
        "skipped": done.skipped,
        "user_username": actor.username,
        "user_role": actor.role,
    });
    if let Some(error) = error {
        details["error"] = serde_json::Value::String(error.to_string());
    }
    if let Err(e) = crate::models::admin_audit::AdminAuditModel::create(
        pool,
        actor.user_id,
        AUDIT_ACTION,
        None,
        None,
        Some(details),
    )
    .await
    {
        tracing::warn!(error = %e, "Cover repair: the audit row could not be written");
    }
}

/// The repair's core, the backup guard held (T4): a fresh plan, D12 again,
/// the changes, the audit row.
async fn repair_holding(
    pool: &DbPool,
    covers_dir: &Path,
    listed: &RepairItems,
    actor: &Actor,
) -> Result<RepairDone, RepairError> {
    let fresh = check(pool, covers_dir).await.map_err(RepairError::Check)?;
    if fresh.looks_unmounted() {
        return Err(RepairError::Unmounted(fresh));
    }
    let confirmed = confirm(&fresh, listed);
    let mut done = RepairDone::default();
    let outcome = apply(pool, covers_dir, &confirmed, &mut done).await;
    done.skipped = (listed.len() as u64).saturating_sub(done.deleted + done.cleared);
    let error = outcome.as_ref().err().map(|e| e.to_string());
    audit(pool, actor, &done, error.as_deref()).await;
    tracing::info!(
        admin = %actor.username,
        deleted = done.deleted,
        cleared = done.cleared,
        queued = done.queued,
        skipped = done.skipped,
        error = error.as_deref().unwrap_or(""),
        "Cover repair done"
    );
    outcome.map_err(|e| RepairError::Check(CoverCheckError::Failed(e)))?;
    Ok(done)
}

/// The repair (T4, T7): refused while the backup guard is held (D11);
/// otherwise a task spawned in the request's maintenance scope owns the
/// guard, so a closed tab never leaves a half repair without its audit row.
pub async fn repair(
    pool: &DbPool,
    backup: &BackupConfig,
    listed: RepairItems,
    actor: Actor,
) -> Result<RepairDone, RepairError> {
    let guard = backup.guard.clone().try_lock_owned().map_err(|_| RepairError::Busy)?;
    let pool = pool.clone();
    let covers_dir = backup.covers_dir.clone();
    let scope = crate::services::maintenance::current_scope();
    let task = tokio::spawn(crate::services::maintenance::rescoped(scope, async move {
        let _held = guard;
        repair_holding(&pool, &covers_dir, &listed, &actor).await
    }));
    task.await.map_err(|e| {
        RepairError::Check(CoverCheckError::Failed(AppError::Internal(format!("cover repair task: {e}"))))
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_own_cover_name_is_exactly_what_the_app_writes() {
        assert_eq!(own_cover_id("1.jpg"), Some(1));
        assert_eq!(own_cover_id("4217.jpg"), Some(4217));
        assert_eq!(own_cover_id(&format!("{}.jpg", u64::MAX)), Some(u64::MAX));
        for bad in [
            "0.jpg",
            "007.jpg",
            "004217.jpg",
            "1234567890123456789012345.jpg",
            ".jpg",
            "1.JPG",
            "1.jpeg",
            "1.jpg.tmp",
            "-1.jpg",
            "+1.jpg",
            " 1.jpg",
            "1 .jpg",
            "a1.jpg",
            ".tmp-0123456789abcdef0123456789abcdef",
            ".staging",
            "notes.txt",
            "12.jpg/",
        ] {
            assert_eq!(own_cover_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_local_cover_address_names_its_file_with_an_optional_version() {
        assert_eq!(local_cover_id("/covers/12.jpg"), Some(12));
        assert_eq!(local_cover_id("/covers/12.jpg?v=1764140000"), Some(12));
        for bad in [
            "/covers/012.jpg",
            "/covers/12.jpg?",
            "/covers/12.jpg?v=",
            "/covers/12.jpg?v=12a",
            "/covers/12.jpg?x=1",
            "/covers/12.jpg?v=1&x=2",
            "/covers/.staging/ab.jpg",
            "/covers/a/12.jpg",
            "covers/12.jpg",
            "https://example.org/covers/12.jpg",
            "/covers-wishlist/12.jpg",
            "",
        ] {
            assert_eq!(local_cover_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_file_gone_since_the_listing_is_skipped_not_a_failure() {
        use std::io::{Error, ErrorKind};
        assert!(cover_file(3, Err(Error::from(ErrorKind::NotFound))).unwrap().is_none());
        assert!(cover_file(3, Err(Error::from(ErrorKind::PermissionDenied))).is_err());
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("3.jpg"), b"abcd").unwrap();
        let f = cover_file(3, std::fs::metadata(tmp.path().join("3.jpg"))).unwrap().unwrap();
        assert_eq!((f.id, f.size), (3, 4));
    }

    #[test]
    fn an_address_names_every_own_file_it_holds() {
        assert_eq!(named_cover_ids("/covers/12.jpg"), vec![12]);
        assert_eq!(named_cover_ids("/covers/12.jpg?v=3"), vec![12]);
        assert_eq!(named_cover_ids("https://nas.local/covers/12.jpg#x"), vec![12]);
        assert_eq!(named_cover_ids("/x/covers/7.jpg?/covers/8.jpg"), vec![7, 8]);
        for none in ["/covers/012.jpg", "/covers/12.jpgx", "/covers/12.jpeg", "/covers/.jpg", "/covers/0.jpg", "/covers/12", ""] {
            assert!(named_cover_ids(none).is_empty(), "{none}");
        }
    }

    #[test]
    fn a_file_named_by_an_absolute_address_is_never_an_orphan() {
        let p = plan(
            &listing(vec![file(40, 1)], 0, &[]),
            &[title(1, Some("https://nas.local/covers/40.jpg?v=1"))],
            &HashSet::new(),
        );
        assert!(p.orphans.is_empty());
        assert_eq!(p.remote, 1, "still another form: never cleared");
    }

    fn file(id: u64, ns: i128) -> CoverFile {
        CoverFile { id, size: 100, modified_ns: ns }
    }

    fn title(id: u64, url: Option<&str>) -> TitleCover {
        TitleCover {
            id,
            title: format!("Title {id}"),
            cover_image_url: url.map(str::to_string),
            in_trash: false,
            hand_set: false,
            has_code: true,
        }
    }

    fn listing(files: Vec<CoverFile>, others: u64, extra_present: &[u64]) -> FolderListing {
        let mut present_ids: HashSet<u64> = files.iter().map(|f| f.id).collect();
        present_ids.extend(extra_present);
        FolderListing { files, others, present_ids }
    }

    #[test]
    fn the_plan_lists_orphans_and_missing_covers_with_their_actions() {
        let files = vec![file(1, 10), file(50, 30), file(60, 20), file(7, 5)];
        let mut trash = title(2, Some("/covers/2.jpg"));
        trash.in_trash = true;
        let mut hand = title(3, Some("/covers/3.jpg?v=17"));
        hand.hand_set = true;
        let mut no_code = title(4, Some("/covers/4.jpg"));
        no_code.has_code = false;
        let mut pending = title(5, Some("/covers/5.jpg"));
        pending.title = "a pending one".into();
        let titles = vec![
            title(1, Some("/covers/1.jpg")), // present
            trash,
            hand,
            no_code,
            pending,
            title(6, Some("/covers/6.jpg?v=9")), // live, queued
            title(7, None),                       // D3: its file is kept
            title(8, Some("https://example.org/x.jpg")),
            title(9, Some("/covers/60.jpg")), // names another title's file
            title(10, Some("")),
        ];
        let p = plan(&listing(files, 3, &[]), &titles, &HashSet::from([5]));
        let orphan_ids: Vec<u64> = p.orphans.iter().map(|f| f.id).collect();
        assert_eq!(orphan_ids, vec![50], "60 is named by title 9, 7 is title 7's, 1 is used");
        let missing: Vec<(u64, MissingAction)> = p.missing.iter().map(|m| (m.id, m.action)).collect();
        assert_eq!(
            missing,
            vec![
                (5, MissingAction::LeavePending),
                (2, MissingAction::ClearInTrash),
                (3, MissingAction::ClearHandSet),
                (4, MissingAction::ClearNoCode),
                (6, MissingAction::ClearAndQueue),
            ],
            "by title, case ignored"
        );
        assert_eq!(p.files_checked, 4);
        assert_eq!(p.others, 3);
        assert_eq!(p.remote, 2, "a remote address and another title's file");
        assert_eq!(p.local_total, 6);
        assert_eq!(p.local_missing, 5);
        assert!(p.looks_unmounted());
        assert!(p.has_repair());
        assert!(!p.is_match());
    }

    #[test]
    fn orphans_are_newest_first_and_a_trash_title_keeps_its_file() {
        let files = vec![file(20, 1), file(21, 3), file(22, 2), file(23, 9)];
        let mut trash = title(23, None);
        trash.in_trash = true;
        let p = plan(&listing(files, 0, &[]), &[trash], &HashSet::new());
        let ids: Vec<u64> = p.orphans.iter().map(|f| f.id).collect();
        assert_eq!(ids, vec![21, 22, 20]);
        assert!(!p.looks_unmounted(), "no local cover in use");
    }

    #[test]
    fn a_link_or_directory_with_an_own_name_is_not_a_missing_file() {
        let p = plan(&listing(vec![], 1, &[9]), &[title(9, Some("/covers/9.jpg"))], &HashSet::new());
        assert!(p.missing.is_empty());
        assert!(p.is_match());
    }

    #[test]
    fn nothing_to_repair_when_folder_and_library_match() {
        let p = plan(
            &listing(vec![file(1, 1), file(2, 1)], 0, &[]),
            &[title(1, Some("/covers/1.jpg")), title(2, Some("/covers/2.jpg?v=3"))],
            &HashSet::new(),
        );
        assert!(p.is_match());
        assert!(!p.has_repair());
        assert_eq!(p.files_checked, 2);
        // Only a pending one missing: listed, but nothing to repair.
        let p = plan(&listing(vec![], 0, &[]), &[title(1, Some("/covers/1.jpg")), title(2, Some("/covers/2.jpg")), title(3, Some("/covers/3.jpg"))], &HashSet::from([1]));
        assert_eq!(p.missing.len(), 3);
        let only_pending = plan(
            &listing(vec![file(2, 1), file(3, 1)], 0, &[]),
            &[title(1, Some("/covers/1.jpg")), title(2, Some("/covers/2.jpg")), title(3, Some("/covers/3.jpg"))],
            &HashSet::from([1]),
        );
        assert!(!only_pending.is_match());
        assert!(!only_pending.has_repair());
    }

    #[test]
    fn d12_more_than_half_missing_looks_unmounted() {
        let titles: Vec<TitleCover> = (1..=4).map(|i| title(i, Some(&format!("/covers/{i}.jpg")))).collect();
        let two = plan(&listing(vec![file(1, 1), file(2, 1)], 0, &[]), &titles, &HashSet::new());
        assert_eq!((two.local_missing, two.local_total), (2, 4));
        assert!(!two.looks_unmounted(), "exactly half is not more than half");
        let one = plan(&listing(vec![file(1, 1)], 0, &[]), &titles, &HashSet::new());
        assert!(one.looks_unmounted());
    }

    #[test]
    fn listed_items_round_trip_and_refuse_other_shapes() {
        let o = OrphanItem { id: 12, size: 3456, modified_ns: -17 };
        assert_eq!(OrphanItem::parse(&o.encode()), Some(o));
        for bad in ["", "0:1:1", "012:1:1", "1:1", "1:1:1:1", "1:-1:1", "1:1:", "1:1:--1", "a:1:1", "1:1:1.5"] {
            assert_eq!(OrphanItem::parse(bad), None, "{bad}");
        }
        let m = MissingItem { id: 5, url: "/covers/5.jpg?v=12".into() };
        assert_eq!(MissingItem::parse(&m.encode()), Some(m));
        for bad in ["5:/covers/6.jpg", "5:https://x/5.jpg", "05:/covers/5.jpg", "5", ":/covers/5.jpg"] {
            assert_eq!(MissingItem::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_listed_items_still_in_the_fresh_plan_are_confirmed() {
        let fresh = plan(
            &listing(vec![file(50, 7), file(51, 8)], 0, &[]),
            &[
                title(1, Some("/covers/1.jpg")),
                title(2, Some("/covers/2.jpg?v=2")),
                title(3, Some("/covers/3.jpg")),
            ],
            &HashSet::from([3]),
        );
        let listed = RepairItems {
            orphans: vec![
                OrphanItem { id: 50, size: 100, modified_ns: 7 },
                OrphanItem { id: 50, size: 100, modified_ns: 7 },
                OrphanItem { id: 51, size: 100, modified_ns: 1 }, // changed
                OrphanItem { id: 52, size: 100, modified_ns: 1 }, // gone
            ],
            missing: vec![
                MissingItem { id: 1, url: "/covers/1.jpg".into() },
                MissingItem { id: 2, url: "/covers/2.jpg?v=1".into() }, // rewritten
                MissingItem { id: 3, url: "/covers/3.jpg".into() },     // pending now
            ],
        };
        let c = confirm(&fresh, &listed);
        assert_eq!(c.orphans, vec![OrphanItem { id: 50, size: 100, modified_ns: 7 }]);
        assert_eq!(c.missing, vec![(MissingItem { id: 1, url: "/covers/1.jpg".into() }, MissingAction::ClearAndQueue)]);
        assert_eq!(c.queued(), 1);
        assert_eq!(RepairItems::of_plan(&fresh).missing.len(), 2, "the pending one is never listed for a change");
    }

    #[test]
    fn the_hand_set_mark_is_read_from_the_json_array_only() {
        assert!(is_hand_set(Some(r#"["title","cover_image_url"]"#)));
        assert!(!is_hand_set(Some(r#"["title"]"#)));
        assert!(!is_hand_set(Some("cover_image_url")));
        assert!(!is_hand_set(None));
    }

    #[test]
    fn the_folder_listing_keeps_own_regular_files_only() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::write(d.join("3.jpg"), b"abc").unwrap();
        std::fs::write(d.join("007.jpg"), b"x").unwrap();
        std::fs::write(d.join("notes.txt"), b"x").unwrap();
        std::fs::write(d.join(".tmp-0123"), b"x").unwrap();
        std::fs::create_dir(d.join(".staging")).unwrap();
        std::fs::write(d.join(".staging").join("4.jpg"), b"x").unwrap();
        std::fs::create_dir(d.join("5.jpg")).unwrap();
        std::os::unix::fs::symlink(d.join("3.jpg"), d.join("9.jpg")).unwrap();
        let l = list_folder(d).unwrap();
        assert_eq!(l.files.len(), 1);
        assert_eq!((l.files[0].id, l.files[0].size), (3, 3));
        assert_eq!(l.others, 6);
        assert_eq!(l.present_ids, HashSet::from([3, 5, 9]));
        assert_eq!(list_folder(&d.join("missing")).unwrap(), FolderListing::default());
    }
}
