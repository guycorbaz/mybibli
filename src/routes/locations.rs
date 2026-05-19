use std::collections::HashMap;

use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, Path, State};
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::location::LocationModel;
use crate::services::locations::LocationService;
use crate::utils::current_url;

use crate::models::PaginatedList;
use crate::models::volume::{VolumeModel, VolumeWithTitle};

#[derive(Deserialize)]
pub struct LocationDetailQuery {
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default = "default_page")]
    pub page: u32,
}

fn default_page() -> u32 {
    1
}

#[derive(Template)]
#[template(path = "pages/location_detail.html")]
pub struct LocationDetailTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub connection_status: crate::utils::ConnectionStatusContext,
    pub shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext,
    pub session_timeout_secs: u64,
    pub csrf_token: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_wishlist: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub nav_menu_open: String,
    pub location: LocationModel,
    pub breadcrumb_segments: Vec<(u64, String)>,
    pub volumes: PaginatedList<VolumeWithTitle>,
    pub contents_title: String,
    pub empty_volumes: String,
    pub col_title: String,
    pub col_author: String,
    pub col_genre: String,
    pub col_dewey: String,
    pub col_condition: String,
    pub col_status: String,
    pub prev_label: String,
    pub next_label: String,
    pub pagination_aria: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
}

pub async fn location_detail(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(_is_htmx): HxRequest,
    Path(id): Path<u64>,
    axum::extract::Query(params): axum::extract::Query<LocationDetailQuery>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;
    let loc = locale.0;

    let location = LocationModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    let breadcrumb_segments = LocationModel::get_path_segments(pool, location.id).await?;
    let volumes =
        VolumeModel::find_by_location(pool, id, &params.sort, &params.dir, params.page).await?;

    let template = LocationDetailTemplate {
        lang: loc.to_string(),
        role: session.role.to_string(),
        current_page: "location",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        connection_status: crate::utils::ConnectionStatusContext::new(loc),
        shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new(loc),
        session_timeout_secs: state.session_timeout_secs(),
        csrf_token: session.csrf_token.clone(),
        nav_catalog: rust_i18n::t!("nav.catalog", locale = loc).to_string(),
        nav_loans: rust_i18n::t!("nav.loans", locale = loc).to_string(),
        nav_wishlist: rust_i18n::t!("nav.wishlist", locale = loc).to_string(),
        nav_locations: rust_i18n::t!("nav.locations", locale = loc).to_string(),
        nav_series: rust_i18n::t!("nav.series", locale = loc).to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers", locale = loc).to_string(),
        nav_admin: rust_i18n::t!("nav.admin", locale = loc).to_string(),
        nav_login: rust_i18n::t!("nav.login", locale = loc).to_string(),
        nav_logout: rust_i18n::t!("nav.logout", locale = loc).to_string(),
        nav_menu_open: rust_i18n::t!("nav.menu_open", locale = loc).to_string(),
        contents_title: rust_i18n::t!("location.contents_title", locale = loc).to_string(),
        empty_volumes: rust_i18n::t!("location.empty_volumes", locale = loc).to_string(),
        col_title: rust_i18n::t!("location.col_title", locale = loc).to_string(),
        col_author: rust_i18n::t!("location.col_author", locale = loc).to_string(),
        col_genre: rust_i18n::t!("location.col_genre", locale = loc).to_string(),
        col_dewey: rust_i18n::t!("location.col_dewey", locale = loc).to_string(),
        col_condition: rust_i18n::t!("location.col_condition", locale = loc).to_string(),
        col_status: rust_i18n::t!("location.col_status", locale = loc).to_string(),
        prev_label: rust_i18n::t!("pagination.previous", locale = loc).to_string(),
        next_label: rust_i18n::t!("pagination.next", locale = loc).to_string(),
        pagination_aria: rust_i18n::t!("pagination.aria_label", locale = loc).to_string(),
        location,
        breadcrumb_segments,
        volumes,
        current_url: current_url(&uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = loc).to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render location detail template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

// ─── Tree data structure ─────────────────────────────────────────

/// A location node in the tree with computed children and volume count.
pub struct TreeNode {
    pub location: LocationModel,
    pub children: Vec<TreeNode>,
    pub volume_count: u64,
}

fn build_tree(locations: &[LocationModel], volume_counts: &HashMap<u64, u64>) -> Vec<TreeNode> {
    let mut children_map: HashMap<Option<u64>, Vec<&LocationModel>> = HashMap::new();
    for loc in locations {
        children_map.entry(loc.parent_id).or_default().push(loc);
    }
    build_subtree(None, &children_map, volume_counts)
}

fn build_subtree(
    parent_id: Option<u64>,
    children_map: &HashMap<Option<u64>, Vec<&LocationModel>>,
    volume_counts: &HashMap<u64, u64>,
) -> Vec<TreeNode> {
    let Some(children) = children_map.get(&parent_id) else {
        return Vec::new();
    };
    children
        .iter()
        .map(|loc| TreeNode {
            children: build_subtree(Some(loc.id), children_map, volume_counts),
            volume_count: *volume_counts.get(&loc.id).unwrap_or(&0),
            location: (*loc).clone(),
        })
        .collect()
}

/// Render the tree as HTML string (avoids recursive template which crashes Askama compiler).
///
/// `csrf_token` is the per-session synchronizer token (see CLAUDE.md → CSRF
/// synchronizer token / story 8-2). It is injected as a hidden input on every
/// inline "add child" form. Forgetting it causes issue #185: the middleware
/// rejects the POST, redirects authenticated users to `/login` which itself
/// redirects to `/` — the action silently fails with no UI feedback.
fn render_tree_html(
    nodes: &[TreeNode],
    node_types: &[(u64, String)],
    next_lcode: &str,
    can_edit: bool,
    loc: &str,
    csrf_token: &str,
) -> String {
    let mut html = String::new();
    for node in nodes {
        render_node_html(node, &mut html, node_types, next_lcode, can_edit, loc, csrf_token);
    }
    html
}

fn render_node_html(
    node: &TreeNode,
    html: &mut String,
    node_types: &[(u64, String)],
    next_lcode: &str,
    can_edit: bool,
    loc: &str,
    csrf_token: &str,
) {
    render_node_at_depth(node, html, node_types, next_lcode, 0, can_edit, loc, csrf_token);
}

#[allow(clippy::too_many_arguments)]
fn render_node_at_depth(
    node: &TreeNode,
    html: &mut String,
    node_types: &[(u64, String)],
    next_lcode: &str,
    depth: usize,
    can_edit: bool,
    loc: &str,
    csrf_token: &str,
) {
    let name = crate::utils::html_escape(&node.location.name);
    let label = crate::utils::html_escape(&node.location.label);
    let node_type = crate::utils::html_escape(&node.location.node_type);
    let icon = if node.children.is_empty() {
        "📍"
    } else {
        "📁"
    };
    let vol = if node.volume_count > 0 {
        format!(
            r#" <span class="text-xs text-indigo-600 dark:text-indigo-400">{} vol</span>"#,
            node.volume_count
        )
    } else {
        String::new()
    };

    // Build type options
    let mut type_options = String::new();
    for (_, nt_name) in node_types {
        let nt_escaped = crate::utils::html_escape(nt_name);
        type_options.push_str(&format!(
            r#"<option value="{nt_escaped}">{nt_escaped}</option>"#
        ));
    }
    let name_lbl =
        crate::utils::html_escape(rust_i18n::t!("location.name_label", locale = loc).as_ref());
    let type_lbl =
        crate::utils::html_escape(rust_i18n::t!("location.type_label", locale = loc).as_ref());
    let lcode_lbl =
        crate::utils::html_escape(rust_i18n::t!("location.lcode_label", locale = loc).as_ref());
    let submit_lbl =
        crate::utils::html_escape(rust_i18n::t!("location.submit", locale = loc).as_ref());
    let form_id = format!("add-child-{}", node.location.id);

    // Indentation classes (defined in static/css/browse.css). Pre-CSP this
    // was an inline `style="padding-left: …px"` / `margin-left: …px`,
    // blocked by strict `style-src 'self'`. Levels deeper than 8 fall back
    // to a single capped value (the tree never realistically reaches that).
    let indent_class = if depth >= 8 {
        "tree-indent-cap".to_string()
    } else {
        format!("tree-indent-{depth}")
    };
    let child_depth = depth + 1;
    let child_margin_class = if child_depth >= 8 {
        "tree-margin-cap".to_string()
    } else {
        format!("tree-margin-{child_depth}")
    };

    let (mutation_controls, child_form) = if can_edit {
        (
            format!(
                r#"<span class="ml-auto flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
<button type="button" data-locations-toggle="{form_id}" class="p-1 text-stone-400 hover:text-green-600 dark:hover:text-green-400" aria-label="Add child under {name}">➕</button>
<a href="/locations/{id}/edit" class="p-1 text-stone-400 hover:text-indigo-600 dark:hover:text-indigo-400" aria-label="Edit {name}">✏️</a>
<button type="button" hx-delete="/locations/{id}" hx-confirm="Delete {name} ({label})?" hx-target="closest [role=treeitem]" hx-swap="outerHTML" class="p-1 text-stone-400 hover:text-red-600 dark:hover:text-red-400" aria-label="Delete {name}">🗑️</button>
</span>"#,
                id = node.location.id,
            ),
            format!(
                r#"<form id="{form_id}" method="POST" action="/locations" class="hidden {child_margin_class} px-3 py-2 space-y-2 bg-stone-50 dark:bg-stone-800/50 rounded-md mt-1 mb-2">
<input type="hidden" name="_csrf_token" value="{csrf_token_escaped}">
<input type="hidden" name="parent_id" value="{id}">
<div class="grid grid-cols-1 md:grid-cols-3 gap-2">
<div><label class="block text-xs text-stone-600 dark:text-stone-400">{name_lbl}</label><input type="text" name="name" required class="w-full px-2 py-1 text-sm border border-stone-300 dark:border-stone-600 rounded bg-white dark:bg-stone-800 text-stone-900 dark:text-stone-100"></div>
<div><label class="block text-xs text-stone-600 dark:text-stone-400">{type_lbl}</label><select name="node_type" required class="w-full px-2 py-1 text-sm border border-stone-300 dark:border-stone-600 rounded bg-white dark:bg-stone-800 text-stone-900 dark:text-stone-100">{type_options}</select></div>
<div><label class="block text-xs text-stone-600 dark:text-stone-400">{lcode_lbl}</label><input type="text" name="label" value="{next_lcode}" required maxlength="5" pattern="L[0-9]{{4}}" class="w-full px-2 py-1 text-sm font-mono border border-stone-300 dark:border-stone-600 rounded bg-white dark:bg-stone-800 text-stone-900 dark:text-stone-100"></div>
</div>
<button type="submit" class="px-3 py-1 text-xs font-medium text-white bg-indigo-600 hover:bg-indigo-700 rounded">{submit_lbl}</button>
</form>"#,
                id = node.location.id,
                next_lcode = crate::utils::html_escape(next_lcode),
                csrf_token_escaped = crate::utils::html_escape(csrf_token),
            ),
        )
    } else {
        (String::new(), String::new())
    };

    // Issue #208: the name/label/type/volume-count span chain becomes
    // an `<a>` linking to /location/:id, so a user clicking a tree row
    // navigates to the per-location volume list (location_detail handler
    // at L73 + templates/pages/location_detail.html). mutation_controls
    // (edit / delete / add-child buttons, when can_edit) stay OUTSIDE
    // the anchor so they remain individually focusable + clickable.
    //
    // Issue #200: rows that have at least one child get a fold/unfold
    // toggle in front of the row, and the children are wrapped in a
    // `<div class="tree-children" id="tree-children-{id}">` container
    // (instead of being rendered as flat siblings). Both pieces work
    // together: `static/js/locations-tree-toggle.js` hides the container
    // on click and persists the collapsed-node-id set in localStorage so
    // the layout survives a reload. Indentation classes still drive the
    // visual depth, so wrapping doesn't change the look when open.
    let has_children = !node.children.is_empty();
    let toggle_html = if has_children {
        format!(
            r#"<button type="button" class="tree-toggle p-1 text-stone-400 hover:text-stone-600 dark:hover:text-stone-200" data-tree-target="tree-children-{id}" aria-expanded="true" aria-label="{toggle_lbl}"><span class="tree-toggle-icon" aria-hidden="true">▼</span></button>"#,
            id = node.location.id,
            toggle_lbl = crate::utils::html_escape(
                rust_i18n::t!("location.tree.toggle", locale = loc).as_ref()
            ),
        )
    } else {
        // Spacer keeps row labels aligned across the column whether or
        // not a row has children. `w-6` matches the toggle button's
        // approximate width (icon + padding).
        r#"<span class="w-6" aria-hidden="true"></span>"#.to_string()
    };

    html.push_str(&format!(
        r#"<div role="treeitem" class="{indent_class}" data-tree-node-id="{id}">
<div class="flex items-center gap-2 px-3 py-2 rounded-md hover:bg-stone-100 dark:hover:bg-stone-800 group">
{toggle_html}
<a href="/location/{id}" class="flex items-center gap-2 flex-1 min-w-0 no-underline">
<span class="text-stone-400" aria-hidden="true">{icon}</span>
<span class="font-medium text-stone-900 dark:text-stone-100">{name}</span>
<span class="text-xs text-stone-400 font-mono">{label}</span>
<span class="text-xs text-stone-500 dark:text-stone-400">({node_type})</span>{vol}
</a>
{mutation_controls}
</div>
{child_form}"#,
        id = node.location.id,
    ));

    if has_children {
        html.push_str(&format!(
            r#"<div class="tree-children" id="tree-children-{id}">"#,
            id = node.location.id,
        ));
        for child in &node.children {
            render_node_at_depth(
                child,
                html,
                node_types,
                next_lcode,
                depth + 1,
                can_edit,
                loc,
                csrf_token,
            );
        }
        html.push_str("</div>");
    }

    html.push_str("</div>");
}

// ─── Location tree page ──────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/locations.html")]
pub struct LocationsTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub connection_status: crate::utils::ConnectionStatusContext,
    pub shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext,
    pub session_timeout_secs: u64,
    pub csrf_token: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_wishlist: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub nav_menu_open: String,
    pub tree_title: String,
    pub tree_html: String,
    pub node_types: Vec<(u64, String)>,
    pub next_lcode: String,
    pub empty_state: String,
    pub add_root_label: String,
    pub name_label: String,
    pub type_label: String,
    pub lcode_label: String,
    pub submit_label: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
}

pub async fn locations_page(
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(_is_htmx): HxRequest,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    // AC #1: location tree browser is Anonymous-accessible. Template gates mutation affordances.
    let pool = &state.pool;
    let loc = locale.0;
    let locations = LocationModel::find_all_tree(pool).await?;
    let node_types = LocationModel::find_node_types(pool).await?;
    let next_lcode = LocationService::get_next_available_lcode(pool).await?;

    // Get volume counts for each location
    let mut volume_counts = HashMap::new();
    for location_row in &locations {
        let count = LocationService::get_recursive_volume_count(pool, location_row.id)
            .await
            .unwrap_or(0);
        volume_counts.insert(location_row.id, count);
    }

    let tree = build_tree(&locations, &volume_counts);
    let can_edit = session.role >= Role::Librarian;
    let tree_html = render_tree_html(
        &tree,
        &node_types,
        &next_lcode,
        can_edit,
        loc,
        &session.csrf_token,
    );

    let template = LocationsTemplate {
        lang: loc.to_string(),
        role: session.role.to_string(),
        current_page: "locations",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        connection_status: crate::utils::ConnectionStatusContext::new(loc),
        shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new(loc),
        session_timeout_secs: state.session_timeout_secs(),
        csrf_token: session.csrf_token.clone(),
        nav_catalog: rust_i18n::t!("nav.catalog", locale = loc).to_string(),
        nav_loans: rust_i18n::t!("nav.loans", locale = loc).to_string(),
        nav_wishlist: rust_i18n::t!("nav.wishlist", locale = loc).to_string(),
        nav_locations: rust_i18n::t!("nav.locations", locale = loc).to_string(),
        nav_series: rust_i18n::t!("nav.series", locale = loc).to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers", locale = loc).to_string(),
        nav_admin: rust_i18n::t!("nav.admin", locale = loc).to_string(),
        nav_login: rust_i18n::t!("nav.login", locale = loc).to_string(),
        nav_logout: rust_i18n::t!("nav.logout", locale = loc).to_string(),
        nav_menu_open: rust_i18n::t!("nav.menu_open", locale = loc).to_string(),
        tree_title: rust_i18n::t!("location.tree_title", locale = loc).to_string(),
        tree_html,
        node_types,
        next_lcode,
        empty_state: rust_i18n::t!("location.empty_state", locale = loc).to_string(),
        add_root_label: rust_i18n::t!("location.add_root", locale = loc).to_string(),
        name_label: rust_i18n::t!("location.name_label", locale = loc).to_string(),
        type_label: rust_i18n::t!("location.type_label", locale = loc).to_string(),
        lcode_label: rust_i18n::t!("location.lcode_label", locale = loc).to_string(),
        submit_label: rust_i18n::t!("location.submit", locale = loc).to_string(),
        current_url: current_url(&uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = loc).to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render locations template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

// ─── Create location ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateLocationForm {
    pub name: String,
    pub node_type: String,
    pub label: String,
    #[serde(default)]
    pub parent_id: Option<u64>,
}

pub async fn create_location(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<CreateLocationForm>,
) -> Result<impl IntoResponse, AppError> {
    // Story 7-1 decision 1a: location creation promoted from Admin → Librarian.
    session.require_role(Role::Librarian, locale.0)?;

    let pool = &state.pool;
    let location = LocationService::create_location(
        pool,
        &form.name,
        &form.node_type,
        form.parent_id,
        &form.label,
    )
    .await?;

    tracing::info!(name = %location.name, label = %location.label, "Location created via form");

    // Standard HTTP redirect back to locations page
    Ok(axum::response::Redirect::to("/locations").into_response())
}

// ─── Edit location ───────────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/location_edit.html")]
pub struct LocationEditTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub connection_status: crate::utils::ConnectionStatusContext,
    pub shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext,
    pub session_timeout_secs: u64,
    pub csrf_token: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_wishlist: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub nav_menu_open: String,
    pub edit_title: String,
    pub location: LocationModel,
    pub version: i32,
    pub node_types: Vec<(u64, String)>,
    pub all_locations: Vec<LocationModel>,
    pub name_label: String,
    pub type_label: String,
    pub parent_label: String,
    pub submit_label: String,
    pub none_label: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
}

pub async fn edit_location_page(
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(_is_htmx): HxRequest,
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    // Story 7-1 decision 1a: Admin → Librarian.
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;

    let pool = &state.pool;
    let loc = locale.0;
    let location = LocationModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;
    let version = LocationModel::get_version(pool, id).await?;
    let node_types = LocationModel::find_node_types(pool).await?;
    let all_locations = LocationModel::find_all_tree(pool).await?;

    let template = LocationEditTemplate {
        lang: loc.to_string(),
        role: session.role.to_string(),
        current_page: "locations",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        connection_status: crate::utils::ConnectionStatusContext::new(loc),
        shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new(loc),
        session_timeout_secs: state.session_timeout_secs(),
        csrf_token: session.csrf_token.clone(),
        nav_catalog: rust_i18n::t!("nav.catalog", locale = loc).to_string(),
        nav_loans: rust_i18n::t!("nav.loans", locale = loc).to_string(),
        nav_wishlist: rust_i18n::t!("nav.wishlist", locale = loc).to_string(),
        nav_locations: rust_i18n::t!("nav.locations", locale = loc).to_string(),
        nav_series: rust_i18n::t!("nav.series", locale = loc).to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers", locale = loc).to_string(),
        nav_admin: rust_i18n::t!("nav.admin", locale = loc).to_string(),
        nav_login: rust_i18n::t!("nav.login", locale = loc).to_string(),
        nav_logout: rust_i18n::t!("nav.logout", locale = loc).to_string(),
        nav_menu_open: rust_i18n::t!("nav.menu_open", locale = loc).to_string(),
        edit_title: rust_i18n::t!("location.edit", locale = loc).to_string(),
        location,
        version,
        node_types,
        all_locations,
        name_label: rust_i18n::t!("location.name_label", locale = loc).to_string(),
        type_label: rust_i18n::t!("location.type_label", locale = loc).to_string(),
        parent_label: rust_i18n::t!("location.parent_label", locale = loc).to_string(),
        submit_label: rust_i18n::t!("location.submit", locale = loc).to_string(),
        none_label: rust_i18n::t!("location.none", locale = loc).to_string(),
        current_url: current_url(&uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = loc).to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render location edit template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateLocationForm {
    pub name: String,
    pub node_type: String,
    pub version: i32,
    #[serde(default)]
    pub parent_id: Option<u64>,
}

pub async fn update_location(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<UpdateLocationForm>,
) -> Result<impl IntoResponse, AppError> {
    // Story 7-1 decision 1a: Admin → Librarian.
    session.require_role(Role::Librarian, locale.0)?;

    LocationService::update_location(
        &state.pool,
        id,
        form.version,
        &form.name,
        &form.node_type,
        form.parent_id,
    )
    .await?;

    Ok(axum::response::Redirect::to("/locations").into_response())
}

// ─── Delete location ─────────────────────────────────────────────

pub async fn delete_location(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Admin, locale.0)?;
    let loc = locale.0;

    LocationService::delete_location(&state.pool, id).await?;

    let message = rust_i18n::t!("location.deleted", locale = loc).to_string();
    Ok(Html(format!(
        r#"<div class="p-3 border-l-4 border-green-500 bg-green-50 dark:bg-green-900/20 rounded-r" role="status">
            <p class="text-stone-700 dark:text-stone-300">{}</p>
        </div>"#,
        crate::utils::html_escape(&message)
    )))
}

// ─── Next L-code JSON endpoint ───────────────────────────────────

pub async fn next_lcode(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    OriginalUri(uri): OriginalUri,
) -> Result<impl IntoResponse, AppError> {
    // Story 7-1 decision 1a: Admin → Librarian (used by create-location form).
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;

    let lcode = LocationService::get_next_available_lcode(&state.pool).await?;
    Ok(axum::Json(serde_json::json!({"lcode": lcode})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;

    #[test]
    fn test_build_tree_empty() {
        let tree = build_tree(&[], &HashMap::new());
        assert!(tree.is_empty());
    }

    #[test]
    fn test_build_tree_single_root() {
        let locations = vec![LocationModel {
            id: 1,
            parent_id: None,
            name: "Maison".to_string(),
            node_type: "Room".to_string(),
            label: "L0001".to_string(),
        }];
        let tree = build_tree(&locations, &HashMap::new());
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].location.name, "Maison");
        assert!(tree[0].children.is_empty());
    }

    #[test]
    fn test_build_tree_nested() {
        let locations = vec![
            LocationModel {
                id: 1,
                parent_id: None,
                name: "Maison".to_string(),
                node_type: "Room".to_string(),
                label: "L0001".to_string(),
            },
            LocationModel {
                id: 2,
                parent_id: Some(1),
                name: "Salon".to_string(),
                node_type: "Room".to_string(),
                label: "L0002".to_string(),
            },
            LocationModel {
                id: 3,
                parent_id: Some(2),
                name: "Étagère 1".to_string(),
                node_type: "Shelf".to_string(),
                label: "L0003".to_string(),
            },
        ];
        let mut counts = HashMap::new();
        counts.insert(3, 5u64);
        let tree = build_tree(&locations, &counts);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children[0].volume_count, 5);
    }

    /// Issue #185 regression lock: the inline "add child" form rendered by
    /// `render_tree_html` MUST include the CSRF synchronizer token as a
    /// hidden input. Forgetting it causes the middleware to reject the POST
    /// with a 303 → /login → / silent redirect.
    ///
    /// This sits outside the Askama `forms_include_csrf_token` audit
    /// (issue #48) because that audit scans templates only, not HTML
    /// produced by Rust code. Keep this test until #48 lands.
    #[test]
    fn render_tree_html_includes_csrf_token_in_inline_form() {
        let locations = vec![LocationModel {
            id: 42,
            parent_id: None,
            name: "Salon".to_string(),
            node_type: "room".to_string(),
            label: "L0001".to_string(),
        }];
        let tree = build_tree(&locations, &HashMap::new());
        let token = "test-csrf-token-abcdef0123456789";
        let html = render_tree_html(
            &tree,
            &[(1, "room".to_string())],
            "L0002",
            /* can_edit */ true,
            "en",
            token,
        );
        let needle = format!(r#"<input type="hidden" name="_csrf_token" value="{token}">"#);
        assert!(
            html.contains(&needle),
            "inline add-child form must include the CSRF token (issue #185). \
             Looked for {needle:?} in rendered HTML."
        );
    }

    /// When the viewer cannot edit, no form is rendered — the CSRF token
    /// should NOT leak into the read-only HTML.
    #[test]
    fn render_tree_html_omits_csrf_token_when_read_only() {
        let locations = vec![LocationModel {
            id: 42,
            parent_id: None,
            name: "Salon".to_string(),
            node_type: "room".to_string(),
            label: "L0001".to_string(),
        }];
        let tree = build_tree(&locations, &HashMap::new());
        let html = render_tree_html(
            &tree,
            &[(1, "room".to_string())],
            "L0002",
            /* can_edit */ false,
            "en",
            "should-not-appear",
        );
        assert!(
            !html.contains("_csrf_token"),
            "no form, no token: read-only tree leaked CSRF token in HTML"
        );
        assert!(
            !html.contains("should-not-appear"),
            "read-only tree leaked the CSRF token value"
        );
    }

    /// CSRF token must be HTML-escaped when injected into the hidden input's
    /// `value=` attribute (defense in depth — tokens are constant-time-compared
    /// against the session row, so a non-escaped token wouldn't directly cause
    /// XSS, but the audit-friendly contract is "every dynamic attribute value
    /// goes through html_escape").
    #[test]
    fn render_tree_html_escapes_csrf_token() {
        let locations = vec![LocationModel {
            id: 1,
            parent_id: None,
            name: "Salon".to_string(),
            node_type: "room".to_string(),
            label: "L0001".to_string(),
        }];
        let tree = build_tree(&locations, &HashMap::new());
        let html = render_tree_html(
            &tree,
            &[(1, "room".to_string())],
            "L0002",
            true,
            "en",
            r#"a"b<c>"#,
        );
        assert!(html.contains(r#"value="a&quot;b&lt;c&gt;""#));
        assert!(!html.contains(r#"value="a"b<c>""#));
    }

    /// Fix #200: a parent node carries a `tree-toggle` button + a
    /// `tree-children-<id>` container around its children. Leaf nodes
    /// carry no toggle (alignment spacer instead). This pairs with
    /// `static/js/locations-tree-toggle.js` for the fold/unfold UX.
    #[test]
    fn render_tree_html_emits_toggle_and_children_container_for_parents() {
        let locations = vec![
            LocationModel {
                id: 1,
                parent_id: None,
                name: "Salon".to_string(),
                node_type: "room".to_string(),
                label: "L0001".to_string(),
            },
            LocationModel {
                id: 2,
                parent_id: Some(1),
                name: "Étagère".to_string(),
                node_type: "shelf".to_string(),
                label: "L0002".to_string(),
            },
        ];
        let tree = build_tree(&locations, &HashMap::new());
        let html = render_tree_html(
            &tree,
            &[(1, "room".to_string())],
            "L0003",
            /* can_edit */ false,
            "en",
            "tok",
        );

        // Parent (id=1) has children → expect a toggle wired to the
        // children container, and the container itself with the matching id.
        assert!(
            html.contains(r#"data-tree-target="tree-children-1""#),
            "parent's toggle should target its children container"
        );
        assert!(
            html.contains(r#"id="tree-children-1""#),
            "parent should wrap its children in a tree-children container"
        );

        // The leaf (id=2) has no children → no toggle button, no container.
        assert!(
            !html.contains(r#"data-tree-target="tree-children-2""#),
            "leaf node should not emit a toggle"
        );
        assert!(
            !html.contains(r#"id="tree-children-2""#),
            "leaf node should not emit a children container"
        );
    }

    #[test]
    fn test_location_detail_template_renders() {
        let template = LocationDetailTemplate {
            lang: "en".to_string(),
            role: "anonymous".to_string(),
            current_page: "location",
            skip_label: "Skip".to_string(),
            connection_status: crate::utils::ConnectionStatusContext::new("en"),
            shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new("en"),
            session_timeout_secs: crate::config::AppSettings::default().session_timeout_secs,
            csrf_token: "tok".to_string(),
            nav_catalog: "Catalog".to_string(),
            nav_loans: "Loans".to_string(),
            nav_wishlist: "Wish list".to_string(),
            nav_locations: "Locations".to_string(),
            nav_series: "Series".to_string(),
            nav_borrowers: "Borrowers".to_string(),
            nav_admin: "Admin".to_string(),
            nav_login: "Log in".to_string(),
            nav_logout: "Log out".to_string(),
            nav_menu_open: "Open menu".to_string(),
            location: LocationModel {
                id: 1,
                parent_id: None,
                name: "Salon".to_string(),
                node_type: "room".to_string(),
                label: "L0001".to_string(),
            },
            breadcrumb_segments: vec![(1, "Salon".to_string())],
            volumes: crate::models::PaginatedList::new(vec![], 1, 0, None, None, None),
            contents_title: "Shelf contents".to_string(),
            empty_volumes: "No volumes".to_string(),
            col_title: "Title".to_string(),
            col_author: "Author".to_string(),
            col_genre: "Genre".to_string(),
            col_dewey: "Dewey".to_string(),
            col_condition: "Condition".to_string(),
            col_status: "Status".to_string(),
            prev_label: "Previous".to_string(),
            next_label: "Next".to_string(),
            pagination_aria: "Pagination".to_string(),
            current_url: "/location/1".to_string(),
            lang_toggle_aria: "Change language".to_string(),
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Salon"));
        assert!(rendered.contains("No volumes"));
    }
}
