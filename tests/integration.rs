//! The one integration-test binary (#717).
//!
//! Every `tests/*.rs` file is a module of this crate instead of a crate of
//! its own (`autotests = false` in `Cargo.toml`): the library and its
//! dependencies are linked once instead of once per file, which was the
//! memory peak of a full `cargo test`. A new test file must be declared
//! below — `every_test_file_is_declared` fails otherwise, so a forgotten
//! file is never silently skipped (#357). Shared helpers stay in `common/`
//! (`use crate::common;` in a test file). Run one file's tests with
//! `cargo test --test integration <file_stem>::`.

mod common;

mod admin_api_keys_delete;
mod admin_audit_fk_set_null;
mod admin_refdata_hx_trigger;
mod admin_system_integration;
mod admin_trash_modal_lifecycle;
mod admin_trash_restore;
mod admin_user_deactivate_modal;
mod api_key_propose_scope;
mod api_key_revocation_lock_scope;
mod api_v1_auth;
mod api_v1_patch_titles;
mod api_v1_series;
mod audit_list_non_empty;
mod auth_service;
mod backup_round_trip;
mod backup_write;
mod borrower_delete_modal;
mod connection_lost_overlay;
mod contextual_help_tooltips;
mod contributor_delete_modal;
mod contributor_live_name;
mod cookieless_requests;
mod cover_manual_survives_refetch;
mod cover_uploads;
mod csrf_integration;
mod dashboard_gaps;
mod dashboard_glance;
mod dashboard_overdue;
mod dashboard_recent_activity;
mod dashboard_recent_additions;
mod dashboard_stats_by_genre;
mod dashboard_unshelved;
mod date_added;
mod default_skills;
mod dewey_preserve_on_refetch;
mod expected_tomes;
mod expected_tomes_web;
mod find_by_location_dewey;
mod find_similar;
mod form_refusals;
mod home_scan_redirect;
mod html_description_cleanup;
mod html_description_proposals;
mod isbn_one_rule;
mod keyboard_shortcuts_cheat_sheet;
mod labels_crud;
mod locale_html_safety;
mod locale_parity;
mod location_delete_modal;
mod location_description;
mod mcp_catalog_read_tools;
mod mcp_cover_proposals;
mod mcp_endpoint;
mod mcp_interop;
mod mcp_label_proposals;
mod mcp_metadata_proposals;
mod mcp_series_gaps_tools;
mod mcp_series_proposals;
mod mcp_skills;
mod mcp_skills_served;
mod metadata_cache_force_refresh;
mod metadata_fetch_dewey;
mod metadata_fetch_race;
mod metadata_retry;
mod navbar_hamburger;
mod navbar_role_visibility;
mod proposal_accept_metadata_cover;
mod proposal_accept_series;
mod proposal_edit;
mod proposal_history;
mod proposal_series_update;
mod proposal_volume_value;
mod proposals_access;
mod proposals_page;
mod proposals_service;
mod reset_admin;
mod return_loan_modal;
mod role_gating;
mod saved_searches_integration;
mod scan_existing_label;
mod scan_irregular_code;
mod scan_title_context;
mod scan_undo_integration;
mod scan_vcode_case;
mod search_filter_browse;
mod search_isbn10;
mod seed_drift;
mod seed_gate;
mod seeded_users;
mod series_delete_modal;
mod series_empty_state;
mod series_list_all_names;
mod series_list_counts;
mod session_cookie_collision;
mod setup_wizard;
mod system_user;
mod tailwind_sources;
mod title_default_genre;
mod title_id_search;
mod title_volume_add;
mod tome_number;
mod toolchain_pins;
mod typed_upc_warning;
mod unimarc_mapping;
mod upc_checksum_guard;
mod upc_isbn10_conversion;
mod users_in_allowed_tables;
mod value_coverage;
mod volume_add_races;
mod volume_detail_loan_status;
mod volume_label_in_trash;
mod volume_move;

/// Every `tests/*.rs` file is declared above as a module (#357, #717).
#[test]
fn every_test_file_is_declared() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let root = std::fs::read_to_string(dir.join("integration.rs")).expect("tests/integration.rs");
    let declared: std::collections::BTreeSet<&str> = root
        .lines()
        .filter_map(|l| l.strip_prefix("mod ")?.strip_suffix(';'))
        .collect();
    let mut missing = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("tests/") {
        let path = entry.expect("tests/ entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            let stem = path.file_stem().and_then(|s| s.to_str()).expect("UTF-8 name");
            if stem != "integration" && !declared.contains(stem) {
                missing.push(stem.to_string());
            }
        }
    }
    assert!(
        missing.is_empty(),
        "tests/*.rs files not declared in tests/integration.rs (they would never run): {missing:?}"
    );
}
