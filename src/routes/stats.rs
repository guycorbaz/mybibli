//! CR #243 — `/stats/value` page handler.
//!
//! Librarian-gated (values are owner-facing — Anonymous sees a 303 to
//! /login). Renders three sections: per-currency totals,
//! per-genre breakdown, per-series breakdown. The home dashboard's
//! opt-in "Library estimated value" indicator links here.

use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, State};
use axum::response::{Html, IntoResponse};

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::locale::Locale;
use crate::models::volume::{
    ValueByGenreRow, ValueBySeriesRow, ValueTotalRow, VolumeModel,
};

#[derive(Template)]
#[template(path = "pages/stats_value.html")]
struct StatsValueTemplate {
    base: crate::utils::BaseContextFields,

    page_heading: String,
    page_intro: String,
    section_totals: String,
    section_by_genre: String,
    section_by_series: String,
    col_currency: String,
    col_total_value: String,
    col_total_purchase: String,
    col_count: String,
    col_genre: String,
    col_series: String,
    col_volumes: String,
    empty_state: String,

    totals: Vec<TotalRowDisplay>,
    by_genre: Vec<GenreRowDisplay>,
    by_series: Vec<SeriesRowDisplay>,
    has_any_data: bool,
}

struct TotalRowDisplay {
    currency: String,
    total_current_value: String,
    total_purchase_price: String,
    current_value_count: i64,
    purchase_price_count: i64,
}

struct GenreRowDisplay {
    genre_label: String,
    currency: String,
    total: String,
    volume_count: i64,
}

struct SeriesRowDisplay {
    series_id: u64,
    series_name: String,
    currency: String,
    total: String,
    volume_count: i64,
}

/// Format an amount with 2 decimals, locale-aware separator. Mirrors
/// the home-dashboard percent formatter's locale split (FR uses
/// comma + NBSP grouping; EN uses dot + comma grouping).
fn format_amount(value: f64, loc: &str) -> String {
    let raw = format!("{value:.2}");
    if loc == "fr" {
        raw.replace('.', ",")
    } else {
        raw
    }
}

/// `GET /stats/value` — Librarian+. Anonymous gets 303 to /login.
pub async fn stats_value_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let totals_raw: Vec<ValueTotalRow> = VolumeModel::value_totals_by_currency(pool).await?;
    let by_genre_raw: Vec<ValueByGenreRow> = VolumeModel::value_by_genre(pool).await?;
    let by_series_raw: Vec<ValueBySeriesRow> = VolumeModel::value_by_series(pool).await?;

    let deleted_genre_placeholder =
        rust_i18n::t!("stats.value.deleted_genre", locale = loc).to_string();

    let totals: Vec<TotalRowDisplay> = totals_raw
        .into_iter()
        .map(|r| TotalRowDisplay {
            currency: r.currency.clone(),
            total_current_value: format_amount(r.total_current_value, loc),
            total_purchase_price: format_amount(r.total_purchase_price, loc),
            current_value_count: r.current_value_count,
            purchase_price_count: r.purchase_price_count,
        })
        .collect();

    let by_genre: Vec<GenreRowDisplay> = by_genre_raw
        .into_iter()
        .map(|r| {
            let label = match (r.genre_id, r.genre_name.as_ref()) {
                (Some(_), Some(name)) if !name.is_empty() => name.clone(),
                _ => deleted_genre_placeholder.clone(),
            };
            GenreRowDisplay {
                genre_label: label,
                currency: r.currency,
                total: format_amount(r.total_current_value, loc),
                volume_count: r.volume_count,
            }
        })
        .collect();

    let by_series: Vec<SeriesRowDisplay> = by_series_raw
        .into_iter()
        .map(|r| SeriesRowDisplay {
            series_id: r.series_id,
            series_name: r.series_name,
            currency: r.currency,
            total: format_amount(r.total_current_value, loc),
            volume_count: r.volume_count,
        })
        .collect();

    let has_any_data = !totals.is_empty();

    let base = crate::utils::base_context(&session, loc, "stats", &uri, state.session_timeout_secs());
    let template = StatsValueTemplate {
        base,

        page_heading: rust_i18n::t!("stats.value.heading", locale = loc).to_string(),
        page_intro: rust_i18n::t!("stats.value.intro", locale = loc).to_string(),
        section_totals: rust_i18n::t!("stats.value.section_totals", locale = loc).to_string(),
        section_by_genre: rust_i18n::t!("stats.value.section_by_genre", locale = loc).to_string(),
        section_by_series: rust_i18n::t!("stats.value.section_by_series", locale = loc)
            .to_string(),
        col_currency: rust_i18n::t!("stats.value.col_currency", locale = loc).to_string(),
        col_total_value: rust_i18n::t!("stats.value.col_total_value", locale = loc).to_string(),
        col_total_purchase: rust_i18n::t!("stats.value.col_total_purchase", locale = loc)
            .to_string(),
        col_count: rust_i18n::t!("stats.value.col_count", locale = loc).to_string(),
        col_genre: rust_i18n::t!("stats.value.col_genre", locale = loc).to_string(),
        col_series: rust_i18n::t!("stats.value.col_series", locale = loc).to_string(),
        col_volumes: rust_i18n::t!("stats.value.col_volumes", locale = loc).to_string(),
        empty_state: rust_i18n::t!("stats.value.empty_state", locale = loc).to_string(),

        totals,
        by_genre,
        by_series,
        has_any_data,
    };

    template
        .render()
        .map(|html| Html(html).into_response())
        .map_err(|e| AppError::Internal(format!("stats value page render: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_amount_en_uses_dot() {
        assert_eq!(format_amount(123.4, "en"), "123.40");
        assert_eq!(format_amount(0.0, "en"), "0.00");
        assert_eq!(format_amount(1_234_567.89, "en"), "1234567.89");
    }

    #[test]
    fn format_amount_fr_uses_comma() {
        assert_eq!(format_amount(123.4, "fr"), "123,40");
        assert_eq!(format_amount(0.0, "fr"), "0,00");
    }
}
