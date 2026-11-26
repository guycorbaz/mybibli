//! Story 19-9 (#658, AC3–AC6) — the cover check and its repair through the
//! routes: the check changes nothing; the repair deletes the orphan,
//! follows D4–D7 for the missing covers, queues the live title, leaves
//! every other entry alone, bumps no `version` and writes one audit row;
//! D12 (a folder that looks unmounted), an item changed since the check,
//! D11 (busy) and access.

use crate::common;

use std::path::Path;

use axum::http::StatusCode;
use common::backup::{Folders, names_in};
use common::web::{Browser, get_anonymous, seed_admin, seed_librarian};
use mybibli::AppState;
use mybibli::db::DbPool;
use mybibli::services::cover_consistency::{self as cc, MissingItem};

struct Fixture {
    f: Folders,
    state: AppState,
    app: axum::Router,
    admin: Browser,
}

async fn fixture(pool: &DbPool, admin: &str) -> Fixture {
    let f = Folders::new();
    let mut state = common::mcp::state_with_pool(pool.clone());
    state.covers_dir = f.covers();
    state.backup = f.config();
    seed_admin(pool, admin).await;
    let app = mybibli::routes::build_router_with_mcp(state.clone(), mybibli::mcp::McpConfig::default());
    let admin = Browser::login(&app, admin).await;
    Fixture { f, state, app, admin }
}

/// A title row with what the plan reads; returns its id.
async fn title(
    pool: &DbPool,
    name: &str,
    isbn: Option<&str>,
    cover: Option<&str>,
    hand_set: bool,
    trashed: bool,
) -> u64 {
    let id = sqlx::query(
        "INSERT INTO titles (title, media_type, genre_id, isbn, manually_edited_fields, deleted_at) \
         VALUES (?, 'book', (SELECT MIN(id) FROM genres), ?, ?, IF(?, '2026-11-20 08:00:00', NULL))",
    )
    .bind(name)
    .bind(isbn)
    .bind(hand_set.then_some(r#"["cover_image_url"]"#))
    .bind(trashed)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id();
    if let Some(cover) = cover {
        let url = cover.replace("{id}", &id.to_string());
        sqlx::query("UPDATE titles SET cover_image_url = ?, updated_at = updated_at WHERE id = ?")
            .bind(url)
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
    id
}

async fn pending_cover_proposal(pool: &DbPool, title_id: u64) {
    let key = sqlx::query(
        "INSERT INTO api_keys (label, key_hash, key_prefix, scope) VALUES ('cc', 'h', 'mybibli_pr_x', 'propose')",
    )
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id();
    let p = sqlx::query(
        "INSERT INTO proposals (kind, api_key_id, sources, equivalence_key) VALUES ('cover', ?, '[]', ?)",
    )
    .bind(key)
    .bind(format!("cover:titles/{title_id}:x"))
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id();
    sqlx::query(
        "INSERT INTO proposal_items (proposal_id, target_table, target_id, target_version, proposed_values) \
         VALUES (?, 'titles', ?, 1, '{\"line\":\"cover\"}')",
    )
    .bind(p)
    .bind(title_id)
    .execute(pool)
    .await
    .unwrap();
}

fn write_cover(dir: &Path, id: u64) {
    std::fs::write(dir.join(format!("{id}.jpg")), b"\xff\xd8jpeg").unwrap();
}

/// Every title's cover address, version and `updated_at`.
async fn titles_state(pool: &DbPool) -> Vec<(u64, Option<String>, i32, String, Option<String>)> {
    sqlx::query_as(
        "SELECT CAST(id AS SIGNED) AS id, cover_image_url, version, CAST(CAST(updated_at AS DATETIME) AS CHAR), \
                CAST(manually_edited_fields AS CHAR) FROM titles ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|(id, url, v, at, mef): (i64, Option<String>, i32, String, Option<String>)| (id as u64, url, v, at, mef))
    .collect()
}

async fn cover_of(pool: &DbPool, id: u64) -> Option<String> {
    sqlx::query_scalar("SELECT cover_image_url FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn queued(pool: &DbPool) -> Vec<u64> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT CAST(title_id AS SIGNED) FROM metadata_fetch_retries WHERE deleted_at IS NULL ORDER BY title_id",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    ids.into_iter().map(|i| i as u64).collect()
}

async fn audit_rows(pool: &DbPool) -> Vec<serde_json::Value> {
    let rows: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT CAST(details AS CHAR) FROM admin_audit WHERE action = 'cover_repair' ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    rows.into_iter().map(|d| serde_json::from_str(&d.unwrap()).unwrap()).collect()
}

/// The hidden fields `name="<name>"` of an answer, as a form body.
fn posted_items(html: &str) -> String {
    let mut body = Vec::new();
    for name in ["orphan", "missing"] {
        let needle = format!(r#"name="{name}" value=""#);
        let mut rest = html;
        while let Some(at) = rest.find(&needle) {
            rest = &rest[at + needle.len()..];
            let end = rest.find('"').unwrap();
            let value = rest[..end].replace("&#x2f;", "/").replace("&#47;", "/").replace("&amp;", "&");
            body.push(format!("{name}={}", urlencode(&value)));
            rest = &rest[end..];
        }
    }
    body.join("&")
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The library of AC4: returns (orphan id, live, trash, hand, pending,
/// no code, present).
async fn seed_library(pool: &DbPool, covers: &Path) -> [u64; 7] {
    let live = title(pool, "CC live", Some("9782070000011"), Some("/covers/{id}.jpg?v=1"), false, false).await;
    let trash = title(pool, "CC trash", Some("9782070000028"), Some("/covers/{id}.jpg"), false, true).await;
    let hand = title(pool, "CC hand", Some("9782070000035"), Some("/covers/{id}.jpg?v=2"), true, false).await;
    let pending = title(pool, "CC pending", Some("9782070000042"), Some("/covers/{id}.jpg"), false, false).await;
    pending_cover_proposal(pool, pending).await;
    let no_code = title(pool, "CC no code", None, Some("/covers/{id}.jpg"), false, false).await;
    let present = title(pool, "CC present", None, Some("/covers/{id}.jpg"), false, false).await;
    write_cover(covers, present);
    // Enough present covers that the missing ones stay under half (D12).
    for i in 0..5 {
        let id = title(pool, &format!("CC present {i}"), None, Some("/covers/{id}.jpg?v=3"), false, false).await;
        write_cover(covers, id);
    }
    let orphan = 900_001;
    write_cover(covers, orphan);
    std::fs::create_dir(covers.join(".staging")).unwrap();
    std::fs::write(covers.join(".staging").join("x.jpg"), b"x").unwrap();
    std::fs::write(covers.join("notes.txt"), b"x").unwrap();
    // An own name no title holds, as a link: counted, never an orphan.
    std::os::unix::fs::symlink(covers.join("notes.txt"), covers.join("800000.jpg")).unwrap();
    [orphan, live, trash, hand, pending, no_code, present]
}

#[sqlx::test(migrations = "./migrations")]
async fn the_check_changes_nothing_and_the_repair_follows_d4_to_d7(pool: DbPool) {
    let fx = fixture(&pool, "cc_admin").await;
    let covers = fx.f.covers();
    let [orphan, live, trash, hand, pending, no_code, present] = seed_library(&pool, &covers).await;
    let before = titles_state(&pool).await;
    let names_before = names_in(&covers);

    // AC3 — the check lists, changes nothing, writes no audit row.
    let (status, html) = fx.admin.post_form(&fx.app, "/admin/covers/check", "").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("aucun titre ne possède (1)"), "{html}");
    assert!(html.contains(&format!("{orphan}.jpg · ")), "{html}");
    assert!(html.contains("Titres dont le fichier de couverture manque (5)"), "{html}");
    for (id, name) in [(live, "CC live"), (trash, "CC trash"), (hand, "CC hand"), (pending, "CC pending"), (no_code, "CC no code")] {
        assert!(html.contains(&format!(r#"href="/title/{id}""#)), "{name}: {html}");
    }
    assert!(!html.contains(&format!(r#"href="/title/{present}""#)));
    assert!(html.contains("dans la corbeille"), "{html}");
    assert!(html.contains("choisie à la main : téléversez-la à nouveau"));
    assert!(html.contains("ni ISBN, ni ISSN, ni UPC : rien à télécharger"));
    assert!(html.contains("une proposition de couverture est en attente : acceptez-la pour installer une couverture"));
    assert!(html.contains("Laissés tels quels : 3 autres éléments dans le dossier des couvertures"), "{html}");
    assert!(html.contains(r#"id="admin-covers-repair""#));
    assert_eq!(titles_state(&pool).await, before, "the check changes no row");
    assert_eq!(names_in(&covers), names_before, "the check changes no file");
    assert!(audit_rows(&pool).await.is_empty(), "a check writes no audit row (D10)");

    // D8 — the modal: the counts of what will change.
    let items = posted_items(&html);
    let (status, modal) = fx.admin.post_form(&fx.app, "/admin/covers/repair/confirm", &items).await;
    assert_eq!(status, StatusCode::OK, "{modal}");
    assert!(modal.contains("Réparer les couvertures ?"), "{modal}");
    assert!(
        modal.contains("1 fichiers de couverture") && modal.contains("seront supprimés. 4 titres perdent")
            && modal.contains("couverture manquante ; 1 d"),
        "{modal}"
    );
    assert!(modal.contains(r#"hx-post="/admin/covers/repair""#));
    assert!(!modal.contains(&format!("{pending}:/covers/")), "the pending one is not posted");

    // The repair. Review pass 1: posted twice over, the duplicates are
    // dropped, never counted as changed.
    let twice = format!("{0}&{0}", posted_items(&modal));
    let (status, done) = fx.admin.post_form(&fx.app, "/admin/covers/repair", &twice).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert!(
        done.contains("Couvertures réparées : 1 fichiers supprimés, 4 adresses de couverture effacées, 1 téléchargements mis en file."),
        "{done}"
    );
    assert!(!done.contains("avaient changé depuis le contrôle"), "{done}");
    assert!(done.contains("Titres dont le fichier de couverture manque (1)"), "the check is refreshed: {done}");

    let names = names_in(&covers);
    assert!(!names.contains(&format!("{orphan}.jpg")), "the orphan is gone");
    for kept in ["800000.jpg", "notes.txt", ".staging", &format!("{present}.jpg")] {
        assert!(names.contains(&kept.to_string()), "{kept} is left alone: {names:?}");
    }
    assert!(covers.join(".staging").join("x.jpg").exists());
    assert_eq!(cover_of(&pool, live).await, None);
    assert_eq!(cover_of(&pool, trash).await, None, "D4: cleared in the Trash");
    assert_eq!(cover_of(&pool, hand).await, None, "D5: cleared");
    assert_eq!(cover_of(&pool, no_code).await, None);
    assert_eq!(cover_of(&pool, pending).await.as_deref(), Some(format!("/covers/{pending}.jpg").as_str()), "D7");
    assert_eq!(cover_of(&pool, present).await.as_deref(), Some(format!("/covers/{present}.jpg").as_str()));
    assert_eq!(queued(&pool).await, vec![live], "D6: only the live title with a code, not set by hand");

    let after = titles_state(&pool).await;
    for (b, a) in before.iter().zip(after.iter()) {
        assert_eq!((b.0, b.2, &b.3, &b.4), (a.0, a.2, &a.3, &a.4), "no version / updated_at / mark change");
    }

    let audit = audit_rows(&pool).await;
    assert_eq!(audit.len(), 1, "one audit row per repair");
    let d = &audit[0];
    assert_eq!((d["deleted"].as_u64(), d["cleared"].as_u64(), d["queued"].as_u64(), d["skipped"].as_u64()), (Some(1), Some(4), Some(1), Some(0)));
    assert_eq!((d["failed"].as_u64(), d["not_handled"].as_u64()), (Some(0), Some(0)));
    assert_eq!(d["user_username"], "cc_admin");
    assert_eq!(d["user_role"], "admin");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_item_changed_since_the_check_is_left_alone_and_counted(pool: DbPool) {
    let fx = fixture(&pool, "cc_changed").await;
    let covers = fx.f.covers();
    let [orphan, live, _, _, _, _, _] = seed_library(&pool, &covers).await;
    let (_, html) = fx.admin.post_form(&fx.app, "/admin/covers/check", "").await;
    let items = posted_items(&html);

    // A title now holds the orphan's id; the live title got a new address.
    sqlx::query("INSERT INTO titles (id, title, media_type, genre_id) VALUES (?, 'CC new', 'book', (SELECT MIN(id) FROM genres))")
        .bind(orphan)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE titles SET cover_image_url = ? WHERE id = ?")
        .bind(format!("/covers/{live}.jpg?v=99"))
        .bind(live)
        .execute(&pool)
        .await
        .unwrap();

    let (status, done) = fx.admin.post_form(&fx.app, "/admin/covers/repair", &items).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert!(done.contains("Couvertures réparées : 0 fichiers supprimés, 3 adresses de couverture effacées, 0 téléchargements mis en file."), "{done}");
    assert!(done.contains("2 éléments avaient changé depuis le contrôle et ont été laissés tels quels."), "{done}");
    assert!(covers.join(format!("{orphan}.jpg")).exists(), "the file of a title is kept");
    assert_eq!(cover_of(&pool, live).await, Some(format!("/covers/{live}.jpg?v=99")));
    assert!(queued(&pool).await.is_empty());
    assert_eq!(audit_rows(&pool).await[0]["skipped"].as_u64(), Some(2));
}

#[sqlx::test(migrations = "./migrations")]
async fn d12_an_emptied_folder_offers_no_repair_and_the_post_changes_nothing(pool: DbPool) {
    let fx = fixture(&pool, "cc_unmounted").await;
    let covers = fx.f.covers();
    let mut ids = Vec::new();
    for i in 0..4 {
        let id = title(&pool, &format!("CC mounted {i}"), Some(&format!("978207000{i}000")), Some("/covers/{id}.jpg"), false, false).await;
        write_cover(&covers, id);
        ids.push(id);
    }
    // The volume is not mounted: an empty folder.
    for id in &ids {
        std::fs::remove_file(covers.join(format!("{id}.jpg"))).unwrap();
    }
    let before = titles_state(&pool).await;
    let (status, html) = fx.admin.post_form(&fx.app, "/admin/covers/check", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains(&format!("4 des 4 couvertures utilisées par la bibliothèque manquent dans {}.", covers.display())),
        "{html}"
    );
    assert!(html.contains("Titres dont le fichier de couverture manque (4)"), "the lists are shown: {html}");
    assert!(!html.contains(r#"id="admin-covers-repair""#), "no repair offered: {html}");

    let items: Vec<String> = ids.iter().map(|id| format!("missing={}", urlencode(&format!("{id}:/covers/{id}.jpg")))).collect();
    let items = items.join("&");
    let (status, modal) = fx.admin.post_form(&fx.app, "/admin/covers/repair/confirm", &items).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!modal.contains("Réparer les couvertures ?"), "no modal: {modal}");
    assert!(modal.contains("couvertures utilisées par la bibliothèque manquent"), "the refreshed check instead: {modal}");
    let (status, body) = fx.admin.post_form(&fx.app, "/admin/covers/repair", &items).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("Cela ressemble à un dossier des couvertures non monté"), "{body}");
    assert_eq!(titles_state(&pool).await, before, "nothing changed");
    assert!(queued(&pool).await.is_empty());
    assert!(audit_rows(&pool).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn d11_the_repair_is_refused_while_a_backup_or_restore_runs(pool: DbPool) {
    let fx = fixture(&pool, "cc_busy").await;
    let covers = fx.f.covers();
    let [orphan, ..] = seed_library(&pool, &covers).await;
    let (_, html) = fx.admin.post_form(&fx.app, "/admin/covers/check", "").await;
    let items = posted_items(&html);
    let before = titles_state(&pool).await;
    let held = fx.state.backup.guard.clone().lock_owned().await;

    // The check is never refused (read only).
    let (status, _) = fx.admin.post_form(&fx.app, "/admin/covers/check", "").await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = fx.admin.post_form(&fx.app, "/admin/covers/repair", &items).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("Une sauvegarde ou une restauration est déjà en cours."), "{body}");
    assert!(covers.join(format!("{orphan}.jpg")).exists());
    assert_eq!(titles_state(&pool).await, before);
    assert!(audit_rows(&pool).await.is_empty());
    drop(held);
}

#[sqlx::test(migrations = "./migrations")]
async fn only_an_admin_checks_and_repairs(pool: DbPool) {
    let fx = fixture(&pool, "cc_access").await;
    let covers = fx.f.covers();
    let [orphan, ..] = seed_library(&pool, &covers).await;
    // Review pass 1 (acceptance L3): real items, taken from an admin check.
    let (_, html) = fx.admin.post_form(&fx.app, "/admin/covers/check", "").await;
    let items = posted_items(&html);
    assert!(items.contains(&format!("orphan={orphan}%3A")), "{items}");

    for path in ["/admin/covers/check", "/admin/covers/repair/confirm", "/admin/covers/repair"] {
        let res = get_anonymous(&fx.app, path).await;
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}: POST only");
        let anon = Browser::anonymous(&fx.app).await;
        let res = anon.send(&fx.app, "POST", path, &items, true).await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER, "anonymous {path}");
        let to = res.headers().get("hx-redirect").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        assert!(to.starts_with("/login?next="), "anonymous {path}: {to}");
        assert_eq!(res.headers().get("location").and_then(|v| v.to_str().ok()), Some(to.as_str()));
    }
    seed_librarian(&pool, "cc_lib").await;
    let lib = Browser::login(&fx.app, "cc_lib").await;
    for path in ["/admin/covers/check", "/admin/covers/repair/confirm", "/admin/covers/repair"] {
        let (status, _) = lib.post_form(&fx.app, path, &items).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "librarian {path}");
    }
    let forged = Browser { cookie: fx.admin.cookie.clone(), csrf: "not-the-token".into() };
    let (status, _) = forged.post_form(&fx.app, "/admin/covers/repair", &items).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the CSRF token is required");
    assert!(covers.join(format!("{orphan}.jpg")).exists(), "nothing was deleted");
    assert!(audit_rows(&pool).await.is_empty());

    // The section is on the Backups tab.
    let (status, page) = fx.admin.get(&fx.app, "/admin?tab=backups").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains(r#"id="admin-covers-check""#), "{page}");
    assert!(page.contains("Contrôle des couvertures"), "{page}");
}

// ─── Review pass 1 ───────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn an_unreadable_folder_is_reported_and_nothing_changes(pool: DbPool) {
    use std::os::unix::fs::PermissionsExt;
    let fx = fixture(&pool, "cc_unreadable").await;
    let covers = fx.f.covers();
    let [orphan, ..] = seed_library(&pool, &covers).await;
    let (_, html) = fx.admin.post_form(&fx.app, "/admin/covers/check", "").await;
    let items = posted_items(&html);
    let before = titles_state(&pool).await;
    std::fs::set_permissions(&covers, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable = std::fs::read_dir(&covers).is_ok();
    let expected = format!("mybibli ne peut pas lire le dossier des couvertures {}", covers.display());

    let (check_status, check) = fx.admin.post_form(&fx.app, "/admin/covers/check", "").await;
    let (modal_status, modal) = fx.admin.post_form(&fx.app, "/admin/covers/repair/confirm", &items).await;
    let (repair_status, repair) = fx.admin.post_form(&fx.app, "/admin/covers/repair", &items).await;
    std::fs::set_permissions(&covers, std::fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        // Running as root: permissions do not stop a read; nothing to test.
        return;
    }
    assert_eq!(check_status, StatusCode::OK);
    assert!(check.contains(&expected), "{check}");
    assert!(check.contains(r#"id="admin-covers-status" hx-swap-oob="innerHTML""#), "{check}");
    assert_eq!(modal_status, StatusCode::OK);
    assert!(modal.contains(&expected) && !modal.contains("Réparer les couvertures ?"), "{modal}");
    assert_eq!(repair_status, StatusCode::CONFLICT, "{repair}");
    assert!(repair.contains(&expected), "{repair}");
    assert!(covers.join(format!("{orphan}.jpg")).exists());
    assert_eq!(titles_state(&pool).await, before);
    assert!(audit_rows(&pool).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_forged_orphan_with_another_mtime_is_left_alone(pool: DbPool) {
    let fx = fixture(&pool, "cc_forged").await;
    let covers = fx.f.covers();
    let [orphan, ..] = seed_library(&pool, &covers).await;
    let forged = format!("orphan={}", urlencode(&format!("{orphan}:6:12345")));
    let (status, done) = fx.admin.post_form(&fx.app, "/admin/covers/repair", &forged).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert!(done.contains("Couvertures réparées : 0 fichiers supprimés"), "{done}");
    assert!(done.contains("1 éléments avaient changé"), "{done}");
    assert!(covers.join(format!("{orphan}.jpg")).exists(), "the file is kept");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_put_back_address_takes_its_lookup_off_the_queue(pool: DbPool) {
    let id = title(&pool, "CC put back", Some("9782070000059"), Some("/covers/{id}.jpg?v=4"), false, false).await;
    let item = MissingItem { id, url: format!("/covers/{id}.jpg?v=4") };
    assert!(cc::clear_address(&pool, &item, true).await.unwrap());
    assert_eq!(cover_of(&pool, id).await, None);
    assert_eq!(queued(&pool).await, vec![id]);
    cc::restore_address(&pool, &item, true).await.unwrap();
    assert_eq!(cover_of(&pool, id).await, Some(item.url.clone()), "the address is back");
    assert!(queued(&pool).await.is_empty(), "the lookup the clear queued is dequeued");

    // An address written meanwhile is never overwritten.
    assert!(cc::clear_address(&pool, &item, false).await.unwrap());
    sqlx::query("UPDATE titles SET cover_image_url = ? WHERE id = ?")
        .bind(format!("/covers/{id}.jpg?v=9"))
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    cc::restore_address(&pool, &item, false).await.unwrap();
    assert_eq!(cover_of(&pool, id).await, Some(format!("/covers/{id}.jpg?v=9")));
    assert!(!cc::clear_address(&pool, &item, false).await.unwrap(), "a changed address is not cleared");
}

#[sqlx::test(migrations = "./migrations")]
async fn owned_ids_reads_rows_and_addresses_in_one_chunk(pool: DbPool) {
    let a = title(&pool, "CC owned", None, None, false, true).await;
    title(&pool, "CC absolute", None, Some("https://nas.local/covers/700001.jpg#x"), false, false).await;
    title(&pool, "CC busted", None, Some("/covers/700002.jpg?v=1"), false, false).await;
    let mut ids: Vec<u64> = (800_000..800_000 + cc::OWNED_CHUNK as u64 - 3).collect();
    ids.extend([a, 700_001, 700_002]);
    let owned = cc::owned_ids(&pool, &ids).await.unwrap();
    assert_eq!(owned, std::collections::HashSet::from([a, 700_001, 700_002]), "a Trash row and named files");
    assert!(cc::owned_ids(&pool, &[]).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_cover_proposal_is_read_from_the_target_index(pool: DbPool) {
    let id = title(&pool, "CC proposal", None, None, false, false).await;
    assert!(!cc::has_pending_cover_proposal(&pool, id).await.unwrap());
    pending_cover_proposal(&pool, id).await;
    assert!(cc::has_pending_cover_proposal(&pool, id).await.unwrap());
    sqlx::query("UPDATE proposals SET status = 'rejected'").execute(&pool).await.unwrap();
    assert!(!cc::has_pending_cover_proposal(&pool, id).await.unwrap(), "a decided proposal does not count");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_error_midway_still_writes_the_audit_row_with_the_counts(pool: DbPool) {
    let fx = fixture(&pool, "cc_midway").await;
    let covers = fx.f.covers();
    let [orphan, live, ..] = seed_library(&pool, &covers).await;
    let (_, html) = fx.admin.post_form(&fx.app, "/admin/covers/check", "").await;
    let items = posted_items(&html);
    // The queue is gone: the live title's clear (which queues) fails.
    sqlx::query("RENAME TABLE metadata_fetch_retries TO metadata_fetch_retries_gone")
        .execute(&pool)
        .await
        .unwrap();
    let (status, _) = fx.admin.post_form(&fx.app, "/admin/covers/repair", &items).await;
    assert!(status.is_server_error(), "{status}");
    assert!(!covers.join(format!("{orphan}.jpg")).exists(), "the orphan went first");
    assert_eq!(
        cover_of(&pool, live).await,
        Some(format!("/covers/{live}.jpg?v=1")),
        "the failed clear rolled back"
    );
    let audit = audit_rows(&pool).await;
    assert_eq!(audit.len(), 1, "one row, even on an error");
    assert_eq!(audit[0]["deleted"].as_u64(), Some(1));
    assert!(audit[0]["error"].as_str().is_some_and(|e| !e.is_empty()), "{:?}", audit[0]);
    assert_eq!(audit[0]["user_username"], "cc_midway");
}

// ─── Review pass 1, approved copy ────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_deletion_refused_by_the_folder_is_counted_as_failed(pool: DbPool) {
    use std::os::unix::fs::PermissionsExt;
    let fx = fixture(&pool, "cc_failed").await;
    let covers = fx.f.covers();
    let [orphan, ..] = seed_library(&pool, &covers).await;
    let (_, html) = fx.admin.post_form(&fx.app, "/admin/covers/check", "").await;
    let items = posted_items(&html);
    // Readable, not writable: the listing works, `remove_file` is refused.
    std::fs::set_permissions(&covers, std::fs::Permissions::from_mode(0o555)).unwrap();
    let writable = std::fs::write(covers.join("probe"), b"x").is_ok();
    let (status, done) = fx.admin.post_form(&fx.app, "/admin/covers/repair", &items).await;
    std::fs::set_permissions(&covers, std::fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        return; // root: permissions do not stop a write
    }
    assert_eq!(status, StatusCode::OK, "{done}");
    assert!(done.contains("Couvertures réparées : 0 fichiers supprimés, 4 adresses de couverture effacées"), "{done}");
    assert!(
        done.contains(&format!(
            "1 fichiers n&#x27;ont pas pu être supprimés : mybibli ne peut pas écrire dans le dossier des couvertures {}.",
            covers.display()
        )),
        "{done}"
    );
    assert!(!done.contains("avaient changé depuis le contrôle"), "a failure is not a change: {done}");
    assert!(covers.join(format!("{orphan}.jpg")).exists());
    let audit = audit_rows(&pool).await;
    assert_eq!((audit[0]["failed"].as_u64(), audit[0]["skipped"].as_u64()), (Some(1), Some(0)));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_repair_stopped_by_a_restore_counts_what_it_did_not_handle(pool: DbPool) {
    let fx = fixture(&pool, "cc_stopped").await;
    let covers = fx.f.covers();
    let [orphan, live, ..] = seed_library(&pool, &covers).await;
    let plan = cc::check(&pool, &covers).await.unwrap();
    let items = cc::RepairItems::of_plan(&plan);
    assert_eq!(items.len(), 5);
    let admin_id: i64 = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM users WHERE username = 'cc_stopped'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let actor = cc::Actor { user_id: admin_id as u64, username: "cc_stopped".into(), role: "admin".into() };
    // The repair's scope is captured before a restore starts (and ends):
    // every write permit is then refused.
    let m = fx.state.backup.maintenance.clone();
    let pool2 = pool.clone();
    let backup = fx.state.backup.clone();
    let run = mybibli::services::maintenance::scoped(&m, async move {
        cc::repair(&pool2, &backup, items, actor).await
    });
    drop(m.begin(std::time::Duration::from_secs(1)).await.unwrap());
    let done = run.await.unwrap();
    assert_eq!(
        (done.deleted, done.cleared, done.skipped, done.failed, done.not_handled),
        (0, 0, 0, 0, 5)
    );
    assert!(covers.join(format!("{orphan}.jpg")).exists());
    assert_eq!(cover_of(&pool, live).await, Some(format!("/covers/{live}.jpg?v=1")));
    let audit = audit_rows(&pool).await;
    assert_eq!(audit[0]["not_handled"].as_u64(), Some(5));
    assert_eq!(
        mybibli::routes::admin_covers::done_text(&done, "/app/covers", "en"),
        "Covers repaired: 0 files deleted, 0 cover addresses cleared, 0 downloads queued. The repair stopped because a restore started; 5 items were not handled. Check again once the restore is over."
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn more_orphans_than_used_covers_is_refused_like_another_database(pool: DbPool) {
    let fx = fixture(&pool, "cc_other_db").await;
    let covers = fx.f.covers();
    let used = title(&pool, "CC used", None, Some("/covers/{id}.jpg"), false, false).await;
    write_cover(&covers, used);
    for orphan in [900_101, 900_102] {
        write_cover(&covers, orphan);
    }
    let (status, html) = fx.admin.post_form(&fx.app, "/admin/covers/check", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("appartiennent à aucun titre, plus que les 1 couvertures utilisées par la bibliothèque."),
        "{html}"
    );
    assert!(html.contains("aucun titre ne possède (2)"), "the list is shown: {html}");
    assert!(!html.contains(r#"id="admin-covers-repair""#), "no repair offered: {html}");

    let items = format!(
        "orphan={}&orphan={}",
        urlencode(&cc::OrphanItem::of(&cc::list_folder(&covers).unwrap().files[1]).encode()),
        urlencode(&cc::OrphanItem::of(&cc::list_folder(&covers).unwrap().files[2]).encode()),
    );
    let (status, modal) = fx.admin.post_form(&fx.app, "/admin/covers/repair/confirm", &items).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!modal.contains("Réparer les couvertures ?"), "no modal: {modal}");
    let (status, body) = fx.admin.post_form(&fx.app, "/admin/covers/repair", &items).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("mybibli ne supprimera donc rien"), "{body}");
    for orphan in [900_101, 900_102] {
        assert!(covers.join(format!("{orphan}.jpg")).exists(), "nothing deleted");
    }
    assert!(audit_rows(&pool).await.is_empty());
}
