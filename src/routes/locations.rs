use std::collections::HashMap;

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::models::location::LocationModel;
use crate::services::locations::LocationService;
use crate::AppState;

use crate::models::volume::{VolumeModel, VolumeWithTitle};
use crate::models::PaginatedList;

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
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub location: LocationModel,
    pub breadcrumb_segments: Vec<(u64, String)>,
    pub volumes: PaginatedList<VolumeWithTitle>,
    pub contents_title: String,
    pub empty_volumes: String,
    pub col_title: String,
    pub col_author: String,
    pub col_genre: String,
    pub col_condition: String,
    pub col_status: String,
    pub prev_label: String,
    pub next_label: String,
}

pub async fn location_detail(
    State(state): State<AppState>,
    session: Session,
    HxRequest(_is_htmx): HxRequest,
    Path(id): Path<u64>,
    axum::extract::Query(params): axum::extract::Query<LocationDetailQuery>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;

    let location = LocationModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    let breadcrumb_segments = LocationModel::get_path_segments(pool, location.id).await?;
    let volumes = VolumeModel::find_by_location(pool, id, &params.sort, &params.dir, params.page).await?;

    let template = LocationDetailTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "location",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_locations: rust_i18n::t!("nav.locations").to_string(),
            nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        contents_title: rust_i18n::t!("location.contents_title").to_string(),
        empty_volumes: rust_i18n::t!("location.empty_volumes").to_string(),
        col_title: rust_i18n::t!("location.col_title").to_string(),
        col_author: rust_i18n::t!("location.col_author").to_string(),
        col_genre: rust_i18n::t!("location.col_genre").to_string(),
        col_condition: rust_i18n::t!("location.col_condition").to_string(),
        col_status: rust_i18n::t!("location.col_status").to_string(),
        prev_label: rust_i18n::t!("pagination.previous").to_string(),
        next_label: rust_i18n::t!("pagination.next").to_string(),
        location,
        breadcrumb_segments,
        volumes,
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
fn render_tree_html(nodes: &[TreeNode], node_types: &[(u64, String)], next_lcode: &str) -> String {
    let mut html = String::new();
    for node in nodes {
        render_node_html(node, &mut html, node_types, next_lcode);
    }
    html
}

fn render_node_html(node: &TreeNode, html: &mut String, node_types: &[(u64, String)], next_lcode: &str) {
    render_node_at_depth(node, html, node_types, next_lcode, 0);
}

fn render_node_at_depth(node: &TreeNode, html: &mut String, node_types: &[(u64, String)], next_lcode: &str, depth: usize) {
    let name = crate::utils::html_escape(&node.location.name);
    let label = crate::utils::html_escape(&node.location.label);
    let node_type = crate::utils::html_escape(&node.location.node_type);
    let icon = if node.children.is_empty() { "📍" } else { "📁" };
    let vol = if node.volume_count > 0 {
        format!(r#" <span class="text-xs text-indigo-600 dark:text-indigo-400">{} vol</span>"#, node.volume_count)
    } else {
        String::new()
    };

    // Build type options
    let mut type_options = String::new();
    for (_, nt_name) in node_types {
        let nt_escaped = crate::utils::html_escape(nt_name);
        type_options.push_str(&format!(r#"<option value="{nt_escaped}">{nt_escaped}</option>"#));
    }
    let name_lbl = crate::utils::html_escape(rust_i18n::t!("location.name_label").as_ref());
    let type_lbl = crate::utils::html_escape(rust_i18n::t!("location.type_label").as_ref());
    let lcode_lbl = crate::utils::html_escape(rust_i18n::t!("location.lcode_label").as_ref());
    let submit_lbl = crate::utils::html_escape(rust_i18n::t!("location.submit").as_ref());
    let form_id = format!("add-child-{}", node.location.id);

    // Indentation: 2rem per depth level
    let indent_px = depth * 32;

    html.push_str(&format!(
        r#"<div role="treeitem" style="padding-left: {indent_px}px;">
<div class="flex items-center gap-2 px-3 py-2 rounded-md hover:bg-stone-100 dark:hover:bg-stone-800 group">
<span class="text-stone-400" aria-hidden="true">{icon}</span>
<span class="font-medium text-stone-900 dark:text-stone-100">{name}</span>
<span class="text-xs text-stone-400 font-mono">{label}</span>
<span class="text-xs text-stone-500 dark:text-stone-400">({node_type})</span>{vol}
<span class="ml-auto flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
<button type="button" onclick="document.getElementById('{form_id}').classList.toggle('hidden')" class="p-1 text-stone-400 hover:text-green-600 dark:hover:text-green-400" aria-label="Add child under {name}">➕</button>
<a href="/locations/{id}/edit" class="p-1 text-stone-400 hover:text-indigo-600 dark:hover:text-indigo-400" aria-label="Edit {name}">✏️</a>
<button type="button" hx-delete="/locations/{id}" hx-confirm="Delete {name} ({label})?" hx-target="closest [role=treeitem]" hx-swap="outerHTML" class="p-1 text-stone-400 hover:text-red-600 dark:hover:text-red-400" aria-label="Delete {name}">🗑️</button>
</span>
</div>
<form id="{form_id}" method="POST" action="/locations" class="hidden px-3 py-2 space-y-2 bg-stone-50 dark:bg-stone-800/50 rounded-md mt-1 mb-2" style="margin-left: {child_indent}px;">
<input type="hidden" name="parent_id" value="{id}">
<div class="grid grid-cols-1 md:grid-cols-3 gap-2">
<div><label class="block text-xs text-stone-600 dark:text-stone-400">{name_lbl}</label><input type="text" name="name" required class="w-full px-2 py-1 text-sm border border-stone-300 dark:border-stone-600 rounded bg-white dark:bg-stone-800 text-stone-900 dark:text-stone-100"></div>
<div><label class="block text-xs text-stone-600 dark:text-stone-400">{type_lbl}</label><select name="node_type" required class="w-full px-2 py-1 text-sm border border-stone-300 dark:border-stone-600 rounded bg-white dark:bg-stone-800 text-stone-900 dark:text-stone-100">{type_options}</select></div>
<div><label class="block text-xs text-stone-600 dark:text-stone-400">{lcode_lbl}</label><input type="text" name="label" value="{next_lcode}" required maxlength="5" pattern="L[0-9]{{4}}" class="w-full px-2 py-1 text-sm font-mono border border-stone-300 dark:border-stone-600 rounded bg-white dark:bg-stone-800 text-stone-900 dark:text-stone-100"></div>
</div>
<button type="submit" class="px-3 py-1 text-xs font-medium text-white bg-indigo-600 hover:bg-indigo-700 rounded">{submit_lbl}</button>
</form>
</div>"#,
        id = node.location.id,
        child_indent = indent_px + 32,
        next_lcode = crate::utils::html_escape(next_lcode),
    ));

    // Render children at deeper indentation
    for child in &node.children {
        render_node_at_depth(child, html, node_types, next_lcode, depth + 1);
    }
}

// ─── Location tree page ──────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/locations.html")]
pub struct LocationsTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
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
}

pub async fn locations_page(
    session: Session,
    HxRequest(_is_htmx): HxRequest,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let pool = &state.pool;
    let locations = LocationModel::find_all_tree(pool).await?;
    let node_types = LocationModel::find_node_types(pool).await?;
    let next_lcode = LocationService::get_next_available_lcode(pool).await?;

    // Get volume counts for each location
    let mut volume_counts = HashMap::new();
    for loc in &locations {
        let count = LocationService::get_recursive_volume_count(pool, loc.id)
            .await
            .unwrap_or(0);
        volume_counts.insert(loc.id, count);
    }

    let tree = build_tree(&locations, &volume_counts);
    let tree_html = render_tree_html(&tree, &node_types, &next_lcode);

    let template = LocationsTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "locations",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_locations: rust_i18n::t!("nav.locations").to_string(),
            nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        tree_title: rust_i18n::t!("location.tree_title").to_string(),
        tree_html,
        node_types,
        next_lcode,
        empty_state: rust_i18n::t!("location.empty_state").to_string(),
        add_root_label: rust_i18n::t!("location.add_root").to_string(),
        name_label: rust_i18n::t!("location.name_label").to_string(),
        type_label: rust_i18n::t!("location.type_label").to_string(),
        lcode_label: rust_i18n::t!("location.lcode_label").to_string(),
        submit_label: rust_i18n::t!("location.submit").to_string(),
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
    State(state): State<AppState>,
    axum::Form(form): axum::Form<CreateLocationForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Admin)?;

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
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
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
}

pub async fn edit_location_page(
    session: Session,
    HxRequest(_is_htmx): HxRequest,
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Admin)?;

    let pool = &state.pool;
    let location = LocationModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;
    let version = LocationModel::get_version(pool, id).await?;
    let node_types = LocationModel::find_node_types(pool).await?;
    let all_locations = LocationModel::find_all_tree(pool).await?;

    let template = LocationEditTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "locations",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_locations: rust_i18n::t!("nav.locations").to_string(),
            nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        edit_title: rust_i18n::t!("location.edit").to_string(),
        location,
        version,
        node_types,
        all_locations,
        name_label: rust_i18n::t!("location.name_label").to_string(),
        type_label: rust_i18n::t!("location.type_label").to_string(),
        parent_label: rust_i18n::t!("location.parent_label").to_string(),
        submit_label: rust_i18n::t!("location.submit").to_string(),
        none_label: rust_i18n::t!("location.none").to_string(),
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
    State(state): State<AppState>,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<UpdateLocationForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Admin)?;

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
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Admin)?;

    LocationService::delete_location(&state.pool, id).await?;

    let message = rust_i18n::t!("location.deleted").to_string();
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
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Admin)?;

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
            LocationModel { id: 1, parent_id: None, name: "Maison".to_string(), node_type: "Room".to_string(), label: "L0001".to_string() },
            LocationModel { id: 2, parent_id: Some(1), name: "Salon".to_string(), node_type: "Room".to_string(), label: "L0002".to_string() },
            LocationModel { id: 3, parent_id: Some(2), name: "Étagère 1".to_string(), node_type: "Shelf".to_string(), label: "L0003".to_string() },
        ];
        let mut counts = HashMap::new();
        counts.insert(3, 5u64);
        let tree = build_tree(&locations, &counts);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children[0].volume_count, 5);
    }

    #[test]
    fn test_location_detail_template_renders() {
        let template = LocationDetailTemplate {
            lang: "en".to_string(),
            role: "anonymous".to_string(),
            current_page: "location",
            skip_label: "Skip".to_string(),
            nav_catalog: "Catalog".to_string(),
            nav_loans: "Loans".to_string(),
            nav_locations: "Locations".to_string(),
            nav_borrowers: "Borrowers".to_string(),
            nav_admin: "Admin".to_string(),
            nav_login: "Log in".to_string(),
            nav_logout: "Log out".to_string(),
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
            col_condition: "Condition".to_string(),
            col_status: "Status".to_string(),
            prev_label: "Previous".to_string(),
            next_label: "Next".to_string(),
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Salon"));
        assert!(rendered.contains("No volumes"));
    }
}
