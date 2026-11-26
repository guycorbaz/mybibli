//! Story 19-8 (#657, T4) — the database steps boot runs after the
//! migrations, shared with a restore (DRY): a restored database goes
//! through exactly what a freshly started one does.
//!
//! Every step is non-fatal, as at boot: a failure is logged and the next
//! step runs (each repair is retried at the next start). Not here: the
//! `MYBIBLI_RESET_ADMIN` hatch (boot only, it exits the process) and the
//! startup purge (the scheduler does it after a restore).

use crate::db::DbPool;
use crate::services::{auto_purge, seed_gate};

/// Run the post-migration database steps, in boot order.
pub async fn prepare_database(pool: &DbPool) {
    // Issue #173 — production gate against the dev seed migrations.
    // Soft-deletes any user whose hash still matches the documented seed
    // hash unless MYBIBLI_SEED_DEV_USERS=1 opts back in (dev/E2E). Runs
    // right after the migrations so the setup wizard's
    // `active_admin_count == 0` predicate sees the correct count. A failed
    // gate leaves the seeded users in place, worth flagging in logs.
    match seed_gate::apply(pool).await {
        Ok(removed) if removed > 0 => {
            tracing::info!(
                removed_count = removed,
                "Seed gate removed {removed} seeded user(s) (issue #173)"
            );
        }
        Ok(_) => {}
        Err(e) => {
            tracing::error!(error = %e, "Seed gate failed (continuing, see issue #173)");
        }
    }

    // Validate FK dependency order against schema. Story 8-7 P5: never
    // panic here — schema evolution MUST NOT be a hard crash.
    if let Err(e) = auto_purge::AutoPurgeService::validate_schema(pool).await {
        tracing::warn!(
            error = %e,
            "FK schema validation failed; continuing startup (auto-purge may skip mismatched tables)"
        );
    }

    // Story 8-5: one-shot env-var migration (legacy provider keys copied
    // into empty `settings` rows), before the first settings load.
    if let Err(e) = crate::config::migrate_legacy_env_vars(pool).await {
        tracing::warn!(
            error = ?e,
            "Legacy env-var migration failed — continuing with current row values"
        );
    }

    // Story 17-3 (#565): stored ISBNs rewritten to their ISBN-13 once per
    // database; before the 13-3 conversion so its collision check sees
    // canonical values.
    if let Err(e) = crate::services::isbn_repair::repair_stored_isbns_once(pool).await {
        tracing::error!(
            error = %e,
            "Stored ISBN: startup repair failed (non-fatal, continuing)"
        );
    }

    // Story 13-3 (#584): ISBN-10 stored in `upc` converted to ISBN-13.
    if let Err(e) = crate::services::title::convert_isbn10_upc_titles(pool).await {
        tracing::error!(
            error = %e,
            "ISBN-10 stored as UPC: startup conversion failed (non-fatal, continuing)"
        );
    }

    // Story 13-4 (#578): providers' HTML in descriptions to plain text,
    // once per database.
    if let Err(e) = crate::services::description_repair::repair_html_descriptions_once(pool).await {
        tracing::error!(
            error = %e,
            "HTML description: startup conversion failed (non-fatal, continuing)"
        );
    }

    // Story 17-6 (#656): the default MCP skills, offered once per database.
    crate::services::default_skills::insert_defaults(pool).await;

    // Story 19-6 (#657, D4): the instance id, created once per database.
    if let Err(e) = crate::services::instance_id::ensure(pool).await {
        tracing::warn!(error = %e, "Instance id could not be created (non-fatal, continuing)");
    }
}
