//! CR #443 tranche 2 — attach and detach labels on titles and volumes.
//!
//! One module for both entity kinds, mirroring `LabelTarget` in the model: the
//! two join tables are structurally identical, and a second copy of these
//! handlers would be the place where the two sides quietly drift apart.
//!
//! **Librarian+ throughout.** Requirement 6 of the issue is that an anonymous
//! visitor must never see labels — not on a page, not through a URL, not in a
//! count. Both handlers require the role, and the detail-page handlers skip
//! loading labels entirely rather than loading them and hiding them.

use askama::Template;
use axum::Form;
use axum::extract::{Extension, Path, State};
use axum::response::Html;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::locale::Locale;
use crate::models::label::{LabelModel, LabelTarget};

/// One attached label as the fragment needs it.
pub struct AttachedLabel {
    pub id: u64,
    pub name: String,
    pub detach_aria: String,
}

/// Vocabulary entry offered in the "add" select.
pub struct AvailableLabel {
    pub id: u64,
    pub name: String,
}

#[derive(Template)]
#[template(path = "fragments/entity_labels.html")]
pub struct EntityLabelsFragment {
    pub labels: Vec<AttachedLabel>,
    pub available: Vec<AvailableLabel>,
    pub labels_endpoint: String,
    pub csrf_token: String,
    pub label_labels_heading: String,
    pub label_labels_none: String,
    pub label_labels_add: String,
}

#[derive(serde::Deserialize)]
pub struct AttachLabelForm {
    pub label_id: u64,
    pub _csrf_token: String,
}

/// Build the fragment for one entity.
///
/// `available` excludes what is already attached: offering a label the entity
/// already carries would make the select a list of no-ops, and the attach is
/// idempotent so nothing would visibly happen.
pub async fn build_fragment(
    state: &AppState,
    target: LabelTarget,
    endpoint: String,
    csrf_token: String,
    loc: &'static str,
) -> Result<EntityLabelsFragment, AppError> {
    let attached = LabelModel::list_for(&state.pool, target).await?;
    let attached_ids: Vec<u64> = attached.iter().map(|l| l.id).collect();
    let available = LabelModel::list_all(&state.pool)
        .await?
        .into_iter()
        .filter(|l| !attached_ids.contains(&l.id))
        .map(|l| AvailableLabel {
            id: l.id,
            name: l.name,
        })
        .collect();

    Ok(EntityLabelsFragment {
        labels: attached
            .into_iter()
            .map(|l| AttachedLabel {
                detach_aria: rust_i18n::t!("labels.detach_aria", locale = loc, name = &l.name)
                    .to_string(),
                id: l.id,
                name: l.name,
            })
            .collect(),
        available,
        labels_endpoint: endpoint,
        csrf_token,
        label_labels_heading: rust_i18n::t!("labels.heading", locale = loc).to_string(),
        label_labels_none: rust_i18n::t!("labels.none", locale = loc).to_string(),
        label_labels_add: rust_i18n::t!("labels.add", locale = loc).to_string(),
    })
}

fn endpoint_for(target: LabelTarget) -> String {
    match target {
        LabelTarget::Title(id) => format!("/title/{id}/labels"),
        LabelTarget::Volume(id) => format!("/volume/{id}/labels"),
    }
}

async fn render(
    state: &AppState,
    target: LabelTarget,
    csrf_token: String,
    loc: &'static str,
) -> Result<Html<String>, AppError> {
    let fragment = build_fragment(state, target, endpoint_for(target), csrf_token, loc).await?;
    Ok(Html(fragment.render().map_err(|_| {
        AppError::Internal("entity labels render failed".to_string())
    })?))
}

pub async fn title_labels_attach(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<AttachLabelForm>,
) -> Result<Html<String>, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let target = LabelTarget::Title(id);
    LabelModel::attach(&state.pool, target, form.label_id).await?;
    render(&state, target, session.csrf_token.clone(), locale.0).await
}

pub async fn title_labels_detach(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path((id, label_id)): Path<(u64, u64)>,
) -> Result<Html<String>, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let target = LabelTarget::Title(id);
    LabelModel::detach(&state.pool, target, label_id).await?;
    render(&state, target, session.csrf_token.clone(), locale.0).await
}

pub async fn volume_labels_attach(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<AttachLabelForm>,
) -> Result<Html<String>, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let target = LabelTarget::Volume(id);
    LabelModel::attach(&state.pool, target, form.label_id).await?;
    render(&state, target, session.csrf_token.clone(), locale.0).await
}

pub async fn volume_labels_detach(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path((id, label_id)): Path<(u64, u64)>,
) -> Result<Html<String>, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let target = LabelTarget::Volume(id);
    LabelModel::detach(&state.pool, target, label_id).await?;
    render(&state, target, session.csrf_token.clone(), locale.0).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_entity_specific() {
        // The fragment posts back to whatever endpoint it was built with, so
        // a mixed-up path would attach a label to the wrong entity kind.
        assert_eq!(endpoint_for(LabelTarget::Title(7)), "/title/7/labels");
        assert_eq!(endpoint_for(LabelTarget::Volume(7)), "/volume/7/labels");
    }
}
