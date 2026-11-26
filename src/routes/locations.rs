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
use crate::services::locations::{DESCRIPTION_MAX_CHARS, DescriptionTooLong, LocationService};
use crate::utils::base_context;

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
    pub base: crate::utils::BaseContextFields,
    pub location: LocationModel,
    pub breadcrumb_segments: Vec<(u64, String)>,
    /// Story 20-4 — the breadcrumb's `aria-label` (was hard-coded English).
    pub breadcrumb_label: String,
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
    // CR #237 — bulk-mark affordance.
    pub can_audit: bool,
    pub bulk_audit_button_label: String,
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

    let base = base_context(&session, loc, "location", &uri, state.page_settings());
    let template = LocationDetailTemplate {
        base,
        breadcrumb_label: rust_i18n::t!("location.breadcrumb_label", locale = loc).to_string(),
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
        // CR #237 — bulk-mark affordance. Hidden for organizational
        // containers (an organizational location holds no volumes,
        // there's nothing to mark) and hidden for Anonymous.
        can_audit: session.role >= Role::Librarian && location.is_assignable(),
        bulk_audit_button_label: rust_i18n::t!(
            "location.bulk_audit_button",
            locale = loc
        )
        .to_string(),
        location,
        breadcrumb_segments,
        volumes,
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render location detail template");
            Err(AppError::TemplateRenderFailed { template: "locations" })
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
    rights: TreeRights,
    loc: &str,
    csrf_token: &str,
) -> String {
    let mut html = String::new();
    for node in nodes {
        render_node_at_depth(node, &mut html, node_types, next_lcode, 0, rights, loc, csrf_token);
    }
    html
}

/// What the viewer may do from the tree rows: add a child and edit
/// (Librarian+), delete (Admin only, like `delete_location` — #698: a
/// Librarian used to see a delete control that always answered a refusal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TreeRights {
    pub can_edit: bool,
    pub can_delete: bool,
}

impl TreeRights {
    pub(crate) fn for_role(role: &Role) -> Self {
        Self {
            can_edit: *role >= Role::Librarian,
            can_delete: *role >= Role::Admin,
        }
    }
}

/// #695 — the row's add / edit / delete controls. They used to be
/// `opacity-0` until hover: invisible on a touch screen (no hover) and while
/// keyboard-focused, and — `group-hover:opacity-100` living only in this Rust
/// string, which the Docker CSS stage never scanned — invisible everywhere in
/// the shipped image. Now always shown, dimmed to 75 % (the glyphs are colour
/// emoji, whose colours CSS does not set: dimmed, never hidden), at full
/// opacity on row hover, on keyboard focus anywhere in the row and whenever a
/// touch pointer is present (`any-pointer-coarse`, hybrid devices included). The classes are only
/// generated because `static/css/input.css` sources `src/` (checked by
/// `tests/tailwind_sources.rs`).
pub(crate) const TREE_ROW_CONTROLS_CLASS: &str = "ml-auto flex items-center gap-1 opacity-75 group-hover:opacity-100 group-focus-within:opacity-100 any-pointer-coarse:opacity-100 transition-opacity";

/// Story 20-5 — classes of the spacer a leaf row shows in place of the
/// disclosure button. Shared with the parent OOB of a deletion that empties
/// a parent (`parent_emptied_oob`), so the two cannot drift.
const TREE_LEAF_SPACER_CLASS: &str = "w-6";

/// Story 20-5 — classes of the 📁 / 📍 icon span inside a row's link. Shared
/// with `parent_emptied_oob` (the parent's icon turns into the leaf icon).
const TREE_ICON_CLASS: &str = "text-stone-500 dark:text-stone-400";

/// Icon of a location with children / without children.
const TREE_ICON_PARENT: &str = "📁";
const TREE_ICON_LEAF: &str = "📍";

#[allow(clippy::too_many_arguments)]
fn render_node_at_depth(
    node: &TreeNode,
    html: &mut String,
    node_types: &[(u64, String)],
    next_lcode: &str,
    depth: usize,
    rights: TreeRights,
    loc: &str,
    csrf_token: &str,
) {
    let name = crate::utils::html_escape(&node.location.name);
    let label = crate::utils::html_escape(&node.location.label);
    let node_type = crate::utils::html_escape(&node.location.node_type);
    let icon = if node.children.is_empty() {
        TREE_ICON_LEAF
    } else {
        TREE_ICON_PARENT
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
    // CR #280 — organizational checkbox copy for the inline child-create
    // form rendered inside the locations tree.
    let org_label = crate::utils::html_escape(
        rust_i18n::t!("location.organizational_label", locale = loc).as_ref(),
    );
    let org_help = crate::utils::html_escape(
        rust_i18n::t!("location.organizational_help", locale = loc).as_ref(),
    );
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

    let (mutation_controls, child_form) = if rights.can_edit {
        // #698 — accessible names localized (the raw name goes through `t!()`,
        // then the whole label is escaped once); delete opens the UX-DR8
        // modal in `#modal-slot` (scanner-guard watches it), Admin only.
        let raw_name = node.location.name.as_str();
        let add_lbl = crate::utils::html_escape(
            rust_i18n::t!("location.tree.add_child_named", locale = loc, name = raw_name).as_ref(),
        );
        let edit_lbl = crate::utils::html_escape(
            rust_i18n::t!("location.tree.edit_named", locale = loc, name = raw_name).as_ref(),
        );
        let delete_control = if rights.can_delete {
            let delete_lbl = crate::utils::html_escape(
                rust_i18n::t!("location.tree.delete_named", locale = loc, name = raw_name).as_ref(),
            );
            // Rule #21 — after the delete, the row (and this trigger) is gone:
            // modal.js then focuses the parent row's link, or the "add root"
            // summary for a root location. The selector follows the row
            // structure `li#tree-node-{id} > div > a` (story 20-5); the
            // parent OOBs of `parent_emptied_oob` never replace that link.
            let focus_fallback = match node.location.parent_id {
                Some(parent) => format!(r#"#tree-node-{parent} > div > a[href="/location/{parent}"]"#),
                None => "#add-root-summary".to_string(),
            };
            format!(
                "\n<button type=\"button\" hx-get=\"/locations/{id}/delete-modal\" hx-target=\"#modal-slot\" hx-swap=\"innerHTML\" data-modal-trigger data-modal-focus-fallback=\"{fallback}\" aria-haspopup=\"dialog\" aria-expanded=\"false\" class=\"p-1 text-stone-600 dark:text-stone-400 hover:text-red-600 dark:hover:text-red-400\" aria-label=\"{delete_lbl}\">🗑️</button>",
                id = node.location.id,
                fallback = crate::utils::html_escape(&focus_fallback),
            )
        } else {
            String::new()
        };
        (
            format!(
                r#"<span class="{TREE_ROW_CONTROLS_CLASS}">
<button type="button" data-locations-toggle="{form_id}" class="p-1 text-stone-600 dark:text-stone-400 hover:text-green-600 dark:hover:text-green-400" aria-label="{add_lbl}">➕</button>
<a href="/locations/{id}/edit" class="p-1 text-stone-600 dark:text-stone-400 hover:text-indigo-600 dark:hover:text-indigo-400" aria-label="{edit_lbl}">✏️</a>{delete_control}
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
<label class="inline-flex items-start gap-2"><input type="checkbox" name="is_organizational" value="true" class="mt-1"><span><span class="text-xs font-medium text-stone-700 dark:text-stone-300">{org_label}</span><span class="block text-xs text-stone-500 dark:text-stone-400">{org_help}</span></span></label>
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
    // Issue #200 / story 20-5 (#700, D10): the tree is a nested list with
    // disclosure buttons — no ARIA tree, no arrow keys, native Tab order.
    // Each location is an `<li id="tree-node-{id}">`; a location with at
    // least one child gets a disclosure button (`aria-expanded`,
    // `aria-controls`) in front of its link, and its children sit in a
    // `<ul class="tree-children" id="tree-children-{id}">` inside its
    // `<li>`. `static/js/locations-tree-toggle.js` hides that list on click
    // (Enter / Space are the native button's) and persists the
    // collapsed-node-id set in localStorage so the layout survives a
    // reload. Indentation classes still drive the visual depth.
    let has_children = !node.children.is_empty();
    let toggle_html = if has_children {
        format!(
            // #424 — the toggle lives in a row that turns `bg-stone-100` on
            // hover, where `text-stone-400` sits at 2.31:1. The glyph is
            // `aria-hidden`, so axe's color-contrast rule skips it, but it is
            // still the visual indicator of a control and WCAG 1.4.11 asks for
            // 3:1. `text-stone-600` gives 6.99:1 there. The hover shade moves
            // to stone-900 because stone-600 was previously the hover value —
            // keeping it would have flattened the affordance.
            //
            // Story 20-5 — the accessible name includes the location name
            // (raw name through `t!()`, the whole label escaped once); the
            // state is read from `aria-expanded`.
            r#"<button type="button" id="tree-toggle-{id}" class="tree-toggle p-1 text-stone-600 dark:text-stone-400 hover:text-stone-900 dark:hover:text-stone-200" aria-controls="tree-children-{id}" aria-expanded="true" aria-label="{toggle_lbl}"><span class="tree-toggle-icon" aria-hidden="true">▼</span></button>"#,
            id = node.location.id,
            toggle_lbl = crate::utils::html_escape(
                rust_i18n::t!(
                    "location.tree.toggle_named",
                    locale = loc,
                    name = node.location.name.as_str()
                )
                .as_ref()
            ),
        )
    } else {
        // Spacer keeps row labels aligned across the column whether or
        // not a row has children. `w-6` matches the toggle button's
        // approximate width (icon + padding).
        format!(r#"<span class="{TREE_LEAF_SPACER_CLASS}" aria-hidden="true"></span>"#)
    };

    html.push_str(&format!(
        r#"<li id="tree-node-{id}" class="{indent_class}" data-tree-node-id="{id}">
<div class="flex items-center gap-2 px-3 py-2 rounded-md hover:bg-stone-100 dark:hover:bg-stone-800 group">
{toggle_html}
<a href="/location/{id}" class="flex items-center gap-2 flex-1 min-w-0 no-underline">
<span id="tree-icon-{id}" class="{TREE_ICON_CLASS}" aria-hidden="true">{icon}</span>
<span class="font-medium text-stone-900 dark:text-stone-100">{name}</span>
<span class="text-xs text-stone-600 dark:text-stone-400 font-mono">{label}</span>
<span class="text-xs text-stone-600 dark:text-stone-400">({node_type})</span>{vol}
</a>
{mutation_controls}
</div>
{child_form}"#,
        id = node.location.id,
    ));

    if has_children {
        html.push_str(&format!(
            // Story 20-5 — `role="list"` keeps list semantics under the
            // preflight's `list-style: none` (VoiceOver drops them otherwise).
            r#"<ul class="tree-children" role="list" id="tree-children-{id}">"#,
            id = node.location.id,
        ));
        for child in &node.children {
            render_node_at_depth(
                child,
                html,
                node_types,
                next_lcode,
                depth + 1,
                rights,
                loc,
                csrf_token,
            );
        }
        html.push_str("</ul>");
    }

    html.push_str("</li>");
}

// ─── Location tree page ──────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/locations.html")]
pub struct LocationsTemplate {
    pub base: crate::utils::BaseContextFields,
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
    // CR #280 — organizational checkbox copy (also used by the inline
    // root-create form on the tree page).
    pub organizational_label: String,
    pub organizational_help: String,
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
    let tree_html = render_tree_html(
        &tree,
        &node_types,
        &next_lcode,
        TreeRights::for_role(&session.role),
        loc,
        &session.csrf_token,
    );

    let base = base_context(&session, loc, "locations", &uri, state.page_settings());
    let template = LocationsTemplate {
        base,
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
        organizational_label: rust_i18n::t!("location.organizational_label", locale = loc)
            .to_string(),
        organizational_help: rust_i18n::t!("location.organizational_help", locale = loc)
            .to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render locations template");
            Err(AppError::TemplateRenderFailed { template: "locations" })
        }
    }
}

// ─── Create location ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateLocationForm {
    pub name: String,
    pub node_type: String,
    pub label: String,
    /// Fix #296 — the form's `<select name="parent_id">` carries
    /// `<option value="">None</option>` for root locations; the
    /// browser submits `parent_id=` (empty string). Plain
    /// `#[serde(default)] Option<u64>` only handles ABSENT fields, so
    /// the u64 parser used to bail with "cannot parse integer from
    /// empty string". The custom deserializer treats empty as `None`.
    #[serde(default, deserialize_with = "crate::routes::series::deserialize_optional_u64")]
    pub parent_id: Option<u64>,
    /// CR #280 — checkbox: HTML submits the field only when checked.
    /// `Option<String>` lets us treat "absent" as `false`.
    #[serde(default)]
    pub is_organizational: Option<String>,
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
        form.is_organizational.is_some(),
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
    pub base: crate::utils::BaseContextFields,
    pub edit_title: String,
    /// The location as the form shows it: the stored row, or — after a
    /// refusal — the row with the typed name, type, parent and
    /// organizational flag (id and L-code stay the stored ones).
    pub location: LocationModel,
    pub version: i32,
    pub node_types: Vec<(u64, String)>,
    pub all_locations: Vec<LocationModel>,
    pub name_label: String,
    pub type_label: String,
    pub parent_label: String,
    pub submit_label: String,
    pub none_label: String,
    // CR #280 — organizational checkbox copy.
    pub organizational_label: String,
    pub organizational_help: String,
    // Story 20-4 (#716) — description field.
    pub description_value: String,
    pub description_error: Option<String>,
    pub description_label: String,
    pub description_help: String,
    pub back_label: String,
}

/// Story 20-4 — what the edit form shows: the stored values, or the typed
/// ones after a refusal, with an optional message under the description.
struct EditFormState {
    name: String,
    node_type: String,
    parent_id: Option<u64>,
    is_organizational: bool,
    description: String,
    version: i32,
    description_error: Option<String>,
}

impl EditFormState {
    fn stored(location: &LocationModel, version: i32) -> Self {
        Self {
            name: location.name.clone(),
            node_type: location.node_type.clone(),
            parent_id: location.parent_id,
            is_organizational: location.is_organizational,
            description: location.description.clone().unwrap_or_default(),
            version,
            description_error: None,
        }
    }
}

/// Render the edit page — the GET and the refusal of a POST share it
/// (story 20-4). `uri` is the page's GET address, so the layout's return
/// links (language toggle, login `next`) never point at the POST route.
async fn render_edit_page(
    state: &AppState,
    session: &Session,
    loc: &'static str,
    uri: &axum::http::Uri,
    stored: LocationModel,
    form: EditFormState,
    status: axum::http::StatusCode,
) -> Result<axum::response::Response, AppError> {
    let pool = &state.pool;
    let node_types = LocationModel::find_node_types(pool).await?;
    let all_locations = LocationModel::find_all_tree(pool).await?;
    let location = LocationModel {
        name: form.name,
        node_type: form.node_type,
        parent_id: form.parent_id,
        is_organizational: form.is_organizational,
        ..stored
    };

    let base = base_context(session, loc, "locations", uri, state.page_settings());
    let template = LocationEditTemplate {
        base,
        edit_title: rust_i18n::t!("location.edit", locale = loc).to_string(),
        location,
        version: form.version,
        node_types,
        all_locations,
        name_label: rust_i18n::t!("location.name_label", locale = loc).to_string(),
        type_label: rust_i18n::t!("location.type_label", locale = loc).to_string(),
        parent_label: rust_i18n::t!("location.parent_label", locale = loc).to_string(),
        submit_label: rust_i18n::t!("location.submit", locale = loc).to_string(),
        none_label: rust_i18n::t!("location.none", locale = loc).to_string(),
        organizational_label: rust_i18n::t!("location.organizational_label", locale = loc)
            .to_string(),
        organizational_help: rust_i18n::t!("location.organizational_help", locale = loc)
            .to_string(),
        description_value: form.description,
        description_error: form.description_error,
        description_label: rust_i18n::t!("location.description_label", locale = loc).to_string(),
        description_help: rust_i18n::t!("location.description_help", locale = loc).to_string(),
        back_label: rust_i18n::t!("location.back_to_tree", locale = loc).to_string(),
    };
    match template.render() {
        Ok(html) => Ok((status, Html(html)).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render location edit template");
            Err(AppError::TemplateRenderFailed { template: "locations" })
        }
    }
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
    let form = EditFormState::stored(&location, version);
    render_edit_page(&state, &session, loc, &uri, location, form, axum::http::StatusCode::OK).await
}

#[derive(Deserialize)]
pub struct UpdateLocationForm {
    pub name: String,
    pub node_type: String,
    pub version: i32,
    /// Fix #296 — same empty-string-from-`<option value="">` shape
    /// as `CreateLocationForm.parent_id` above. See that comment for
    /// the full rationale.
    #[serde(default, deserialize_with = "crate::routes::series::deserialize_optional_u64")]
    pub parent_id: Option<u64>,
    #[serde(default)]
    pub is_organizational: Option<String>,
    /// Story 20-4 (#716) — optional; absent from an older form = blank.
    #[serde(default)]
    pub description: String,
}

pub async fn update_location(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<UpdateLocationForm>,
) -> Result<axum::response::Response, AppError> {
    // Story 7-1 decision 1a: Admin → Librarian.
    session.require_role(Role::Librarian, locale.0)?;
    let loc = locale.0;

    // Story 20-4 — a description over the limit is refused next to its
    // field (422, typed values kept, nothing written). The service's other
    // refusals keep their behaviour (issue #732).
    let description = match LocationService::normalize_description(&form.description) {
        Ok(description) => description,
        Err(DescriptionTooLong) => {
            let stored = LocationModel::find_by_id(&state.pool, id).await?.ok_or_else(|| {
                AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
            })?;
            let page = format!("/locations/{id}/edit");
            let uri: axum::http::Uri = page
                .parse()
                .map_err(|e| AppError::Internal(format!("location edit uri: {e}")))?;
            let typed = EditFormState {
                name: form.name,
                node_type: form.node_type,
                parent_id: form.parent_id,
                is_organizational: form.is_organizational.is_some(),
                description: form.description,
                version: form.version,
                description_error: Some(
                    rust_i18n::t!(
                        "location.description_too_long",
                        locale = loc,
                        max = DESCRIPTION_MAX_CHARS
                    )
                    .to_string(),
                ),
            };
            return render_edit_page(
                &state,
                &session,
                loc,
                &uri,
                stored,
                typed,
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            )
            .await;
        }
    };

    LocationService::update_location(
        &state.pool,
        id,
        form.version,
        &form.name,
        &form.node_type,
        form.parent_id,
        form.is_organizational.is_some(),
        description.as_deref(),
    )
    .await?;

    Ok(axum::response::Redirect::to("/locations").into_response())
}

// ─── Delete location ─────────────────────────────────────────────

/// Story 19-1 (#698) — the UX-DR8 confirmation of a tree delete.
#[derive(Template)]
#[template(path = "fragments/location_delete_modal.html")]
pub struct LocationDeleteModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
}

/// `GET /locations/{id}/delete-modal` — the delete confirmation for a tree
/// row (Admin, like the delete itself). HTMX only: a direct navigation gets
/// 405 + `Allow: GET`, like the other modal fragments. A location already
/// gone answers the "deleted by another session" entry in `#feedback-list`.
pub async fn delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<axum::response::Response, AppError> {
    session.require_role_with_return(Role::Admin, "/locations", locale.0)?;
    let loc = locale.0;

    if !is_htmx {
        return Ok((
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            [(axum::http::header::ALLOW, "GET")],
            String::new(),
        )
            .into_response());
    }

    let Some(location) = LocationModel::find_by_id(&state.pool, id).await? else {
        let emptied = emptied_parent(&state.pool, id).await;
        return Ok(already_gone_response(id, emptied, loc, AlreadyGoneAt::ModalOpen));
    };

    // Raw values through `t!()`; Askama escapes `{{ title }}` in the macro.
    let title = rust_i18n::t!(
        "location.delete_modal_title",
        locale = loc,
        name = location.name.as_str(),
        label = location.label.as_str()
    )
    .to_string();
    let body_html = format!(
        "<p>{}</p>",
        crate::utils::html_escape(rust_i18n::t!("location.delete_modal_body", locale = loc).as_ref())
    );
    let template = LocationDeleteModalTemplate {
        title,
        body_html,
        confirm_label: rust_i18n::t!("location.delete_modal_confirm", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/locations/{}", location.id),
        csrf_token: session.csrf_token.clone(),
    };
    template
        .render()
        .map(|html| Html(html).into_response())
        .map_err(|e| AppError::Internal(format!("location delete modal render: {e}")))
}

/// `DELETE /locations/{id}` — from the modal's Confirm. Success: a feedback
/// entry for `#feedback-list` (the modal's target), the tree row removed out
/// of band, and `modal-close` after the swap (so the OOB lookup runs while
/// the Confirm form is still attached). A refusal (children, volumes) is an
/// `AppError::BadRequest` in the request locale, shown inside the modal.
pub async fn delete_location(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Admin, locale.0)?;
    let loc = locale.0;

    match LocationService::delete_location(&state.pool, id, loc).await {
        Ok(()) => {}
        // Deleted meanwhile (another tab): say so and drop the stale row —
        // and, like a deletion made here, update a parent it left empty.
        Err(AppError::NotFound(_)) => {
            let emptied = emptied_parent(&state.pool, id).await;
            return Ok(already_gone_response(id, emptied, loc, AlreadyGoneAt::Confirm));
        }
        Err(e) => return Err(e),
    }

    let emptied = emptied_parent(&state.pool, id).await;
    let message = rust_i18n::t!("location.deleted", locale = loc).to_string();
    Ok(delete_success_response(id, emptied, &message))
}

/// Story 20-5 (#700, AC 4) — the parent of the deleted location `id` when
/// it has no live child left, else `None` (a root, a parent with other live
/// children, a row already purged). Read after the delete, a plain read (no
/// lock, Rule #22). A failure never turns the answer into an error — the
/// delete happened; the parent row is merely stale until a reload.
async fn emptied_parent(pool: &crate::db::DbPool, id: u64) -> Option<u64> {
    let parent = match LocationModel::parent_id_any_state(pool, id).await {
        Ok(Some(parent)) => parent,
        Ok(None) => return None,
        Err(e) => {
            tracing::warn!(location_id = id, error = %e, "Could not read the parent of a deleted location");
            return None;
        }
    };
    match LocationModel::find_children(pool, parent).await {
        Ok(children) if children.is_empty() => Some(parent),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!(location_id = id, parent_id = parent, error = %e, "Could not read the live children of a deleted location's parent");
            None
        }
    }
}

/// Where a location already deleted by another session is noticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlreadyGoneAt {
    /// Opening the modal (`hx-target` is `#modal-slot`: retarget the entry).
    ModalOpen,
    /// The modal's Confirm (already aimed at `#feedback-list`; close the modal).
    Confirm,
}

/// The "deleted by another session" entry, added on top of `#feedback-list`
/// (never replacing the entries already there), with the stale row removed
/// and, when `emptied_parent` is set, that parent turned into a leaf
/// (story 20-5).
fn already_gone_response(
    id: u64,
    emptied_parent: Option<u64>,
    loc: &'static str,
    at: AlreadyGoneAt,
) -> axum::response::Response {
    let oob = tree_removal_oob(id, emptied_parent);
    match at {
        AlreadyGoneAt::ModalOpen => crate::routes::build_already_deleted_response_with(
            loc,
            "#feedback-list",
            "afterbegin",
            &oob,
        ),
        AlreadyGoneAt::Confirm => {
            let mut main = crate::routes::already_deleted_entry_html(loc);
            main.push_str(&oob);
            crate::middleware::htmx::HtmxResponse { main, oob: Vec::new() }
                .into_response_with_hx_trigger_after_swap("modal-close")
        }
    }
}

/// htmx 2: an OOB element with `hx-swap-oob="delete"` removes its target —
/// here the tree row `#tree-node-{id}`.
fn row_removal_oob(id: u64) -> String {
    format!(r#"<div id="tree-node-{id}" hx-swap-oob="delete"></div>"#)
}

/// Story 20-5 (#700, AC 4) — a parent whose last live child was deleted
/// becomes a leaf at once: its (now empty) child list goes, its disclosure
/// button becomes the leaf spacer, its 📁 icon the 📍 one. Three targeted
/// OOBs rather than a re-render of the parent `<li>` (which would need its
/// depth, volume count, rights, CSRF token… and would wipe an open inline
/// "add child" form). None of them replaces the parent's link, the focus
/// fallback of the delete control (Rule #21).
fn parent_emptied_oob(parent_id: u64) -> String {
    format!(
        concat!(
            r#"<ul id="tree-children-{p}" hx-swap-oob="delete"></ul>"#,
            r#"<span id="tree-toggle-{p}" class="{spacer}" aria-hidden="true" hx-swap-oob="outerHTML"></span>"#,
            r#"<span id="tree-icon-{p}" class="{icon_class}" aria-hidden="true" hx-swap-oob="outerHTML">{leaf}</span>"#,
        ),
        p = parent_id,
        spacer = TREE_LEAF_SPACER_CLASS,
        icon_class = TREE_ICON_CLASS,
        leaf = TREE_ICON_LEAF,
    )
}

/// The row removal first, then the parent update when the parent was left
/// empty (had the list gone first, the row OOB would only miss its target).
fn tree_removal_oob(id: u64, emptied_parent: Option<u64>) -> String {
    let mut oob = row_removal_oob(id);
    if let Some(parent) = emptied_parent {
        oob.push_str(&parent_emptied_oob(parent));
    }
    oob
}

/// The success answer of [`delete_location`], split out for its unit test.
fn delete_success_response(
    id: u64,
    emptied_parent: Option<u64>,
    message: &str,
) -> axum::response::Response {
    let mut main = crate::utils::feedback_html("success", message, "");
    main.push_str(&tree_removal_oob(id, emptied_parent));
    crate::middleware::htmx::HtmxResponse { main, oob: Vec::new() }
        .into_response_with_hx_trigger_after_swap("modal-close")
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

    /// A non-organizational location (test fixture).
    fn loc(id: u64, parent_id: Option<u64>, name: &str, node_type: &str, label: &str) -> LocationModel {
        LocationModel {
            id,
            parent_id,
            name: name.to_string(),
            node_type: node_type.to_string(),
            label: label.to_string(),
            is_organizational: false,
            description: None,
        }
    }

    const ADMIN: TreeRights = TreeRights { can_edit: true, can_delete: true };
    const LIBRARIAN: TreeRights = TreeRights { can_edit: true, can_delete: false };
    const READ_ONLY: TreeRights = TreeRights { can_edit: false, can_delete: false };

    /// The tree of one root location "Salon", rendered with the given rights and token.
    fn render_salon(id: u64, rights: TreeRights, csrf_token: &str) -> String {
        render_salon_in(id, rights, csrf_token, "en")
    }

    fn render_salon_in(id: u64, rights: TreeRights, csrf_token: &str, locale: &str) -> String {
        let tree = build_tree(&[loc(id, None, "Salon", "room", "L0001")], &HashMap::new());
        render_tree_html(&tree, &[(1, "room".to_string())], "L0002", rights, locale, csrf_token)
    }

    /// Fix #296 — `<select name="parent_id"><option value="">None</option>...
    /// </select>` sends `parent_id=` (empty string) when the user picks the
    /// root-location option. The `deserialize_optional_u64` wiring on
    /// `parent_id` must accept that as `None`. Same coverage for the
    /// `is_organizational` checkbox absence (the form omits the field
    /// entirely when unchecked).
    #[test]
    fn create_location_form_accepts_empty_parent_id() {
        let body = "name=Salon&node_type=room&label=L0001&parent_id=";
        let form: CreateLocationForm = serde_urlencoded::from_str(body).unwrap();
        assert_eq!(form.parent_id, None);
        assert!(form.is_organizational.is_none());
    }

    #[test]
    fn create_location_form_parses_non_empty_parent_id() {
        let body = "name=Tablette&node_type=shelf&label=L0002&parent_id=7";
        let form: CreateLocationForm = serde_urlencoded::from_str(body).unwrap();
        assert_eq!(form.parent_id, Some(7));
    }

    #[test]
    fn update_location_form_accepts_empty_parent_id_with_organizational_checkbox() {
        // Reproduces #296 exactly — root location, organisationnel ticked.
        let body = "name=Salon&node_type=room&version=3&parent_id=&is_organizational=on";
        let form: UpdateLocationForm = serde_urlencoded::from_str(body).unwrap();
        assert_eq!(form.parent_id, None);
        assert!(form.is_organizational.is_some());
    }

    #[test]
    fn update_location_form_parses_non_empty_parent_id() {
        let body = "name=Tablette&node_type=shelf&version=2&parent_id=4";
        let form: UpdateLocationForm = serde_urlencoded::from_str(body).unwrap();
        assert_eq!(form.parent_id, Some(4));
        assert!(form.is_organizational.is_none());
        // Story 20-4 — a form without the field reads as blank.
        assert_eq!(form.description, "");
    }

    #[test]
    fn update_location_form_parses_a_description() {
        let body = "name=Tablette&node_type=shelf&version=2&parent_id=&description=Left+bookcase%0D%0Atop+shelf";
        let form: UpdateLocationForm = serde_urlencoded::from_str(body).unwrap();
        assert_eq!(form.description, "Left bookcase\r\ntop shelf");
    }

    #[test]
    fn test_build_tree_empty() {
        let tree = build_tree(&[], &HashMap::new());
        assert!(tree.is_empty());
    }

    #[test]
    fn test_build_tree_single_root() {
        let locations = vec![loc(1, None, "Maison", "Room", "L0001")];
        let tree = build_tree(&locations, &HashMap::new());
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].location.name, "Maison");
        assert!(tree[0].children.is_empty());
    }

    #[test]
    fn test_build_tree_nested() {
        let locations = vec![
            loc(1, None, "Maison", "Room", "L0001"),
            loc(2, Some(1), "Salon", "Room", "L0002"),
            loc(3, Some(2), "Étagère 1", "Shelf", "L0003"),
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
        let token = "test-csrf-token-abcdef0123456789";
        let html = render_salon(42, ADMIN, token);
        let needle = format!(r#"<input type="hidden" name="_csrf_token" value="{token}">"#);
        assert!(
            html.contains(&needle),
            "inline add-child form must include the CSRF token (issue #185). \
             Looked for {needle:?} in rendered HTML."
        );
    }

    /// #695 — the add / edit / delete controls are never hidden at rest:
    /// dimmed, and full opacity on row hover, focus or a coarse pointer.
    #[test]
    fn tree_row_controls_are_visible_at_rest() {
        let html = render_salon(42, ADMIN, "t");
        assert!(html.contains(&format!(r#"<span class="{TREE_ROW_CONTROLS_CLASS}">"#)));
        let classes: Vec<&str> = TREE_ROW_CONTROLS_CLASS.split_whitespace().collect();
        assert!(!classes.contains(&"opacity-0"), "controls must not be invisible at rest");
        for needed in [
            "opacity-75",
            "group-hover:opacity-100",
            "group-focus-within:opacity-100",
            "any-pointer-coarse:opacity-100",
        ] {
            assert!(classes.contains(&needed), "missing {needed}");
        }
    }

    /// When the viewer cannot edit, no form is rendered — the CSRF token
    /// should NOT leak into the read-only HTML.
    #[test]
    fn render_tree_html_omits_csrf_token_when_read_only() {
        let html = render_salon(42, READ_ONLY, "should-not-appear");
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
        let html = render_salon(1, ADMIN, r#"a"b<c>"#);
        assert!(html.contains(r#"value="a&quot;b&lt;c&gt;""#));
        assert!(!html.contains(r#"value="a"b<c>""#));
    }

    /// Fix #200 / story 20-5: a parent node carries a disclosure button
    /// (`aria-expanded`, `aria-controls` naming its child list) and a
    /// `<ul class="tree-children" id="tree-children-<id>">` around its
    /// children. Leaf nodes carry no button (alignment spacer instead). This
    /// pairs with `static/js/locations-tree-toggle.js` for the fold/unfold UX.
    #[test]
    fn render_tree_html_emits_toggle_and_children_container_for_parents() {
        let locations = vec![
            loc(1, None, "Salon", "room", "L0001"),
            loc(2, Some(1), "Étagère", "shelf", "L0002"),
        ];
        let tree = build_tree(&locations, &HashMap::new());
        let html = render_tree_html(
            &tree,
            &[(1, "room".to_string())],
            "L0003",
            READ_ONLY,
            "en",
            "tok",
        );

        // Parent (id=1): the button names its child list, which exists.
        let button = html
            .split('<')
            .find(|t| t.starts_with("button") && t.contains(r#"id="tree-toggle-1""#))
            .expect("the parent has a disclosure button");
        assert!(button.contains(r#"class="tree-toggle "#), "{button}");
        assert!(button.contains(r#"aria-controls="tree-children-1""#), "{button}");
        assert!(button.contains(r#"aria-expanded="true""#), "{button}");
        assert!(
            html.contains(r#"<ul class="tree-children" role="list" id="tree-children-1">"#),
            "the id aria-controls names must exist: {html}"
        );
        assert!(html.contains(r#"<span id="tree-icon-1" class="text-stone-500 dark:text-stone-400" aria-hidden="true">📁</span>"#));

        // The leaf (id=2) has no children → no button, no container, the spacer.
        assert!(!html.contains("tree-toggle-2"), "leaf node should not emit a toggle");
        assert!(
            !html.contains(r#"id="tree-children-2""#),
            "leaf node should not emit a children container"
        );
        assert!(html.contains(r#"<span id="tree-icon-2" class="text-stone-500 dark:text-stone-400" aria-hidden="true">📍</span>"#));
        assert!(html.contains(r#"<span class="w-6" aria-hidden="true"></span>"#));
    }

    /// Story 20-5 (D10) — a nested list, no ARIA tree: no tree role in the
    /// rows nor in the `/locations` page around them. The attribute is
    /// matched, not the word: the row keeps the Tailwind `group` class.
    #[test]
    fn the_tree_carries_no_tree_roles() {
        let tree = build_tree(
            &[
                loc(1, None, "Salon", "room", "L0001"),
                loc(2, Some(1), "Étagère", "shelf", "L0002"),
            ],
            &HashMap::new(),
        );
        let tree_html = render_tree_html(&tree, &[], "L0003", ADMIN, "en", "t");
        let page = LocationsTemplate {
            base: crate::utils::test_base_context(
                "admin",
                "locations",
                crate::config::AppSettings::default().session_timeout_secs,
            ),
            tree_title: "Storage locations".to_string(),
            tree_html: tree_html.clone(),
            node_types: vec![(1, "room".to_string())],
            next_lcode: "L0003".to_string(),
            empty_state: "No locations yet".to_string(),
            add_root_label: "Add a root location".to_string(),
            name_label: "Name".to_string(),
            type_label: "Type".to_string(),
            lcode_label: "L-code".to_string(),
            submit_label: "Create".to_string(),
            organizational_label: "Organizational".to_string(),
            organizational_help: "Holds locations only".to_string(),
        }
        .render()
        .unwrap();
        for html in [&tree_html, &page] {
            for role in [r#"role="tree""#, r#"role="treeitem""#, r#"role="group""#] {
                assert!(!html.contains(role), "{role} left in: {html}");
            }
        }
        assert!(tree_html.contains(" group\">"), "the row keeps the Tailwind group class");
        assert!(page.contains(r#"<h1 id="locations-heading""#), "{page}");
        assert!(
            page.contains(r#"<ul id="locations-tree" role="list" aria-labelledby="locations-heading""#),
            "{page}"
        );
        assert!(page.contains(r#"id="tree-node-1""#));
    }

    /// Story 20-5 — every `<li>` opened is closed, and a child list sits
    /// inside its parent's `<li>` (root → child → grandchild).
    #[test]
    fn nested_rows_are_well_formed_list_items() {
        let tree = build_tree(
            &[
                loc(1, None, "Maison", "room", "L0001"),
                loc(2, Some(1), "Salon", "room", "L0002"),
                loc(3, Some(2), "Étagère", "shelf", "L0003"),
            ],
            &HashMap::new(),
        );
        let html = render_tree_html(&tree, &[], "L0004", ADMIN, "en", "t");
        assert_eq!(html.matches("<li ").count(), 3);
        assert_eq!(html.matches("</li>").count(), 3);
        assert_eq!(html.matches(r#"<ul class="tree-children""#).count(), 2);
        assert_eq!(html.matches("</ul>").count(), 2);

        // Walk the tags: the stack of open li / ul must nest as
        // li#1 > ul#1 > li#2 > ul#2 > li#3, and end empty.
        let mut stack: Vec<String> = Vec::new();
        let mut max_depth = 0;
        for tag in html.split('<').skip(1) {
            let name: String = tag.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '/').collect();
            match name.as_str() {
                "li" | "ul" => {
                    if name == "li" {
                        assert!(matches!(stack.last().map(String::as_str), None | Some("ul")), "li outside a list: {stack:?}");
                    } else {
                        assert_eq!(stack.last().map(String::as_str), Some("li"), "child list outside its parent li");
                    }
                    stack.push(name);
                    max_depth = max_depth.max(stack.len());
                }
                "/li" | "/ul" => {
                    assert_eq!(stack.pop().as_deref(), Some(&name[1..]), "mismatched {name}");
                }
                _ => {}
            }
        }
        assert!(stack.is_empty(), "unclosed: {stack:?}");
        assert_eq!(max_depth, 5);
        let li2 = html.find(r#"<li id="tree-node-2""#).unwrap();
        let ul1 = html.find(r#"id="tree-children-1""#).unwrap();
        let li3 = html.find(r#"<li id="tree-node-3""#).unwrap();
        let ul2 = html.find(r#"id="tree-children-2""#).unwrap();
        assert!(ul1 < li2 && li2 < ul2 && ul2 < li3);
    }

    /// Story 20-5 (D11) — the disclosure button's name includes the location
    /// name, in the request language, escaped once.
    #[test]
    fn toggle_name_is_localized_and_escaped() {
        let parent_and_child = |name: &str| {
            build_tree(
                &[loc(1, None, name, "room", "L0001"), loc(2, Some(1), "Étagère", "shelf", "L0002")],
                &HashMap::new(),
            )
        };
        let en = render_tree_html(&parent_and_child("Salon"), &[], "L0003", READ_ONLY, "en", "t");
        assert!(en.contains(r#"aria-label="Locations inside Salon""#), "{en}");
        let fr = render_tree_html(&parent_and_child("Salon"), &[], "L0003", READ_ONLY, "fr", "t");
        assert!(fr.contains(r#"aria-label="Emplacements dans Salon""#), "{fr}");

        let html = render_tree_html(&parent_and_child(r#"A<b>"c"#), &[], "L0003", READ_ONLY, "en", "t");
        assert!(html.contains(r#"aria-label="Locations inside A&lt;b&gt;&quot;c""#), "{html}");
        assert!(!html.contains("&amp;lt;"), "double-escaped");
    }

    /// #698 — rights follow the handlers: Librarians add and edit, only an
    /// Admin gets the delete control (the delete itself is Admin-only).
    #[test]
    fn tree_rights_follow_the_role() {
        assert_eq!(TreeRights::for_role(&Role::Admin), ADMIN);
        assert_eq!(TreeRights::for_role(&Role::Librarian), LIBRARIAN);
        assert_eq!(TreeRights::for_role(&Role::Anonymous), READ_ONLY);
    }

    /// #698 — delete opens the UX-DR8 modal (no native `hx-confirm`), the row
    /// carries the id the success answer removes, a Librarian sees no delete.
    #[test]
    fn delete_control_opens_the_modal_for_admins_only() {
        let admin = render_salon(42, ADMIN, "t");
        assert!(!admin.contains("hx-confirm"));
        assert!(!admin.contains("hx-delete"));
        assert!(admin.contains(r##"hx-get="/locations/42/delete-modal" hx-target="#modal-slot" hx-swap="innerHTML" data-modal-trigger data-modal-focus-fallback="#add-root-summary" aria-haspopup="dialog""##));
        assert!(admin.contains(r#"id="tree-node-42""#));

        let librarian = render_salon(42, LIBRARIAN, "t");
        assert!(!librarian.contains("delete-modal"));
        assert!(librarian.contains(r#"href="/locations/42/edit""#));
        assert!(librarian.contains(r#"data-locations-toggle="add-child-42""#));
    }

    /// #698 — the controls' accessible names are in the request language,
    /// with the location name escaped once.
    #[test]
    fn control_labels_are_localized_and_escaped() {
        let fr = render_salon_in(42, ADMIN, "t", "fr");
        assert!(fr.contains(r#"aria-label="Ajouter un emplacement dans Salon""#));
        assert!(fr.contains(r#"aria-label="Modifier Salon""#));
        assert!(fr.contains(r#"aria-label="Supprimer Salon""#));

        let tree = build_tree(&[loc(7, None, r#"A<b>"c"#, "room", "L0001")], &HashMap::new());
        let html = render_tree_html(&tree, &[], "L0002", ADMIN, "en", "t");
        assert!(html.contains(r#"aria-label="Delete A&lt;b&gt;&quot;c""#));
        assert!(!html.contains("&amp;lt;"), "double-escaped");
    }

    /// #698 — the success answer: a feedback entry, the row removed out of
    /// band, the modal closed after the swap.
    #[tokio::test]
    async fn delete_success_answer_removes_the_row_and_closes_the_modal() {
        let resp = delete_success_response(42, None, "Location deleted.");
        assert_eq!(
            resp.headers().get("hx-trigger-after-swap").unwrap(),
            "modal-close"
        );
        let body = body_of(resp).await;
        assert!(body.contains("feedback-entry"));
        assert!(body.contains(r#"data-feedback-variant="success""#));
        assert!(body.contains("Location deleted."));
        assert!(body.contains(r#"<div id="tree-node-42" hx-swap-oob="delete"></div>"#));
        // Story 20-5 — the parent kept other children: no parent update.
        assert!(!body.contains("tree-children-"), "{body}");
        assert!(!body.contains("tree-toggle-"), "{body}");
        assert!(!body.contains("tree-icon-"), "{body}");
    }

    /// Story 20-5 (AC 4) — the last child deleted: after the row removal,
    /// the parent's child list goes, its button becomes the leaf spacer and
    /// its icon the leaf icon, with the classes the tree renders.
    #[tokio::test]
    async fn delete_success_answer_turns_an_emptied_parent_into_a_leaf() {
        let body = body_of(delete_success_response(42, Some(1), "Location deleted.")).await;
        let row = body.find(r#"<div id="tree-node-42" hx-swap-oob="delete"></div>"#).unwrap();
        let list = body.find(r#"<ul id="tree-children-1" hx-swap-oob="delete"></ul>"#).unwrap();
        assert!(row < list, "the row removal comes first");
        assert!(body.contains(r#"<span id="tree-toggle-1" class="w-6" aria-hidden="true" hx-swap-oob="outerHTML"></span>"#), "{body}");
        assert!(body.contains(r#"<span id="tree-icon-1" class="text-stone-500 dark:text-stone-400" aria-hidden="true" hx-swap-oob="outerHTML">📍</span>"#), "{body}");

        // The OOB copies of the spacer and the icon match the rendered ones.
        let tree = build_tree(&[loc(1, None, "Salon", "room", "L0001")], &HashMap::new());
        let rendered = render_tree_html(&tree, &[], "L0002", ADMIN, "en", "t");
        assert!(rendered.contains(&format!(r#"<span class="{TREE_LEAF_SPACER_CLASS}" aria-hidden="true"></span>"#)));
        assert!(rendered.contains(&format!(r#"<span id="tree-icon-1" class="{TREE_ICON_CLASS}" aria-hidden="true">{TREE_ICON_LEAF}</span>"#)));
    }

    /// #698 — the modal fragment: the delete variant, DELETE to the location,
    /// the answer aimed at `#feedback-list`, the name escaped by Askama.
    #[test]
    fn delete_modal_template_renders_the_confirmation() {
        let html = LocationDeleteModalTemplate {
            title: "Delete <Salon> (L0001)?".to_string(),
            body_html: "<p>Trash</p>".to_string(),
            confirm_label: "Delete the location".to_string(),
            cancel_label: "Cancel".to_string(),
            action_url: "/locations/42".to_string(),
            csrf_token: "tok".to_string(),
        }
        .render()
        .unwrap();
        assert!(html.contains("Salon") && !html.contains("<Salon>"), "title not escaped");
        assert!(html.contains(r#"hx-delete="/locations/42""#), "{html}");
        assert!(html.contains("#feedback-list"));
        assert!(!html.contains("hx-confirm"));
    }

    /// Rule #21 — a child row's delete names its parent's link as the focus
    /// fallback (the row and its trigger leave with the delete).
    #[test]
    fn child_delete_falls_back_to_the_parent_link() {
        let tree = build_tree(
            &[loc(1, None, "Salon", "room", "L0001"), loc(2, Some(1), "Étagère", "shelf", "L0002")],
            &HashMap::new(),
        );
        let html = render_tree_html(&tree, &[], "L0003", ADMIN, "en", "t");
        assert!(html.contains(
            r##"data-modal-focus-fallback="#tree-node-1 &gt; div &gt; a[href=&quot;/location/1&quot;]""##
        ));
    }

    async fn body_of(resp: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    /// Review 19-1 — a location already deleted elsewhere: the entry is added
    /// on top of `#feedback-list` (not replacing it), in the request locale,
    /// and the stale row is removed; at Confirm the modal closes.
    #[tokio::test]
    async fn already_gone_answers_add_an_entry_and_drop_the_row() {
        let open = already_gone_response(9, None, "fr", AlreadyGoneAt::ModalOpen);
        assert_eq!(open.headers().get("HX-Retarget").unwrap(), "#feedback-list");
        assert_eq!(open.headers().get("HX-Reswap").unwrap(), "afterbegin");
        assert!(open.headers().get("hx-trigger-after-swap").is_none());
        let body = body_of(open).await;
        assert!(body.contains(r#"<div id="tree-node-9" hx-swap-oob="delete"></div>"#));
        let fr = rust_i18n::t!("error.already_deleted_by_another_session", locale = "fr").to_string();
        assert!(body.contains(&crate::utils::html_escape(&fr)), "{body}");

        let confirm = already_gone_response(9, None, "en", AlreadyGoneAt::Confirm);
        assert!(confirm.headers().get("HX-Retarget").is_none());
        assert_eq!(confirm.headers().get("hx-trigger-after-swap").unwrap(), "modal-close");
        assert!(!body_of(confirm).await.contains("tree-children-"));
    }

    /// Story 20-5 — a child deleted in another tab can have emptied its
    /// parent: both "already deleted" answers update it like a deletion.
    #[tokio::test]
    async fn already_gone_answers_update_an_emptied_parent() {
        for at in [AlreadyGoneAt::ModalOpen, AlreadyGoneAt::Confirm] {
            let body = body_of(already_gone_response(9, Some(3), "en", at)).await;
            let row = body.find(r#"<div id="tree-node-9" hx-swap-oob="delete"></div>"#).unwrap();
            let list = body.find(r#"<ul id="tree-children-3" hx-swap-oob="delete"></ul>"#).unwrap();
            assert!(row < list, "{at:?}: {body}");
            assert!(body.contains(r#"id="tree-toggle-3""#), "{at:?}");
            assert!(body.contains(r#"id="tree-icon-3""#), "{at:?}");
        }
    }

    fn detail_template(location: LocationModel) -> LocationDetailTemplate {
        LocationDetailTemplate {
            base: crate::utils::test_base_context("anonymous", "location", crate::config::AppSettings::default().session_timeout_secs),
            location,
            breadcrumb_segments: vec![(1, "Salon".to_string())],
            breadcrumb_label: "Location path".to_string(),
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
            can_audit: false,
            bulk_audit_button_label: "Mark all volumes for audit".to_string(),
        }
    }

    #[test]
    fn test_location_detail_template_renders() {
        let rendered = detail_template(loc(1, None, "Salon", "room", "L0001")).render().unwrap();
        assert!(rendered.contains("Salon"));
        assert!(rendered.contains("No volumes"));
        assert!(rendered.contains(r#"aria-label="Location path""#));
        // Story 20-4 — no description, no element.
        assert!(!rendered.contains("location-description"), "{rendered}");
    }

    /// Story 20-4 — the description is shown escaped, line breaks kept by
    /// the `whitespace-pre-line` class, never as HTML.
    #[test]
    fn location_detail_shows_the_description_escaped() {
        let mut location = loc(1, None, "Salon", "room", "L0001");
        location.description = Some("Left <b>bookcase</b>\r\ntop shelf".to_string());
        let rendered = detail_template(location).render().unwrap();
        assert!(rendered.contains(r#"id="location-description""#), "{rendered}");
        assert!(rendered.contains("whitespace-pre-line"));
        assert!(rendered.contains("Left &#60;b&#62;bookcase&#60;/b&#62;\r\ntop shelf"), "{rendered}");
        assert!(!rendered.contains("<b>bookcase"));
    }

    fn edit_template(description_value: &str, description_error: Option<&str>) -> String {
        LocationEditTemplate {
            base: crate::utils::test_base_context("librarian", "locations", crate::config::AppSettings::default().session_timeout_secs),
            edit_title: "Edit location".to_string(),
            location: loc(7, None, "Salon", "room", "L0007"),
            version: 3,
            node_types: vec![(1, "room".to_string())],
            all_locations: vec![],
            name_label: "Name".to_string(),
            type_label: "Type".to_string(),
            parent_label: "Parent".to_string(),
            submit_label: "Save".to_string(),
            none_label: "None".to_string(),
            organizational_label: "Organizational".to_string(),
            organizational_help: "Holds locations only".to_string(),
            description_value: description_value.to_string(),
            description_error: description_error.map(str::to_string),
            description_label: "Description".to_string(),
            description_help: "Optional. Where to find it.".to_string(),
            back_label: "Back to locations".to_string(),
        }
        .render()
        .unwrap()
    }

    /// Story 20-4 — normal state: the textarea carries the stored text and
    /// points at the help line only; no error slot.
    #[test]
    fn location_edit_shows_the_description_field() {
        let html = edit_template("Top <shelf>", None);
        assert!(html.contains(r#"<label for="edit-description""#), "{html}");
        assert!(html.contains(r#"aria-describedby="edit-description-help""#), "{html}");
        assert!(html.contains(">Top &#60;shelf&#62;</textarea>"), "{html}");
        assert!(html.contains(r#"id="edit-description-help""#));
        assert!(!html.contains("edit-description-error"), "{html}");
        assert!(!html.contains("aria-invalid"), "{html}");
        assert!(html.contains("Back to locations"));
        assert!(!html.contains("← Back\n"), "the hard-coded English link is gone");
    }

    /// Story 20-4 — refused state: the message sits under the textarea,
    /// which is marked invalid and names both the help and the error.
    #[test]
    fn location_edit_shows_the_refusal_next_to_the_field() {
        let typed = "x".repeat(501);
        let html = edit_template(&typed, Some("Description must be at most 500 characters."));
        assert!(html.contains(r#"aria-invalid="true""#), "{html}");
        assert!(
            html.contains(r#"aria-describedby="edit-description-help edit-description-error""#),
            "{html}"
        );
        assert!(html.contains(r#"<p id="edit-description-error""#), "{html}");
        assert!(html.contains("Description must be at most 500 characters."));
        assert!(html.contains(&format!(">{typed}</textarea>")));
        let textarea = html.find(r#"id="edit-description""#).unwrap();
        let error = html.find(r#"id="edit-description-error""#).unwrap();
        assert!(textarea < error, "the error comes right after the field");
    }
}
