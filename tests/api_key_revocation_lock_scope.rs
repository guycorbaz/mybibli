//! Story 13-14 (#554, item 1) — lock scope of an API-key revocation.
//!
//! Two keys revoked at the same moment deadlocked (CI run 41604087139:
//! `1213 (40001)` on `POST /admin/api-keys/{id}/revoke`): the locking read
//! of a key's pending proposals could lock the library-wide pending range
//! instead of the key's own rows. It now starts from the key index, and
//! the revocation is retried on a deadlock.

use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyModel;
use mybibli::models::proposal::ProposalModel;
use mybibli::services::proposals::ProposalService;
use std::time::Duration;

async fn seed_propose_key(pool: &DbPool, label: &str) -> u64 {
    sqlx::query(
        "INSERT INTO api_keys (label, key_hash, key_prefix, scope) VALUES (?, 'x', ?, 'propose')",
    )
    .bind(label)
    .bind(format!("mybibli_pr_{label}"))
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn seed_pending(pool: &DbPool, key: u64) -> u64 {
    sqlx::query("INSERT INTO proposals (kind, api_key_id, sources) VALUES ('metadata', ?, '[]')")
        .bind(key)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn status(pool: &DbPool, id: u64) -> String {
    sqlx::query_scalar("SELECT CAST(status AS CHAR) FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn key_version(pool: &DbPool, id: u64) -> i32 {
    sqlx::query_scalar("SELECT version FROM api_keys WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn a_revocation_does_not_lock_proposals_of_another_key(pool: DbPool) {
    let a = seed_propose_key(&pool, "a").await;
    let b = seed_propose_key(&pool, "b").await;
    let a_prop = seed_pending(&pool, a).await;
    let b_prop = seed_pending(&pool, b).await;
    // More pending rows of other keys, so the status range is not the
    // two rows alone.
    let other = seed_propose_key(&pool, "other").await;
    for _ in 0..5 {
        seed_pending(&pool, other).await;
    }

    // Transaction 1: what revoking key A runs, kept open.
    let mut tx1 = pool.begin().await.unwrap();
    let va = key_version(&pool, a).await;
    assert_eq!(ApiKeyModel::revoke_in(&mut tx1, a, va).await.unwrap(), 1);
    let locked = ProposalModel::lock_pending_of_key(&mut tx1, a)
        .await
        .unwrap();
    assert_eq!(locked, vec![a_prop]);

    // Transaction 2: revoking key B must not wait for key A's locks.
    let mut tx2 = pool.begin().await.unwrap();
    sqlx::query("SET SESSION innodb_lock_wait_timeout = 2")
        .execute(&mut *tx2)
        .await
        .unwrap();
    let vb = key_version(&pool, b).await;
    assert_eq!(ApiKeyModel::revoke_in(&mut tx2, b, vb).await.unwrap(), 1);
    let locked_b = ProposalModel::lock_pending_of_key(&mut tx2, b)
        .await
        .expect("revoking key B must not wait for key A's proposal locks");
    assert_eq!(locked_b, vec![b_prop]);
    // Do not hand a 2 s lock-wait timeout back to the pool.
    sqlx::query("SET SESSION innodb_lock_wait_timeout = DEFAULT")
        .execute(&mut *tx2)
        .await
        .unwrap();
    tx2.rollback().await.unwrap();
    tx1.rollback().await.unwrap();

    // The service, run for both keys at once, marks each key's own
    // proposal stale and nothing else.
    let (ra, rb) = futures::future::join(
        ProposalService::revoke_key(&pool, a, va),
        ProposalService::revoke_key(&pool, b, vb),
    )
    .await;
    assert_eq!(ra.unwrap().unwrap().stale_proposal_ids, vec![a_prop]);
    assert_eq!(rb.unwrap().unwrap().stale_proposal_ids, vec![b_prop]);
    assert_eq!(status(&pool, a_prop).await, "stale");
    assert_eq!(status(&pool, b_prop).await, "stale");
    let other_pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM proposals WHERE api_key_id = ? AND status = 'pending'",
    )
    .bind(other)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(other_pending, 5, "other keys' proposals are untouched");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_second_revocation_with_the_old_version_is_a_no_op(pool: DbPool) {
    let a = seed_propose_key(&pool, "a").await;
    seed_pending(&pool, a).await;
    let va = key_version(&pool, a).await;
    assert!(
        ProposalService::revoke_key(&pool, a, va)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        ProposalService::revoke_key(&pool, a, va)
            .await
            .unwrap()
            .is_none()
    );
}

// The retry is the real guarantee: provoke a genuine deadlock and check
// that the revocation, chosen as the victim, is re-run and succeeds.
// The blocker transaction holds the key's proposal, then asks for the key
// row the revocation holds — a cycle InnoDB breaks by rolling back the
// lighter transaction (the revocation: one row written, while the blocker
// has written many).
#[sqlx::test(migrations = "./migrations")]
async fn a_deadlocked_revocation_is_retried_and_succeeds(pool: DbPool) {
    let a = seed_propose_key(&pool, "a").await;
    let a_prop = seed_pending(&pool, a).await;
    let ballast = seed_propose_key(&pool, "ballast").await;
    let va = key_version(&pool, a).await;

    let mut blocker = pool.begin().await.unwrap();
    // Weight: many written rows, so InnoDB picks the revocation as victim.
    for _ in 0..30 {
        sqlx::query(
            "INSERT INTO proposals (kind, api_key_id, sources) VALUES ('metadata', ?, '[]')",
        )
        .bind(ballast)
        .execute(&mut *blocker)
        .await
        .unwrap();
    }
    sqlx::query("SELECT id FROM proposals WHERE id = ? FOR UPDATE")
        .bind(a_prop)
        .execute(&mut *blocker)
        .await
        .unwrap();

    // The revocation takes the key row, then waits for the proposal.
    let revoking = tokio::spawn({
        let pool = pool.clone();
        async move { ProposalService::revoke_key(&pool, a, va).await }
    });
    // Wait until it is blocked on the proposal lock (a statement of this
    // test database's connections in a lock wait).
    let mut waiting = false;
    for _ in 0..100 {
        let n: i64 = sqlx::query_scalar(
            // The revocation's locking read is still running only while it
            // waits for the blocker's row lock.
            "SELECT COUNT(*) FROM information_schema.PROCESSLIST \
             WHERE ID <> CONNECTION_ID() AND INFO LIKE '%FORCE INDEX (idx_proposals_api_key)%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        if n > 0 {
            waiting = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(waiting, "the revocation should be waiting for the blocker");

    // Close the cycle: the blocker asks for the key row.
    sqlx::query("SELECT id FROM api_keys WHERE id = ? FOR UPDATE")
        .bind(a)
        .execute(&mut *blocker)
        .await
        .expect("InnoDB rolls back the lighter transaction, not the blocker");
    blocker.commit().await.unwrap();

    let revoked = revoking
        .await
        .unwrap()
        .expect("the deadlocked revocation is retried")
        .expect("the key is revoked on the retry");
    assert_eq!(revoked.stale_proposal_ids, vec![a_prop]);
    assert_eq!(status(&pool, a_prop).await, "stale");
}
