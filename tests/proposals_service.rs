//! Story 11-9 — DB-backed tests for `services::proposals`
//! (FR134, FR140, NFR47, AR29).
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test proposals_service

use mybibli::models::proposal::{
    ProposalFilter, ProposalKind, ProposalStatus, ProposalTarget, ProposalTargetTable,
};
use mybibli::services::proposals::{
    Decision, NewProposal, NewProposalItem, ProposalError, ProposalService, SourceRejection,
    StaleReason, Transition,
};
use serde_json::json;
use sqlx::MySqlPool;

// ─── Fixtures ───────────────────────────────────────────────────────

async fn insert_key(pool: &MySqlPool, label: &str) -> u64 {
    sqlx::query(
        "INSERT INTO api_keys (label, key_hash, key_prefix, scope) VALUES (?, 'h', 'mybibli_pr_x', 'propose')",
    )
    .bind(label)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn insert_user(pool: &MySqlPool, username: &str) -> u64 {
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, 'x', 'librarian')")
        .bind(username)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn insert_title(pool: &MySqlPool, title: &str) -> u64 {
    let genre: u64 =
        sqlx::query_scalar("SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id) VALUES (?, 'fr', 'book', ?)",
    )
    .bind(title)
    .bind(genre)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn insert_series(pool: &MySqlPool, name: &str) -> u64 {
    sqlx::query(
        "INSERT INTO series (name, series_type, total_volume_count) VALUES (?, 'closed', 5)",
    )
    .bind(name)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn bump_version(pool: &MySqlPool, table: &str, id: u64) {
    sqlx::query(&format!(
        "UPDATE {table} SET version = version + 1 WHERE id = ?"
    ))
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

async fn soft_delete(pool: &MySqlPool, table: &str, id: u64) {
    sqlx::query(&format!(
        "UPDATE {table} SET deleted_at = NOW() WHERE id = ?"
    ))
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

async fn proposal_count(pool: &MySqlPool) -> (i64, i64) {
    let p: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM proposals")
        .fetch_one(pool)
        .await
        .unwrap();
    let i: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM proposal_items")
        .fetch_one(pool)
        .await
        .unwrap();
    (p, i)
}

fn title_target(id: u64) -> Option<ProposalTarget> {
    Some(ProposalTarget {
        table: ProposalTargetTable::Titles,
        id,
    })
}

fn series_target(id: u64) -> Option<ProposalTarget> {
    Some(ProposalTarget {
        table: ProposalTargetTable::Series,
        id,
    })
}

fn proposal(kind: ProposalKind, key: u64, targets: &[Option<ProposalTarget>]) -> NewProposal {
    NewProposal {
        kind,
        api_key_id: key,
        sources: vec!["https://www.bnf.fr/notice/1".to_string()],
        items: targets
            .iter()
            .enumerate()
            .map(|(i, t)| NewProposalItem {
                target: *t,
                expected_version: None,
                proposed_values: json!({ "line": i + 1 }),
            })
            .collect(),
    }
}

async fn file(pool: &MySqlPool, p: &NewProposal) -> u64 {
    ProposalService::file(pool, p)
        .await
        .expect("filing must succeed")
}

async fn status(pool: &MySqlPool, id: u64) -> ProposalStatus {
    ProposalService::detail(pool, id)
        .await
        .unwrap()
        .expect("proposal exists")
        .proposal
        .status
}

// ─── Filing ─────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn filing_stores_sources_items_and_target_versions(pool: MySqlPool) {
    let key = insert_key(&pool, "Claude agent").await;
    let title = insert_title(&pool, "Astérix le Gaulois").await;
    bump_version(&pool, "titles", title).await;
    bump_version(&pool, "titles", title).await; // version 3
    let series = insert_series(&pool, "Astérix").await;

    let mut p = proposal(
        ProposalKind::SeriesAttach,
        key,
        &[series_target(series), title_target(title), None],
    );
    p.sources = vec![
        "  HTTPS://WWW.Example.org/a  ".to_string(),
        "https://www.example.org/a".to_string(),
        "http://catalogue.bnf.fr/ark:/12148/cb1".to_string(),
    ];
    let id = file(&pool, &p).await;

    let d = ProposalService::detail(&pool, id).await.unwrap().unwrap();
    assert_eq!(d.proposal.kind, ProposalKind::SeriesAttach);
    assert_eq!(d.proposal.status, ProposalStatus::Pending);
    assert_eq!(d.proposal.api_key_id, key);
    assert_eq!(d.proposal.api_key_label.as_deref(), Some("Claude agent"));
    assert_eq!(d.proposal.version, 1);
    assert_eq!(
        d.proposal.sources,
        vec![
            "https://www.example.org/a",
            "http://catalogue.bnf.fr/ark:/12148/cb1"
        ]
    );
    assert!(d.proposal.decided_by.is_none() && d.proposal.decided_at.is_none());
    assert!(d.proposal.rejection_reason.is_none());

    assert_eq!(d.items.len(), 3);
    assert_eq!(d.items[0].target, series_target(series));
    assert_eq!(d.items[0].target_version, Some(1));
    assert_eq!(d.items[1].target, title_target(title));
    assert_eq!(d.items[1].target_version, Some(3));
    assert_eq!(d.items[2].target, None);
    assert_eq!(d.items[2].target_version, None);
    assert_eq!(d.items[2].proposed_values, json!({ "line": 3 }));
    assert!(d.items[0].id < d.items[1].id && d.items[1].id < d.items[2].id);
}

#[sqlx::test(migrations = "./migrations")]
async fn filing_refuses_invalid_sources_and_writes_nothing(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let title = insert_title(&pool, "T").await;

    let mut p = proposal(ProposalKind::Metadata, key, &[title_target(title)]);
    p.sources.clear();
    assert!(matches!(
        ProposalService::file(&pool, &p).await,
        Err(ProposalError::NoSources)
    ));

    p.sources = vec![
        "https://ok.example/".to_string(),
        "javascript:alert(document.cookie)".to_string(),
    ];
    let err = ProposalService::file(&pool, &p).await.unwrap_err();
    assert!(err.is_invalid_input());
    assert!(matches!(
        &err,
        ProposalError::InvalidSource { index: 2, reason: SourceRejection::Scheme(s) } if s == "javascript"
    ));
    assert!(err.to_string().contains("Source 2"));

    assert_eq!(proposal_count(&pool).await, (0, 0));
}

#[sqlx::test(migrations = "./migrations")]
async fn filing_refuses_unknown_or_deleted_target_and_writes_nothing(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let live = insert_title(&pool, "Live").await;
    let gone = insert_title(&pool, "Gone").await;
    soft_delete(&pool, "titles", gone).await;

    let p = proposal(
        ProposalKind::SeriesCreate,
        key,
        &[None, title_target(live), title_target(gone)],
    );
    assert!(matches!(
        ProposalService::file(&pool, &p).await,
        Err(ProposalError::TargetNotFound { index: 3, table: ProposalTargetTable::Titles, id }) if id == gone
    ));

    let p = proposal(ProposalKind::SeriesAttach, key, &[series_target(999_999)]);
    assert!(matches!(
        ProposalService::file(&pool, &p).await,
        Err(ProposalError::TargetNotFound {
            index: 1,
            table: ProposalTargetTable::Series,
            id: 999_999
        })
    ));

    let p = proposal(ProposalKind::Metadata, key, &[]);
    assert!(matches!(
        ProposalService::file(&pool, &p).await,
        Err(ProposalError::NoItems)
    ));

    assert_eq!(proposal_count(&pool).await, (0, 0));
}

#[sqlx::test(migrations = "./migrations")]
async fn check_constraint_refuses_a_half_set_target(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let id = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;

    for (table, target_id, version) in [
        (Some("titles"), None, None),
        (Some("titles"), Some(1_u64), None),
        (None, Some(1_u64), Some(1_i32)),
    ] {
        let res = sqlx::query(
            "INSERT INTO proposal_items (proposal_id, target_table, target_id, target_version, proposed_values) \
             VALUES (?, ?, ?, ?, '{}')",
        )
        .bind(id)
        .bind(table)
        .bind(target_id)
        .bind(version)
        .execute(&pool)
        .await;
        assert!(
            res.is_err(),
            "{table:?} {target_id:?} {version:?} must be refused"
        );
    }
}

// ─── Listing and detail ─────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn listing_filters_paginates_and_hides_soft_deleted(pool: MySqlPool) {
    let key_a = insert_key(&pool, "Agent A").await;
    let key_b = insert_key(&pool, "Agent B").await;
    let user = insert_user(&pool, "lib-list").await;
    let title = insert_title(&pool, "T").await;

    let mut a_ids = Vec::new();
    for _ in 0..26 {
        a_ids.push(
            file(
                &pool,
                &proposal(ProposalKind::Metadata, key_a, &[title_target(title), None]),
            )
            .await,
        );
    }
    let b1 = file(
        &pool,
        &proposal(ProposalKind::Cover, key_b, &[title_target(title)]),
    )
    .await;
    let b2 = file(&pool, &proposal(ProposalKind::SeriesCreate, key_b, &[None])).await;
    ProposalService::transition(
        &pool,
        b1,
        &Decision::Reject {
            by: user,
            reason: None,
        },
    )
    .await
    .unwrap();
    // A soft-deleted proposal and a soft-deleted item never show.
    soft_delete(&pool, "proposals", a_ids[0]).await;
    sqlx::query(
        "UPDATE proposal_items SET deleted_at = NOW() WHERE proposal_id = ? AND target_id IS NULL",
    )
    .bind(a_ids[1])
    .execute(&pool)
    .await
    .unwrap();
    // Revoking a key keeps its label visible.
    sqlx::query("UPDATE api_keys SET revoked_at = NOW() WHERE id = ?")
        .bind(key_b)
        .execute(&pool)
        .await
        .unwrap();

    let all = ProposalService::list(&pool, &ProposalFilter::default(), 1)
        .await
        .unwrap();
    assert_eq!(all.total_items, 27);
    assert_eq!(all.total_pages, 2);
    assert_eq!(all.items.len(), 25);
    // Newest first.
    assert_eq!(all.items[0].id, b2);
    assert_eq!(all.items[1].id, b1);
    assert!(all.items.iter().all(|s| s.id != a_ids[0]));

    let page2 = ProposalService::list(&pool, &ProposalFilter::default(), 2)
        .await
        .unwrap();
    assert_eq!(page2.items.len(), 2);
    let a1 = page2
        .items
        .iter()
        .find(|s| s.id == a_ids[1])
        .expect("oldest live on page 2");
    assert_eq!(a1.item_count, 1);
    assert_eq!(a1.api_key_label.as_deref(), Some("Agent A"));

    let rejected = ProposalService::list(
        &pool,
        &ProposalFilter {
            status: Some(ProposalStatus::Rejected),
            ..Default::default()
        },
        1,
    )
    .await
    .unwrap();
    assert_eq!(rejected.items.len(), 1);
    assert_eq!(rejected.items[0].id, b1);
    assert_eq!(rejected.items[0].api_key_label.as_deref(), Some("Agent B"));
    assert!(rejected.items[0].decided_at.is_some());

    let by_kind = ProposalService::list(
        &pool,
        &ProposalFilter {
            kind: Some(ProposalKind::SeriesCreate),
            ..Default::default()
        },
        1,
    )
    .await
    .unwrap();
    assert_eq!(
        by_kind.items.iter().map(|s| s.id).collect::<Vec<_>>(),
        vec![b2]
    );

    let by_key = ProposalService::list(
        &pool,
        &ProposalFilter {
            api_key_id: Some(key_b),
            status: Some(ProposalStatus::Pending),
            kind: None,
        },
        1,
    )
    .await
    .unwrap();
    assert_eq!(
        by_key.items.iter().map(|s| s.id).collect::<Vec<_>>(),
        vec![b2]
    );

    // Page 0 is read as page 1; a page past the end is empty with true totals.
    let p0 = ProposalService::list(&pool, &ProposalFilter::default(), 0)
        .await
        .unwrap();
    assert_eq!(p0.page, 1);
    let p9 = ProposalService::list(&pool, &ProposalFilter::default(), 9)
        .await
        .unwrap();
    assert!(p9.items.is_empty());
    assert_eq!(p9.total_items, 27);
}

#[sqlx::test(migrations = "./migrations")]
async fn detail_is_none_for_unknown_or_soft_deleted(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    assert!(
        ProposalService::detail(&pool, 424_242)
            .await
            .unwrap()
            .is_none()
    );
    let id = file(
        &pool,
        &proposal(ProposalKind::SeriesCreate, key, &[None, None]),
    )
    .await;
    sqlx::query(
        "UPDATE proposal_items SET deleted_at = NOW() WHERE proposal_id = ? ORDER BY id LIMIT 1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    let d = ProposalService::detail(&pool, id).await.unwrap().unwrap();
    assert_eq!(d.items.len(), 1, "a soft-deleted item is not listed");
    assert_eq!(d.items[0].proposed_values, json!({ "line": 2 }));
    soft_delete(&pool, "proposals", id).await;
    assert!(ProposalService::detail(&pool, id).await.unwrap().is_none());
}

// ─── Transitions ────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn accept_and_reject_record_the_decision(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let user = insert_user(&pool, "lib-decide").await;

    let a = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;
    assert_eq!(
        ProposalService::transition(&pool, a, &Decision::Accept { by: user })
            .await
            .unwrap(),
        ProposalStatus::Accepted
    );
    let d = ProposalService::detail(&pool, a)
        .await
        .unwrap()
        .unwrap()
        .proposal;
    assert_eq!(d.status, ProposalStatus::Accepted);
    assert_eq!(d.decided_by, Some(user));
    assert_eq!(d.decided_by_username.as_deref(), Some("lib-decide"));
    assert!(d.decided_at.is_some());
    assert_eq!(d.version, 2);
    assert!(d.rejection_reason.is_none());

    let r = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;
    ProposalService::transition(
        &pool,
        r,
        &Decision::Reject {
            by: user,
            reason: Some("  Wrong series, see tome 3  ".into()),
        },
    )
    .await
    .unwrap();
    let d = ProposalService::detail(&pool, r)
        .await
        .unwrap()
        .unwrap()
        .proposal;
    assert_eq!(d.status, ProposalStatus::Rejected);
    assert_eq!(
        d.rejection_reason.as_deref(),
        Some("Wrong series, see tome 3")
    );
    assert_eq!(d.decided_by, Some(user));

    let r2 = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;
    ProposalService::transition(
        &pool,
        r2,
        &Decision::Reject {
            by: user,
            reason: Some("   ".into()),
        },
    )
    .await
    .unwrap();
    let d = ProposalService::detail(&pool, r2)
        .await
        .unwrap()
        .unwrap()
        .proposal;
    assert!(d.rejection_reason.is_none());

    let s = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;
    ProposalService::transition(&pool, s, &Decision::MarkStale)
        .await
        .unwrap();
    let d = ProposalService::detail(&pool, s)
        .await
        .unwrap()
        .unwrap()
        .proposal;
    assert_eq!(d.status, ProposalStatus::Stale);
    assert!(d.decided_by.is_none());
    assert!(d.decided_at.is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn nothing_leaves_a_final_state(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let user = insert_user(&pool, "lib-final").await;

    let decisions = [
        Decision::Accept { by: user },
        Decision::Reject {
            by: user,
            reason: None,
        },
        Decision::MarkStale,
    ];
    for first in &decisions {
        let id = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;
        let reached = ProposalService::transition(&pool, id, first).await.unwrap();
        for next in &decisions {
            match ProposalService::transition(&pool, id, next).await {
                Err(ProposalError::NotPending { id: got, status }) => {
                    assert_eq!(got, id);
                    assert_eq!(status, reached);
                }
                other => panic!("{first:?} then {next:?}: expected NotPending, got {other:?}"),
            }
        }
        let d = ProposalService::detail(&pool, id)
            .await
            .unwrap()
            .unwrap()
            .proposal;
        assert_eq!(d.status, reached);
        assert_eq!(d.version, 2, "a refused transition must not touch the row");
    }

    assert!(matches!(
        ProposalService::transition(&pool, 777_777, &Decision::MarkStale).await,
        Err(ProposalError::NotFound { id: 777_777 })
    ));
    let gone = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;
    soft_delete(&pool, "proposals", gone).await;
    assert!(matches!(
        ProposalService::transition(&pool, gone, &Decision::Accept { by: user }).await,
        Err(ProposalError::NotFound { .. })
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn too_long_reason_leaves_the_proposal_pending(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let user = insert_user(&pool, "lib-reason").await;
    let id = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;
    let reason = "x".repeat(1001);
    assert!(matches!(
        ProposalService::transition(
            &pool,
            id,
            &Decision::Reject {
                by: user,
                reason: Some(reason)
            }
        )
        .await,
        Err(ProposalError::ReasonTooLong { chars: 1001 })
    ));
    assert_eq!(status(&pool, id).await, ProposalStatus::Pending);
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_deciders_cannot_both_win(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let u1 = insert_user(&pool, "lib-one").await;
    let u2 = insert_user(&pool, "lib-two").await;
    let id = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;

    // The first decider holds its transaction open; the second must wait
    // on the row lock, then see the first decision.
    let mut tx = pool.begin().await.unwrap();
    assert_eq!(
        ProposalService::transition_in(&mut tx, id, &Decision::Accept { by: u1 })
            .await
            .unwrap(),
        Transition::Decided(ProposalStatus::Accepted)
    );
    let second = {
        let pool = pool.clone();
        tokio::spawn(async move {
            ProposalService::transition(
                &pool,
                id,
                &Decision::Reject {
                    by: u2,
                    reason: None,
                },
            )
            .await
        })
    };
    // Wait until the second decider is actually blocked on the row lock,
    // so the test proves the guard under real contention.
    wait_until_blocked(&pool, "SELECT status FROM proposals%FOR UPDATE").await;
    tx.commit().await.unwrap();
    match second.await.unwrap() {
        Err(ProposalError::NotPending { status, .. }) => {
            assert_eq!(status, ProposalStatus::Accepted)
        }
        other => panic!("expected NotPending(accepted), got {other:?}"),
    }
    let d = ProposalService::detail(&pool, id)
        .await
        .unwrap()
        .unwrap()
        .proposal;
    assert_eq!(d.status, ProposalStatus::Accepted);
    assert_eq!(d.decided_by, Some(u1));
    assert_eq!(d.version, 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn accepting_a_stale_proposal_marks_it_stale(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let user = insert_user(&pool, "lib-accept-stale").await;
    let title = insert_title(&pool, "T").await;
    let id = file(
        &pool,
        &proposal(ProposalKind::Metadata, key, &[title_target(title)]),
    )
    .await;
    bump_version(&pool, "titles", title).await;

    match ProposalService::transition(&pool, id, &Decision::Accept { by: user }).await {
        Err(ProposalError::Stale { id: got, targets }) => {
            assert_eq!(got, id);
            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].reason, StaleReason::Changed);
        }
        other => panic!("expected Stale, got {other:?}"),
    }
    let d = ProposalService::detail(&pool, id)
        .await
        .unwrap()
        .unwrap()
        .proposal;
    assert_eq!(
        d.status,
        ProposalStatus::Stale,
        "the stale mark is committed"
    );
    assert!(d.decided_by.is_none());

    // Rejecting does not need fresh targets.
    let id2 = file(
        &pool,
        &proposal(ProposalKind::Metadata, key, &[title_target(title)]),
    )
    .await;
    bump_version(&pool, "titles", title).await;
    assert_eq!(
        ProposalService::transition(
            &pool,
            id2,
            &Decision::Reject {
                by: user,
                reason: None
            }
        )
        .await
        .unwrap(),
        ProposalStatus::Rejected
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_decision_needs_an_active_user(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let gone = insert_user(&pool, "lib-deactivated").await;
    soft_delete(&pool, "users", gone).await;
    let id = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;

    for by in [gone, 9_999_999] {
        assert!(matches!(
            ProposalService::transition(&pool, id, &Decision::Accept { by }).await,
            Err(ProposalError::UnknownDecider { user_id }) if user_id == by
        ));
    }
    assert_eq!(status(&pool, id).await, ProposalStatus::Pending);
}

#[sqlx::test(migrations = "./migrations")]
async fn only_a_live_propose_key_can_file(pool: MySqlPool) {
    let title = insert_title(&pool, "T").await;
    let revoked = insert_key(&pool, "revoked").await;
    sqlx::query("UPDATE api_keys SET revoked_at = NOW() WHERE id = ?")
        .bind(revoked)
        .execute(&pool)
        .await
        .unwrap();
    let deleted = insert_key(&pool, "deleted").await;
    soft_delete(&pool, "api_keys", deleted).await;
    let read_key = insert_key(&pool, "read").await;
    sqlx::query("UPDATE api_keys SET scope = 'read' WHERE id = ?")
        .bind(read_key)
        .execute(&pool)
        .await
        .unwrap();

    for key in [revoked, deleted, read_key, 8_888_888] {
        let p = proposal(ProposalKind::Metadata, key, &[title_target(title)]);
        assert!(matches!(
            ProposalService::file(&pool, &p).await,
            Err(ProposalError::KeyNotAllowed { api_key_id }) if api_key_id == key
        ));
    }
    assert_eq!(proposal_count(&pool).await, (0, 0));
}

#[sqlx::test(migrations = "./migrations")]
async fn deeply_nested_values_are_refused_before_the_database(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let mut deep = json!(1);
    for _ in 0..40 {
        deep = json!({ "a": deep });
    }
    let p = NewProposal {
        kind: ProposalKind::SeriesCreate,
        api_key_id: key,
        sources: vec!["https://example.org/".into()],
        items: vec![NewProposalItem {
            target: None,
            expected_version: None,
            proposed_values: deep,
        }],
    };
    let err = ProposalService::file(&pool, &p).await.unwrap_err();
    assert!(err.is_invalid_input(), "{err:?}");
    assert!(matches!(err, ProposalError::ValuesTooDeep { index: 1 }));
    assert_eq!(proposal_count(&pool).await, (0, 0));
}

#[sqlx::test(migrations = "./migrations")]
async fn decided_by_is_set_null_when_the_user_row_is_purged(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let user = insert_user(&pool, "lib-purged").await;
    let id = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;
    ProposalService::transition(&pool, id, &Decision::Accept { by: user })
        .await
        .unwrap();

    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(user)
        .execute(&pool)
        .await
        .expect("a decision must not block the 30-day user purge");
    let d = ProposalService::detail(&pool, id)
        .await
        .unwrap()
        .unwrap()
        .proposal;
    assert_eq!(d.status, ProposalStatus::Accepted);
    assert!(d.decided_by.is_none());
    assert_eq!(
        d.decided_by_username.as_deref(),
        Some("lib-purged"),
        "the decider's name survives the purge (FR146)"
    );
    assert!(d.decided_at.is_some());
}

// ─── Staleness ──────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn unchanged_targets_keep_the_proposal_pending(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let title = insert_title(&pool, "T").await;
    let series = insert_series(&pool, "S").await;
    let id = file(
        &pool,
        &proposal(
            ProposalKind::SeriesAttach,
            key,
            &[series_target(series), title_target(title), None],
        ),
    )
    .await;
    // A change to an untargeted entity does not matter.
    let other = insert_title(&pool, "Other").await;
    bump_version(&pool, "titles", other).await;

    let check = ProposalService::check_staleness(&pool, id).await.unwrap();
    assert_eq!(check.status, ProposalStatus::Pending);
    assert!(check.stale_targets.is_empty());
    assert_eq!(status(&pool, id).await, ProposalStatus::Pending);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_version_change_makes_the_proposal_stale(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let title = insert_title(&pool, "T").await;
    let series = insert_series(&pool, "S").await;
    let id = file(
        &pool,
        &proposal(
            ProposalKind::SeriesAttach,
            key,
            &[series_target(series), title_target(title)],
        ),
    )
    .await;
    bump_version(&pool, "series", series).await;

    let targets = ProposalService::stale_targets(&pool, id).await.unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].target, series_target(series).unwrap());
    assert_eq!(targets[0].reason, StaleReason::Changed);
    // Reading the stale targets alone changes nothing.
    assert_eq!(status(&pool, id).await, ProposalStatus::Pending);

    let check = ProposalService::check_staleness(&pool, id).await.unwrap();
    assert_eq!(check.status, ProposalStatus::Stale);
    assert_eq!(check.stale_targets, targets);
    let d = ProposalService::detail(&pool, id)
        .await
        .unwrap()
        .unwrap()
        .proposal;
    assert_eq!(d.status, ProposalStatus::Stale);
    assert!(d.decided_by.is_none() && d.decided_at.is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_soft_or_hard_deleted_target_makes_the_proposal_stale(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;

    let soft = insert_title(&pool, "Soft").await;
    let p1 = file(
        &pool,
        &proposal(ProposalKind::Metadata, key, &[title_target(soft)]),
    )
    .await;
    soft_delete(&pool, "titles", soft).await;
    let check = ProposalService::check_staleness(&pool, p1).await.unwrap();
    assert_eq!(check.status, ProposalStatus::Stale);
    assert_eq!(check.stale_targets[0].reason, StaleReason::Deleted);

    // The 30-day auto-purge hard-deletes the row: no FK blocks it, and
    // the proposal reads it as deleted.
    let hard = insert_title(&pool, "Hard").await;
    let p2 = file(
        &pool,
        &proposal(ProposalKind::Cover, key, &[title_target(hard)]),
    )
    .await;
    sqlx::query("DELETE FROM titles WHERE id = ?")
        .bind(hard)
        .execute(&pool)
        .await
        .expect("a proposal must not block the purge of its target");
    let check = ProposalService::check_staleness(&pool, p2).await.unwrap();
    assert_eq!(check.status, ProposalStatus::Stale);
    assert_eq!(check.stale_targets.len(), 1);
    assert_eq!(check.stale_targets[0].reason, StaleReason::Deleted);
}

#[sqlx::test(migrations = "./migrations")]
async fn staleness_leaves_a_decided_proposal_untouched(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let user = insert_user(&pool, "lib-stale").await;
    let title = insert_title(&pool, "T").await;
    let id = file(
        &pool,
        &proposal(ProposalKind::Metadata, key, &[title_target(title)]),
    )
    .await;
    ProposalService::transition(
        &pool,
        id,
        &Decision::Reject {
            by: user,
            reason: None,
        },
    )
    .await
    .unwrap();
    bump_version(&pool, "titles", title).await;

    let check = ProposalService::check_staleness(&pool, id).await.unwrap();
    assert_eq!(check.status, ProposalStatus::Rejected);
    assert!(check.stale_targets.is_empty());
    let d = ProposalService::detail(&pool, id)
        .await
        .unwrap()
        .unwrap()
        .proposal;
    assert_eq!(d.status, ProposalStatus::Rejected);
    assert_eq!(d.version, 2);

    assert!(matches!(
        ProposalService::check_staleness(&pool, 31_337).await,
        Err(ProposalError::NotFound { id: 31_337 })
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn staleness_in_a_rolled_back_transaction_changes_nothing(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let title = insert_title(&pool, "T").await;
    let id = file(
        &pool,
        &proposal(ProposalKind::Metadata, key, &[title_target(title)]),
    )
    .await;
    bump_version(&pool, "titles", title).await;

    let mut tx = pool.begin().await.unwrap();
    let check = ProposalService::check_staleness_in(&mut tx, id)
        .await
        .unwrap();
    assert_eq!(check.status, ProposalStatus::Stale);
    tx.rollback().await.unwrap();
    assert_eq!(status(&pool, id).await, ProposalStatus::Pending);
}

#[sqlx::test(migrations = "./migrations")]
async fn soft_deleted_items_are_ignored_by_staleness(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let title = insert_title(&pool, "T").await;
    let id = file(
        &pool,
        &proposal(ProposalKind::Metadata, key, &[title_target(title), None]),
    )
    .await;
    bump_version(&pool, "titles", title).await;
    // A librarian removed the targeted line (story 11-19): it no longer counts.
    sqlx::query("UPDATE proposal_items SET deleted_at = NOW() WHERE proposal_id = ? AND target_id IS NOT NULL")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let check = ProposalService::check_staleness(&pool, id).await.unwrap();
    assert_eq!(check.status, ProposalStatus::Pending);
    assert!(check.stale_targets.is_empty());
}

// ─── Review round 2 ─────────────────────────────────────────────────

/// Wait until a statement matching `pattern` (SQL LIKE) is executing on
/// this test's database — i.e. blocked, when another transaction holds
/// the lock it needs.
async fn wait_until_blocked(pool: &MySqlPool, pattern: &str) {
    for _ in 0..250 {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.PROCESSLIST \
             WHERE DB = DATABASE() AND COMMAND IN ('Execute', 'Query') AND INFO LIKE ?",
        )
        .bind(pattern)
        .fetch_one(pool)
        .await
        .unwrap();
        if n > 0 {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("no statement matching {pattern} ever waited");
}

#[sqlx::test(migrations = "./migrations")]
async fn accepting_fresh_targets_holds_them_until_commit(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let user = insert_user(&pool, "lib-hold").await;
    let title = insert_title(&pool, "T").await;
    let series = insert_series(&pool, "S").await;
    let id = file(
        &pool,
        &proposal(
            ProposalKind::SeriesAttach,
            key,
            &[series_target(series), title_target(title)],
        ),
    )
    .await;

    let mut tx = pool.begin().await.unwrap();
    assert_eq!(
        ProposalService::transition_in(&mut tx, id, &Decision::Accept { by: user })
            .await
            .unwrap(),
        Transition::Decided(ProposalStatus::Accepted)
    );
    // A librarian edit of the target must wait for the accepting
    // transaction (11-17 applies onto exactly what was checked).
    let edit = {
        let pool = pool.clone();
        tokio::spawn(async move {
            sqlx::query("UPDATE titles SET version = version + 1 WHERE id = ?")
                .bind(title)
                .execute(&pool)
                .await
                .unwrap();
        })
    };
    wait_until_blocked(&pool, "UPDATE titles SET version%").await;
    assert!(!edit.is_finished());
    tx.commit().await.unwrap();
    edit.await.unwrap();
    assert_eq!(status(&pool, id).await, ProposalStatus::Accepted);
}

#[sqlx::test(migrations = "./migrations")]
async fn accept_sees_changes_committed_after_its_snapshot(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let user = insert_user(&pool, "lib-snapshot").await;
    let title = insert_title(&pool, "T").await;
    let id = file(
        &pool,
        &proposal(ProposalKind::Metadata, key, &[title_target(title)]),
    )
    .await;

    // The caller's transaction takes its snapshot before the edit…
    let mut tx = pool.begin().await.unwrap();
    let _: i32 = sqlx::query_scalar("SELECT version FROM titles WHERE id = ?")
        .bind(title)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    bump_version(&pool, "titles", title).await;
    // …yet the accept's locking read sees the committed edit.
    match ProposalService::transition_in(&mut tx, id, &Decision::Accept { by: user })
        .await
        .unwrap()
    {
        Transition::WentStale(targets) => assert_eq!(targets[0].reason, StaleReason::Changed),
        other => panic!("expected WentStale, got {other:?}"),
    }
    tx.commit().await.unwrap();
    assert_eq!(status(&pool, id).await, ProposalStatus::Stale);
}

#[sqlx::test(migrations = "./migrations")]
async fn filing_refuses_a_target_changed_since_the_agent_read_it(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let title = insert_title(&pool, "T").await;
    bump_version(&pool, "titles", title).await; // version 2

    let mut p = proposal(ProposalKind::Metadata, key, &[title_target(title)]);
    p.items[0].expected_version = Some(1);
    assert!(matches!(
        ProposalService::file(&pool, &p).await,
        Err(ProposalError::TargetChanged {
            index: 1,
            expected: 1,
            current: 2,
            ..
        })
    ));
    assert_eq!(proposal_count(&pool).await, (0, 0));

    p.items[0].expected_version = Some(2);
    let id = file(&pool, &p).await;
    let d = ProposalService::detail(&pool, id).await.unwrap().unwrap();
    assert_eq!(d.items[0].target_version, Some(2));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_proposal_without_items_can_only_be_rejected(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let user = insert_user(&pool, "lib-empty").await;
    let id = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;
    sqlx::query("UPDATE proposal_items SET deleted_at = NOW() WHERE proposal_id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    assert!(matches!(
        ProposalService::transition(&pool, id, &Decision::Accept { by: user }).await,
        Err(ProposalError::NothingToApply { .. })
    ));
    assert_eq!(status(&pool, id).await, ProposalStatus::Pending);
    assert_eq!(
        ProposalService::transition(
            &pool,
            id,
            &Decision::Reject {
                by: user,
                reason: None
            }
        )
        .await
        .unwrap(),
        ProposalStatus::Rejected
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn accept_sees_items_and_deciders_changed_after_its_snapshot(pool: MySqlPool) {
    let key = insert_key(&pool, "k").await;
    let user = insert_user(&pool, "lib-items-snapshot").await;
    let id = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;

    // Snapshot taken, then every item is removed (story 11-19)…
    let mut tx = pool.begin().await.unwrap();
    let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM proposal_items WHERE proposal_id = ?")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE proposal_items SET deleted_at = NOW() WHERE proposal_id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    // …and the accept still sees that nothing is left.
    assert!(matches!(
        ProposalService::transition_in(&mut tx, id, &Decision::Accept { by: user }).await,
        Err(ProposalError::NothingToApply { .. })
    ));
    tx.rollback().await.unwrap();

    // Same for a decider deactivated after the snapshot.
    let id2 = file(&pool, &proposal(ProposalKind::SeriesCreate, key, &[None])).await;
    let mut tx = pool.begin().await.unwrap();
    let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE id = ?")
        .bind(user)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    soft_delete(&pool, "users", user).await;
    assert!(matches!(
        ProposalService::transition_in(&mut tx, id2, &Decision::Accept { by: user }).await,
        Err(ProposalError::UnknownDecider { .. })
    ));
    tx.rollback().await.unwrap();
}
