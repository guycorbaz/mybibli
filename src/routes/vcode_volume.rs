//! The V-code arm shared by `/catalog/scan` and the title page (story 17-2,
//! #652): read the code, answer an existing label (#585), ask the CR #300
//! confirmation when the title already has volumes, create the volume and
//! word its refusals. One copy of that logic (DRY).
//!
//! What depends on the catalog scanning session — the title in context, the
//! shelve of an existing label at the active location, the auto-shelve, the
//! session counter, the last label, the context banner — stays in
//! `routes/catalog.rs`. The title page names its own title and uses none of
//! it (story 17-2 D2). Each caller renders its own success.
//!
//! Every message is worded in the request locale `loc` (#661).

use askama::Template;
use axum::response::{Html, IntoResponse, Response};

use crate::db::DbPool;
use crate::error::AppError;
use crate::middleware::auth::Session;
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;
use crate::routes::catalog_volumes::VolumeConfirmAddModalTemplate;
use crate::services::volume::{VolumeCreation, VolumeService};
use crate::utils::{feedback_html, html_escape};

/// The canonical V-code of `input` (`v0042` → `V0042`), or `None` when it
/// is not `[vV]` + 4 ASCII digits or is `V0000` (Rule #20: nothing else is
/// rewritten).
pub fn read_vcode(input: &str) -> Option<String> {
    VolumeService::normalize_vcode(input.trim()).filter(|c| VolumeService::validate_vcode(c))
}

/// The refusal of an input that is not a V-code: names the expected shape
/// (Rule #9).
pub fn invalid_vcode_response(loc: &str) -> Response {
    let message = rust_i18n::t!("feedback.vcode_invalid", locale = loc).to_string();
    Html(feedback_html("error", &message, "")).into_response()
}

/// Where the CR #300 modal's Confirm re-posts and where its answer lands.
#[derive(Debug, Clone, Copy)]
pub struct ConfirmTarget<'a> {
    pub action_url: &'a str,
    pub hx_target: &'a str,
    pub hx_swap: &'a str,
}

/// What the shared arm did.
#[derive(Debug)]
pub enum VcodeAdd {
    /// The answer is complete: an existing label, the CR #300 modal, or a
    /// refusal of the input.
    Answered(Response),
    /// The title vanished before the volume could be attached (#441).
    TitleGone,
    /// The volume now labels a volume of the title.
    Created(VolumeCreation),
}

/// Add the volume labelled `code` (already canonical, see [`read_vcode`]) to
/// `title_id`. `existing` is the live volume carrying the label, if any.
/// `confirmed` is the CR #300 bypass set by the modal's Confirm.
#[allow(clippy::too_many_arguments)]
pub async fn add_to_title(
    pool: &DbPool,
    session: &Session,
    code: &str,
    title_id: u64,
    existing: Option<&VolumeModel>,
    confirmed: bool,
    confirm: ConfirmTarget<'_>,
    loc: &str,
) -> Result<VcodeAdd, AppError> {
    // #585 — the label already labels a live volume: never try to create it
    // again; say whose it is instead.
    if let Some(existing) = existing {
        let resp = super::catalog_existing_label::existing_label_response(
            pool, existing, title_id, loc,
        )
        .await?;
        return Ok(VcodeAdd::Answered(closing_modal(resp, confirmed)));
    }

    // CR #300: when the title already has volumes and the librarian has not
    // confirmed, ask before attaching another copy. Only reached for a
    // genuinely new label (an existing one returned above).
    if !confirmed {
        // A failed count must not skip the confirmation (fail closed).
        let already_has = VolumeModel::count_by_title(pool, title_id).await?;
        if already_has >= 1 {
            let Some(title) = TitleModel::find_by_id(pool, title_id).await? else {
                return Ok(VcodeAdd::TitleGone);
            };
            let html = confirm_modal_html(session, code, &title.title, already_has, confirm, loc)?;
            return Ok(VcodeAdd::Answered(
                (
                    axum::http::StatusCode::OK,
                    [("HX-Retarget", "#modal-slot"), ("HX-Reswap", "innerHTML")],
                    Html(html),
                )
                    .into_response(),
            ));
        }
    }

    match VolumeService::create_volume_in(pool, code, title_id, session.user_id, loc).await {
        Ok(creation) => Ok(VcodeAdd::Created(creation)),
        Err(e) => {
            // #585 — a refusal of the librarian's input is not a server
            // fault: warn, keep `error` for the rest.
            if matches!(e, AppError::BadRequest(_) | AppError::Conflict(_)) {
                tracing::warn!(error = %e, code = %code, "V-code scan refused");
            } else {
                tracing::error!(error = %e, code = %code, "V-code scan failed");
            }
            let (level, message) = match e {
                AppError::BadRequest(msg) => ("error", msg),
                // Already user-facing copy (e.g. volume_label_race).
                AppError::Conflict(msg) => ("warning", msg),
                // #441 — the title was deleted between the caller's check
                // and the INSERT.
                AppError::NotFound(_) => return Ok(VcodeAdd::TitleGone),
                _ => ("error", rust_i18n::t!("error.internal", locale = loc).to_string()),
            };
            Ok(VcodeAdd::Answered(closing_modal(
                Html(feedback_html(level, &message, "")).into_response(),
                confirmed,
            )))
        }
    }
}

/// An answer to the CR #300 modal's Confirm (`confirmed`) closes the modal
/// too, after the swap: a refusal at that point (the label taken meanwhile,
/// a label in the Trash with loans) must not sit behind an open dialog.
pub fn closing_modal(mut resp: Response, confirmed: bool) -> Response {
    if confirmed {
        resp.headers_mut().insert(
            axum::http::HeaderName::from_static("hx-trigger-after-swap"),
            axum::http::HeaderValue::from_static("modal-close"),
        );
    }
    resp
}

/// The CR #300 confirmation modal (UX-DR8), re-posting `code` with
/// `confirmed=true` to `confirm.action_url`.
fn confirm_modal_html(
    session: &Session,
    code: &str,
    title_name: &str,
    already_has: u64,
    confirm: ConfirmTarget<'_>,
    loc: &str,
) -> Result<String, AppError> {
    let escaped_code = html_escape(code);
    let body_html = rust_i18n::t!(
        "feedback.volume_confirm.body",
        locale = loc,
        title = &html_escape(title_name),
        count = &already_has.to_string()
    )
    .to_string();
    let modal = VolumeConfirmAddModalTemplate {
        title: rust_i18n::t!("feedback.volume_confirm.title", locale = loc).to_string(),
        body_html,
        confirm_label: rust_i18n::t!("feedback.volume_confirm.confirm", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("feedback.volume_confirm.cancel", locale = loc).to_string(),
        csrf_token: session.csrf_token.clone(),
        inner_form_html: format!(
            r#"<input type="hidden" name="code" value="{escaped_code}"><input type="hidden" name="confirmed" value="true">"#
        ),
        action_url: confirm.action_url.to_string(),
        hx_target: confirm.hx_target.to_string(),
        hx_swap: confirm.hx_swap.to_string(),
    };
    modal
        .render()
        .map_err(|_| AppError::Internal("volume confirm modal render failed".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_vcode_accepts_either_case_and_trims() {
        assert_eq!(read_vcode("V0042").as_deref(), Some("V0042"));
        assert_eq!(read_vcode("v0042").as_deref(), Some("V0042"));
        assert_eq!(read_vcode("  v0042 \n").as_deref(), Some("V0042"));
    }

    #[test]
    fn read_vcode_refuses_every_other_shape() {
        for input in ["", "V0000", "v0000", "V12345", "V12A4", "9782070360246", "L0001", "Vol", "V 0042"] {
            assert_eq!(read_vcode(input), None, "{input}");
        }
    }

    #[test]
    fn invalid_vcode_names_the_expected_shape_in_the_request_locale() {
        let en = rust_i18n::t!("feedback.vcode_invalid", locale = "en").to_string();
        let fr = rust_i18n::t!("feedback.vcode_invalid", locale = "fr").to_string();
        assert_ne!(en, fr, "the key is translated");
        assert!(en.contains("V0042"));
    }

    #[test]
    fn an_answer_to_confirm_closes_the_modal_after_the_swap() {
        let closed = closing_modal(Html("x").into_response(), true);
        assert_eq!(closed.headers().get("hx-trigger-after-swap").unwrap(), "modal-close");
        let plain = closing_modal(Html("x").into_response(), false);
        assert!(plain.headers().get("hx-trigger-after-swap").is_none());
    }

    #[test]
    fn confirm_modal_posts_to_the_given_target() {
        let session = Session::anonymous_with_token("tok".to_string());
        let html = confirm_modal_html(
            &session,
            "V0099",
            "<Tintin>",
            2,
            ConfirmTarget {
                action_url: "/title/7/volumes",
                hx_target: "#title-volume-add-feedback",
                hx_swap: "innerHTML",
            },
            "en",
        )
        .unwrap();
        assert!(html.contains(r#"hx-post="/title/7/volumes""#), "{html}");
        assert!(html.contains(r##"hx-target="#title-volume-add-feedback""##), "{html}");
        assert!(html.contains(r#"name="code" value="V0099""#), "{html}");
        assert!(html.contains(r#"name="confirmed" value="true""#), "{html}");
        assert!(html.contains("&lt;Tintin&gt;"), "title escaped: {html}");
        assert!(!html.contains("/catalog/scan"), "{html}");
    }
}
