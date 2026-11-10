//! Story 17-3 (#565, FR174) — run-once boot repair of stored ISBNs.
//!
//! Before v1.27.0 the manual title form stored an ISBN as typed (with its
//! separators, an `ISBN` prefix or as an ISBN-10), so a later scan of the
//! bare ISBN-13 missed the title and created a duplicate. Every form now
//! goes through the one rule (`utils::parse_isbn`); this repair applies it
//! once per database to the titles already stored — **titles in the Trash
//! included** (coordinator decision, 2026-11-10), so a later restore never
//! brings back the old form
//! ([`repair_stored_isbns_once`], from `main.rs`, under a `data_repairs`
//! marker):
//!
//! - positive shapes only (Rule #20): a stored value is rewritten only when
//!   `parse_isbn` accepts it and its ISBN-13 differs; anything else
//!   (another EAN, a wrong check digit, text) is left as is;
//! - two or more titles (live or trashed) that would carry the same ISBN-13
//!   after the repair are all left untouched (no merge, see #591);
//! - a **non-edit write** guarded by `version` and the old value, `version`
//!   not bumped (#521, story 13-4); the pending metadata / cover items'
//!   `identity.isbn` follows, so the repair outdates no proposal.
//!
//! The first boot is auditable from the logs (Rule #20 mitigation, the
//! production export being unavailable): one `info` line per rewritten
//! title (id, old value, new value), one `warn` per title left untouched
//! by a collision, one `info` summary.
//!
//! [`plan_isbn_repair`] is the pure classification, also run over a
//! production export by the ignored test `production_corpus`.

use std::collections::BTreeMap;

use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::isbn_repair::{list_isbns, set_canonical_isbn};
use crate::models::proposal::ProposalModel;
use crate::services::data_repairs;

/// Name of the story 17-3 repair in `data_repairs`.
pub const STORED_ISBN_REPAIR: &str = "17-3-stored-isbn-one-rule";

/// One stored value the repair rewrites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsbnRewrite {
    pub title_id: u64,
    pub old: String,
    pub new: String,
}

/// Titles that would carry the same ISBN-13 after the repair: all left
/// untouched. `titles` holds `(id, stored value)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsbnCollision {
    pub isbn13: String,
    pub titles: Vec<(u64, String)>,
}

/// What the repair would do with a set of stored ISBNs (pure).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IsbnRepairPlan {
    pub rewrites: Vec<IsbnRewrite>,
    pub collisions: Vec<IsbnCollision>,
    /// Already the canonical ISBN-13.
    pub canonical: usize,
    /// Not an ISBN under the one rule: left as stored.
    pub not_isbn: Vec<(u64, String)>,
}

/// Classify the stored ISBNs `(title id, value)` of the titles, live and
/// trashed alike.
pub fn plan_isbn_repair<'a>(rows: impl IntoIterator<Item = (u64, &'a str)>) -> IsbnRepairPlan {
    let mut plan = IsbnRepairPlan::default();
    let mut groups: BTreeMap<String, Vec<(u64, &str)>> = BTreeMap::new();
    for (id, stored) in rows {
        match crate::utils::parse_isbn(stored) {
            Ok(isbn13) => groups.entry(isbn13).or_default().push((id, stored)),
            Err(_) => plan.not_isbn.push((id, stored.to_string())),
        }
    }
    for (isbn13, members) in groups {
        let rewrites = members.iter().filter(|(_, old)| *old != isbn13).count();
        if rewrites == 0 {
            plan.canonical += members.len();
            continue;
        }
        if members.len() > 1 {
            plan.collisions.push(IsbnCollision {
                isbn13,
                titles: members.iter().map(|(id, old)| (*id, old.to_string())).collect(),
            });
            continue;
        }
        let (id, old) = members[0];
        plan.rewrites.push(IsbnRewrite { title_id: id, old: old.to_string(), new: isbn13 });
    }
    plan.rewrites.sort_by_key(|r| r.title_id);
    plan
}

/// What one run of the repair did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IsbnRepairReport {
    pub rewritten: usize,
    /// Of which titles in the Trash.
    pub rewritten_trashed: usize,
    /// Titles left untouched because they would collide.
    pub collided: usize,
    pub canonical: usize,
    pub not_isbn: usize,
    /// The row changed between the read and the update (guarded write).
    pub superseded: usize,
    /// Pending proposal items whose `identity.isbn` followed.
    pub items_carried: u64,
}

/// Run the repair once per database; `None` when it already ran.
pub async fn repair_stored_isbns_once(pool: &DbPool) -> Result<Option<IsbnRepairReport>, AppError> {
    data_repairs::run_once(pool, STORED_ISBN_REPAIR, |conn| {
        Box::pin(async move {
            let report = repair_in(conn).await?;
            tracing::info!(
                rewritten = report.rewritten,
                rewritten_trashed = report.rewritten_trashed,
                skipped_collision = report.collided,
                skipped_canonical = report.canonical,
                skipped_not_isbn = report.not_isbn,
                skipped_superseded = report.superseded,
                items_carried = report.items_carried,
                "Stored ISBN: startup repair to the one rule done (#565)"
            );
            Ok(report)
        })
    })
    .await
}

/// The transactional part (pub for the DB integration tests, which run it
/// without the marker).
pub async fn repair_in(conn: &mut MySqlConnection) -> Result<IsbnRepairReport, AppError> {
    let stored = list_isbns(conn).await?;
    let rows: BTreeMap<u64, (i32, bool)> =
        stored.iter().map(|s| (s.id, (s.version, s.deleted))).collect();
    let plan = plan_isbn_repair(stored.iter().map(|s| (s.id, s.isbn.as_str())));
    let mut report = IsbnRepairReport {
        canonical: plan.canonical,
        not_isbn: plan.not_isbn.len(),
        ..Default::default()
    };
    for collision in &plan.collisions {
        report.collided += collision.titles.len();
        for (id, stored) in &collision.titles {
            let others: Vec<u64> =
                collision.titles.iter().map(|(o, _)| *o).filter(|o| o != id).collect();
            tracing::warn!(
                title_id = id,
                stored = %stored,
                isbn = %collision.isbn13,
                trashed = rows[id].1,
                colliding_title_ids = ?others,
                "Stored ISBN: another title would carry the same ISBN; left unchanged (see #591)"
            );
        }
    }
    for rewrite in &plan.rewrites {
        let (version, trashed) = rows[&rewrite.title_id];
        if !set_canonical_isbn(conn, rewrite.title_id, version, &rewrite.old, &rewrite.new).await? {
            report.superseded += 1;
            tracing::info!(
                title_id = rewrite.title_id,
                old = %rewrite.old,
                isbn = %rewrite.new,
                trashed,
                "Stored ISBN: title changed meanwhile; left unchanged"
            );
            continue;
        }
        report.rewritten += 1;
        report.items_carried +=
            ProposalModel::carry_identity_isbn(conn, rewrite.title_id, &rewrite.old, &rewrite.new)
                .await?;
        report.rewritten_trashed += usize::from(trashed);
        tracing::info!(
            title_id = rewrite.title_id,
            old = %rewrite.old,
            isbn = %rewrite.new,
            trashed,
            "Stored ISBN rewritten to its ISBN-13"
        );
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rw(id: u64, old: &str, new: &str) -> IsbnRewrite {
        IsbnRewrite { title_id: id, old: old.into(), new: new.into() }
    }

    #[test]
    fn positive_shapes_are_rewritten_everything_else_is_left() {
        let plan = plan_isbn_repair([
            (1, "9782070360246"),
            (2, "978-2-205-00121-1"),
            (3, "2-205-01150-2"),
            (4, "ISBN 978 2 07 036025 3"),
            (5, "080442957x"),
            (6, "978-2-07-036024-7"),
            (7, "9772070360246"),
            (8, "inconnu"),
            (9, "978207036024"),
        ]);
        assert_eq!(
            plan.rewrites,
            vec![
                rw(2, "978-2-205-00121-1", "9782205001211"),
                rw(3, "2-205-01150-2", "9782205011500"),
                rw(4, "ISBN 978 2 07 036025 3", "9782070360253"),
                rw(5, "080442957x", "9780804429573"),
            ]
        );
        assert_eq!(plan.canonical, 1);
        assert!(plan.collisions.is_empty());
        let left: Vec<u64> = plan.not_isbn.iter().map(|(id, _)| *id).collect();
        assert_eq!(left, vec![6, 7, 8, 9]);
    }

    #[test]
    fn titles_that_would_collide_are_all_left_untouched() {
        let plan = plan_isbn_repair([
            (10, "9782205001211"),
            (11, "978-2-205-00121-1"),
            (12, "2-205-01150-2"),
            (13, "2205011502"),
        ]);
        assert!(plan.rewrites.is_empty());
        assert_eq!(plan.canonical, 0);
        assert_eq!(
            plan.collisions,
            vec![
                IsbnCollision {
                    isbn13: "9782205001211".into(),
                    titles: vec![(10, "9782205001211".into()), (11, "978-2-205-00121-1".into())],
                },
                IsbnCollision {
                    isbn13: "9782205011500".into(),
                    titles: vec![(12, "2-205-01150-2".into()), (13, "2205011502".into())],
                },
            ]
        );
    }

    #[test]
    fn a_trashed_twin_blocks_the_rewrite_of_a_live_title() {
        // Coordinator decision (2026-11-10): the collision check spans live
        // and trashed titles. A live ISBN-10 whose duplicate (made by a
        // later scan) sits in the Trash is left as stored and logged (#669).
        let plan = plan_isbn_repair([(30, "2-205-00121-3"), (31, "9782205001211")]);
        assert!(plan.rewrites.is_empty());
        assert_eq!(plan.collisions.len(), 1);
    }

    #[test]
    fn canonical_duplicates_are_not_a_repair_collision() {
        // Two titles already sharing an ISBN-13 (#591) need no rewrite.
        let plan = plan_isbn_repair([(20, "9782205001211"), (21, "9782205001211")]);
        assert_eq!(plan, IsbnRepairPlan { canonical: 2, ..Default::default() });
    }

    #[test]
    fn the_plan_is_idempotent() {
        let first = plan_isbn_repair([(1, "978-2-205-00121-1"), (2, "2070360245")]);
        let after: Vec<(u64, String)> =
            first.rewrites.iter().map(|r| (r.title_id, r.new.clone())).collect();
        let second = plan_isbn_repair(after.iter().map(|(id, v)| (*id, v.as_str())));
        assert!(second.rewrites.is_empty());
        assert_eq!(second.canonical, 2);
    }

    /// Story 17-3 (Rule #20) — run the classification over a production
    /// export: `MYBIBLI_ISBN_CORPUS=/path/to/export.csv cargo test --lib
    /// production_corpus -- --ignored --nocapture`. The CSV has a header and
    /// the columns `id, isbn, deleted` (`deleted`: 1 / true for a title in
    /// the Trash, which the repair covers too). Never commit the export.
    /// Kept for later use: on 2026-11-10 the project lead had no export and
    /// replaced this step by the production-log analysis (story file).
    #[test]
    #[ignore]
    fn production_corpus() {
        let path = std::env::var("MYBIBLI_ISBN_CORPUS").expect("MYBIBLI_ISBN_CORPUS");
        let text = std::fs::read_to_string(&path).expect("read corpus");
        let mut rows = Vec::new();
        let mut trashed = std::collections::BTreeSet::new();
        for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
            let cols: Vec<&str> = line.split(',').map(|c| c.trim().trim_matches('"')).collect();
            let id: u64 = cols[0].parse().expect("id");
            let isbn = cols.get(1).copied().unwrap_or("");
            if isbn.is_empty() || isbn.eq_ignore_ascii_case("null") || isbn == "\\N" {
                continue;
            }
            if matches!(cols.get(2).copied(), Some("1" | "true" | "TRUE")) {
                trashed.insert(id);
            }
            rows.push((id, isbn.to_string()));
        }
        let mark = |id: &u64| if trashed.contains(id) { " (trash)" } else { "" };
        let plan = plan_isbn_repair(rows.iter().map(|(id, v)| (*id, v.as_str())));
        println!("titles with an ISBN: {} (of which in the Trash: {})", rows.len(), trashed.len());
        println!("canonical (unchanged): {}", plan.canonical);
        println!("rewrites: {}", plan.rewrites.len());
        for r in &plan.rewrites {
            println!("  REWRITE title {}{}: {:?} -> {}", r.title_id, mark(&r.title_id), r.old, r.new);
        }
        println!("collisions (left untouched): {}", plan.collisions.len());
        for c in &plan.collisions {
            println!("  COLLISION {}: {:?}", c.isbn13, c.titles);
        }
        println!("not an ISBN (left untouched): {}", plan.not_isbn.len());
        for (id, v) in &plan.not_isbn {
            println!("  LEFT title {id}{}: {v:?}", mark(id));
        }
    }
}
