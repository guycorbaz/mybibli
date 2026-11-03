//! Story 13-8 (#580) — "Move to another title" on the volume page.
//!
//! - `GET  /volume/{id}/move-modal` — the UX-DR8 Pattern A modal (HTMX only).
//! - `GET  /volume/{id}/move/candidates?q=` — the picker's candidate list.
//! - `GET  /volume/{id}/move/selection?title_id=` — the picked target: the
//!   hidden `title_id` and the confirmation sentence.
//! - `POST /volume/{id}/move` — the move (`title_id`, `version`, CSRF).
//!
//! Kept out of `catalog.rs` / `titles.rs` (Foundation Rule #12). All four
//! are Librarian+. Every fragment is an auto-escaped Askama render, so the
//! titles, authors and ISBNs shown are escaped.

use askama::Template;
use axum::Extension;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;
use crate::models::volume_move::{self, MoveCandidate};
use crate::services::volume::VolumeService;
use crate::services::volume_move::move_refusal;
use crate::utils::feedback_html;

#[derive(Template)]
#[template(path = "fragments/volume_move_modal.html")]
pub struct VolumeMoveModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
    pub version: i32,
    pub inner_form_html: String,
}

#[derive(Template)]
#[template(path = "fragments/volume_move_picker.html")]
pub struct VolumeMovePickerTemplate {
    pub volume_id: u64,
    pub intro: String,
    pub search_label: String,
    pub search_placeholder: String,
}

#[derive(Template)]
#[template(path = "fragments/volume_move_candidates.html")]
pub struct VolumeMoveCandidatesTemplate {
    pub volume_id: u64,
    pub query_empty: bool,
    pub candidates: Vec<MoveCandidate>,
    pub empty_label: String,
}

#[derive(Template)]
#[template(path = "fragments/volume_move_selection.html")]
pub struct VolumeMoveSelectionTemplate {
    pub title_id: Option<u64>,
    pub text: String,
}

/// The "Title:" row of the volume page (also its OOB update after a move).
#[derive(Template)]
#[template(path = "fragments/volume_title_row.html")]
pub struct VolumeTitleRowTemplate {
    pub title_row_id: u64,
    pub title_row_name: String,
}

#[derive(Debug, Deserialize)]
pub struct CandidatesQuery {
    #[serde(default)]
    pub q: String,
}

#[derive(Debug, Deserialize)]
pub struct SelectionQuery {
    #[serde(default)]
    pub title_id: String,
}

#[derive(Debug, Deserialize)]
pub struct MoveForm {
    pub version: i32,
    #[serde(default)]
    pub title_id: String,
}

/// A form/query `title_id`: a positive id, else no target.
pub(crate) fn parse_title_id(raw: &str) -> Option<u64> {
    raw.trim().parse::<u64>().ok().filter(|id| *id > 0)
}

/// How a target title is named in the confirmation and the feedback:
/// its title, then its primary contributor when known.
pub(crate) fn candidate_display(c: &MoveCandidate) -> String {
    match c.primary_contributor.as_deref() {
        Some(author) if !author.trim().is_empty() => format!("{} — {}", c.title, author),
        _ => c.title.clone(),
    }
}

fn render<T: Template>(template: T, what: &str) -> Result<String, AppError> {
    template
        .render()
        .map_err(|e| AppError::Internal(format!("volume move {what} render: {e}")))
}

async fn live_volume(state: &AppState, id: u64, loc: &str) -> Result<VolumeModel, AppError> {
    VolumeModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(
                rust_i18n::t!("error.volume_move_volume_gone", locale = loc).to_string(),
            )
        })
}

async fn title_name(state: &AppState, id: u64) -> Result<String, AppError> {
    Ok(TitleModel::find_by_id(&state.pool, id)
        .await?
        .map(|t| t.title)
        .unwrap_or_else(|| "?".to_string()))
}

fn gate(session: &Session, id: u64, loc: &'static str) -> Result<(), AppError> {
    session.require_role_with_return(Role::Librarian, &format!("/volume/{id}"), loc)
}

/// `GET /volume/{id}/move-modal` — HTMX only (a direct navigation gets 405,
/// like the delete-volume modal).
pub async fn move_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    gate(&session, id, loc)?;
    if !is_htmx {
        return Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    let volume = live_volume(&state, id, loc).await?;
    let current = title_name(&state, volume.title_id).await?;
    let label = volume.label.as_str();

    let body_html = render(
        VolumeMovePickerTemplate {
            volume_id: volume.id,
            intro: rust_i18n::t!(
                "volume.move_modal_intro",
                locale = loc,
                label = label,
                title = current.as_str()
            )
            .to_string(),
            search_label: rust_i18n::t!("volume.move_search_label", locale = loc).to_string(),
            search_placeholder: rust_i18n::t!("volume.move_search_placeholder", locale = loc)
                .to_string(),
        },
        "picker",
    )?;
    let selection = render(
        VolumeMoveSelectionTemplate {
            title_id: None,
            text: rust_i18n::t!("volume.move_nothing_picked", locale = loc).to_string(),
        },
        "selection",
    )?;
    let inner_form_html =
        format!(r#"<div id="volume-move-selection" class="mb-2">{selection}</div>"#);

    let html = render(
        VolumeMoveModalTemplate {
            title: rust_i18n::t!("volume.move_modal_title", locale = loc, label = label)
                .to_string(),
            body_html,
            confirm_label: rust_i18n::t!("volume.move_confirm", locale = loc).to_string(),
            cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
            action_url: format!("/volume/{}/move", volume.id),
            csrf_token: session.csrf_token.clone(),
            version: volume.version,
            inner_form_html,
        },
        "modal",
    )?;
    Ok(Html(html).into_response())
}

/// `GET /volume/{id}/move/candidates?q=` — live titles matching `q` by
/// title or ISBN, the volume's current title excluded.
pub async fn move_candidates(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Query(query): Query<CandidatesQuery>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    gate(&session, id, loc)?;
    let volume = live_volume(&state, id, loc).await?;
    let candidates = volume_move::search_candidates(&state.pool, &query.q, volume.title_id).await?;
    let html = render(
        VolumeMoveCandidatesTemplate {
            volume_id: volume.id,
            query_empty: query.q.trim().is_empty(),
            candidates,
            empty_label: rust_i18n::t!("volume.move_no_candidates", locale = loc).to_string(),
        },
        "candidates",
    )?;
    Ok(Html(html).into_response())
}

/// `GET /volume/{id}/move/selection?title_id=` — the picked target. A
/// target the move would refuse (same title, deleted) shows the refusal
/// and no `title_id`.
pub async fn move_selection(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Query(query): Query<SelectionQuery>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    gate(&session, id, loc)?;
    let volume = live_volume(&state, id, loc).await?;
    let target_id = parse_title_id(&query.title_id);
    let target = match target_id {
        Some(t) if t != volume.title_id => volume_move::find_candidate(&state.pool, t).await?,
        _ => None,
    };

    let selection = match (
        move_refusal(volume.title_id, target_id, target.is_some()),
        target,
    ) {
        (None, Some(target)) => {
            let current = title_name(&state, volume.title_id).await?;
            VolumeMoveSelectionTemplate {
                title_id: Some(target.id),
                text: rust_i18n::t!(
                    "volume.move_confirmation",
                    locale = loc,
                    label = volume.label.as_str(),
                    from = current.as_str(),
                    to = candidate_display(&target).as_str()
                )
                .to_string(),
            }
        }
        (refusal, _) => VolumeMoveSelectionTemplate {
            title_id: None,
            text: match refusal {
                Some(r) => r.message(loc),
                None => rust_i18n::t!("volume.move_nothing_picked", locale = loc).to_string(),
            },
        },
    };
    Ok(Html(render(selection, "selection")?).into_response())
}

/// `POST /volume/{id}/move` — the move. Success: a success FeedbackEntry
/// for `#feedback-list`, the title row updated out of band, and
/// `HX-Trigger-After-Swap: modal-close` (after the swap, so the OOB target
/// lookup runs before modal.js empties the slot — story 11-16).
pub async fn move_volume(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<MoveForm>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    gate(&session, id, loc)?;

    let moved = VolumeService::move_to_title(
        &state.pool,
        id,
        form.version,
        parse_title_id(&form.title_id),
        session.user_id,
        loc,
    )
    .await?;

    let from = title_name(&state, moved.from_title_id).await?;
    let to_name = title_name(&state, moved.to_title_id).await?;
    let to_display = volume_move::find_candidate(&state.pool, moved.to_title_id)
        .await?
        .map(|c| candidate_display(&c))
        .unwrap_or_else(|| to_name.clone());
    let message = rust_i18n::t!(
        "volume.move_done",
        locale = loc,
        label = moved.label.as_str(),
        from = from.as_str(),
        to = to_display.as_str()
    )
    .to_string();
    let row = render(
        VolumeTitleRowTemplate {
            title_row_id: moved.to_title_id,
            title_row_name: to_name,
        },
        "title row",
    )?;

    Ok(HtmxResponse {
        main: feedback_html("success", &message, ""),
        oob: vec![OobUpdate {
            target: "volume-title-row".to_string(),
            content: row,
            ..Default::default()
        }],
    }
    .into_response_with_hx_trigger_after_swap("modal-close"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_id_must_be_a_positive_integer() {
        assert_eq!(parse_title_id("42"), Some(42));
        assert_eq!(parse_title_id(" 7 "), Some(7));
        assert_eq!(parse_title_id(""), None);
        assert_eq!(parse_title_id("0"), None);
        assert_eq!(parse_title_id("-3"), None);
        assert_eq!(parse_title_id("abc"), None);
    }

    fn candidate(author: Option<&str>) -> MoveCandidate {
        MoveCandidate {
            id: 5,
            title: "Les Bijoux <de> la Castafiore".to_string(),
            primary_contributor: author.map(str::to_string),
            isbn: Some("9782203001190".to_string()),
        }
    }

    #[test]
    fn a_candidate_is_named_by_title_then_author() {
        assert_eq!(
            candidate_display(&candidate(Some("Hergé"))),
            "Les Bijoux <de> la Castafiore — Hergé"
        );
        assert_eq!(
            candidate_display(&candidate(None)),
            "Les Bijoux <de> la Castafiore"
        );
        assert_eq!(
            candidate_display(&candidate(Some("  "))),
            "Les Bijoux <de> la Castafiore"
        );
    }

    #[test]
    fn candidates_are_escaped_and_point_at_the_selection() {
        let html = VolumeMoveCandidatesTemplate {
            volume_id: 9,
            query_empty: false,
            candidates: vec![candidate(Some("<b>Hergé</b>"))],
            empty_label: "none".to_string(),
        }
        .render()
        .unwrap();
        assert!(
            html.contains("Les Bijoux &#60;de&#62; la Castafiore")
                || html.contains("Les Bijoux &lt;de&gt; la Castafiore"),
            "{html}"
        );
        assert!(!html.contains("<b>Hergé</b>"), "{html}");
        assert!(html.contains(r#"hx-get="/volume/9/move/selection?title_id=5""#));
        assert!(html.contains("9782203001190"));
    }

    #[test]
    fn an_empty_query_lists_nothing_and_no_match_says_so() {
        let empty = VolumeMoveCandidatesTemplate {
            volume_id: 9,
            query_empty: true,
            candidates: vec![],
            empty_label: "No other title".to_string(),
        }
        .render()
        .unwrap();
        assert!(!empty.contains("No other title"));
        let none = VolumeMoveCandidatesTemplate {
            volume_id: 9,
            query_empty: false,
            candidates: vec![],
            empty_label: "No other title".to_string(),
        }
        .render()
        .unwrap();
        assert!(none.contains("No other title"));
    }

    #[test]
    fn a_selection_carries_the_hidden_title_id_only_when_picked() {
        let picked = VolumeMoveSelectionTemplate {
            title_id: Some(5),
            text: "V0001 will move from “A” to “<B>”.".to_string(),
        }
        .render()
        .unwrap();
        assert!(picked.contains(r#"<input type="hidden" name="title_id" value="5">"#));
        assert!(!picked.contains("<B>"));
        let none = VolumeMoveSelectionTemplate {
            title_id: None,
            text: "No title chosen yet.".to_string(),
        }
        .render()
        .unwrap();
        assert!(!none.contains("name=\"title_id\""));
    }

    #[test]
    fn the_modal_is_a_pattern_a_dialog_posting_the_move() {
        let html = VolumeMoveModalTemplate {
            title: "Move volume V0001".to_string(),
            body_html: "<p>picker</p>".to_string(),
            confirm_label: "Move".to_string(),
            cancel_label: "Cancel".to_string(),
            action_url: "/volume/1/move".to_string(),
            csrf_token: "tok".to_string(),
            version: 3,
            inner_form_html: r#"<div id="volume-move-selection"></div>"#.to_string(),
        }
        .render()
        .unwrap();
        assert!(html.contains(r#"<dialog open aria-modal="true""#));
        assert!(html.contains(r#"hx-post="/volume/1/move""#));
        assert!(html.contains(r##"hx-target="#feedback-list""##));
        assert!(html.contains(r#"name="version" value="3""#));
        assert!(html.contains(r#"name="_csrf_token" value="tok""#));
        assert!(!html.contains("hx-confirm"));
        // The picker sits before the confirm form, the selection inside it.
        let form_at = html.find("<form").unwrap();
        assert!(html.find("<p>picker</p>").unwrap() < form_at);
        assert!(html.find("volume-move-selection").unwrap() > form_at);
    }

    #[test]
    fn the_picker_focuses_its_search_field_and_queries_candidates() {
        let html = VolumeMovePickerTemplate {
            volume_id: 4,
            intro: "V0001 is under “<A>”.".to_string(),
            search_label: "Title or ISBN".to_string(),
            search_placeholder: "Search".to_string(),
        }
        .render()
        .unwrap();
        assert!(html.contains("data-modal-default-focus"));
        assert!(html.contains(r#"hx-get="/volume/4/move/candidates""#));
        assert!(!html.contains("“<A>”"));
    }
}
