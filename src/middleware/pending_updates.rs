use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::db::DbPool;

/// Resolved pending metadata update row from the database.
struct PendingUpdate {
    title_id: u64,
    status: String,
    title_name: String,
    author_name: Option<String>,
    isbn: Option<String>,
}

/// Axum middleware that checks for resolved pending metadata updates
/// and appends OOB swap HTML to the response for skeleton replacement.
pub async fn pending_updates_middleware(request: Request, next: Next) -> Response {
    // Only process HTMX requests
    let is_htmx = request
        .headers()
        .get("hx-request")
        .and_then(|v| v.to_str().ok())
        .map(|s| s == "true")
        .unwrap_or(false);

    if !is_htmx {
        return next.run(request).await;
    }

    // Extract session token from cookie
    let session_token = request
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(extract_session_token);

    // Extract pool from extensions
    let pool = request.extensions().get::<DbPool>().cloned();

    let response = next.run(request).await;

    // If no session or pool, return response as-is
    let (Some(token), Some(pool)) = (session_token, pool) else {
        return response;
    };

    // Query resolved pending updates for this session
    let updates = match fetch_resolved_updates(&pool, &token).await {
        Ok(updates) if !updates.is_empty() => updates,
        _ => return response,
    };

    // Render OOB swaps and append to response body
    let oob_html = render_oob_updates(&updates);

    // Soft-delete processed rows
    let title_ids: Vec<u64> = updates.iter().map(|u| u.title_id).collect();
    if let Err(e) = soft_delete_processed(&pool, &title_ids, &token).await {
        tracing::warn!(error = %e, "Failed to soft-delete processed pending updates");
    }

    append_oob_to_response(response, &oob_html).await
}

/// Extract session token from cookie header.
fn extract_session_token(cookie_header: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix("session=")
            && !value.is_empty()
        {
            return Some(value.to_string());
        }
    }
    None
}

/// Query resolved pending metadata updates for a session.
async fn fetch_resolved_updates(
    pool: &DbPool,
    session_token: &str,
) -> Result<Vec<PendingUpdate>, sqlx::Error> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(u64, String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT p.title_id, p.status, t.title, \
         (SELECT c.name FROM title_contributors tc \
          JOIN contributors c ON c.id = tc.contributor_id AND c.deleted_at IS NULL \
          JOIN contributor_roles cr ON cr.id = tc.role_id AND cr.deleted_at IS NULL \
          WHERE tc.title_id = p.title_id AND tc.deleted_at IS NULL AND cr.name = 'Auteur' \
          LIMIT 1) as author_name, \
         t.isbn \
         FROM pending_metadata_updates p \
         JOIN titles t ON t.id = p.title_id AND t.deleted_at IS NULL \
         WHERE p.session_token = ? AND p.resolved_at IS NOT NULL AND p.deleted_at IS NULL",
    )
    .bind(session_token)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(title_id, status, title_name, author_name, isbn)| PendingUpdate {
                title_id,
                status,
                title_name,
                author_name,
                isbn,
            },
        )
        .collect())
}

/// Render OOB swap HTML for resolved pending updates.
fn render_oob_updates(updates: &[PendingUpdate]) -> String {
    let mut html = String::new();
    for update in updates {
        let entry_html = match update.status.as_str() {
            "resolved" => {
                let message = match &update.author_name {
                    Some(author) => rust_i18n::t!(
                        "feedback.metadata_resolved",
                        title = &update.title_name,
                        author = author
                    )
                    .to_string(),
                    None => rust_i18n::t!(
                        "feedback.metadata_resolved_no_author",
                        title = &update.title_name
                    )
                    .to_string(),
                };
                resolved_feedback_html(&message, update.title_id)
            }
            _ => {
                let isbn = update.isbn.as_deref().unwrap_or("?");
                let message = rust_i18n::t!("feedback.metadata_failed", isbn = isbn).to_string();
                failed_feedback_html(&message, update.title_id)
            }
        };
        html.push_str(&entry_html);
    }
    html
}

/// Render a success feedback entry for resolved metadata (OOB swap).
fn resolved_feedback_html(message: &str, title_id: u64) -> String {
    let escaped = html_escape(message);
    format!(
        r#"<div id="feedback-entry-{title_id}" hx-swap-oob="true" class="p-3 border-l-4 border-green-500 bg-green-50 dark:bg-green-900/20 rounded-r feedback-entry" role="status" data-feedback-variant="success" data-resolved-at="{ts}">
  <div class="flex items-start gap-2">
    <svg class="text-green-600 dark:text-green-400 w-5 h-5 flex-shrink-0 mt-0.5" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.857-9.809a.75.75 0 00-1.214-.882l-3.483 4.79-1.88-1.88a.75.75 0 10-1.06 1.061l2.5 2.5a.75.75 0 001.137-.089l4-5.5z" clip-rule="evenodd" /></svg>
    <div class="flex-1">
      <p class="text-stone-700 dark:text-stone-300">{escaped}</p>
    </div>
  </div>
</div>"#,
        ts = chrono::Utc::now().timestamp_millis()
    )
}

/// Render a warning feedback entry for failed metadata (OOB swap).
fn failed_feedback_html(message: &str, title_id: u64) -> String {
    let escaped = html_escape(message);
    let edit_label = rust_i18n::t!("feedback.edit_manually").to_string();
    format!(
        r#"<div id="feedback-entry-{title_id}" hx-swap-oob="true" class="p-3 border-l-4 border-amber-500 bg-amber-50 dark:bg-amber-900/20 rounded-r feedback-entry" role="status" data-feedback-variant="warning">
  <div class="flex items-start gap-2">
    <svg class="text-amber-600 dark:text-amber-400 w-5 h-5 flex-shrink-0 mt-0.5" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" d="M8.485 2.495c.673-1.167 2.357-1.167 3.03 0l6.28 10.875c.673 1.167-.17 2.625-1.516 2.625H3.72c-1.347 0-2.189-1.458-1.515-2.625L8.485 2.495zM10 5a.75.75 0 01.75.75v3.5a.75.75 0 01-1.5 0v-3.5A.75.75 0 0110 5zm0 9a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd" /></svg>
    <div class="flex-1">
      <p class="text-stone-700 dark:text-stone-300">{escaped}</p>
      <p class="text-sm mt-1"><a href="/title/{title_id}" class="text-indigo-600 dark:text-indigo-400 hover:underline">{edit_escaped}</a></p>
    </div>
    <button type="button" class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-200 p-1 min-w-[44px] min-h-[44px] md:min-w-[36px] md:min-h-[36px] flex items-center justify-center" aria-label="Dismiss" onclick="this.closest('.feedback-entry').remove()"><svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z" /></svg></button>
  </div>
</div>"#,
        edit_escaped = html_escape(&edit_label)
    )
}

use crate::utils::html_escape;

/// Soft-delete processed pending_metadata_updates rows (batched, scoped to session).
async fn soft_delete_processed(
    pool: &DbPool,
    title_ids: &[u64],
    session_token: &str,
) -> Result<(), sqlx::Error> {
    if title_ids.is_empty() {
        return Ok(());
    }
    let placeholders: Vec<&str> = title_ids.iter().map(|_| "?").collect();
    let query_str = format!(
        "UPDATE pending_metadata_updates SET deleted_at = NOW() \
         WHERE title_id IN ({}) AND session_token = ? AND resolved_at IS NOT NULL AND deleted_at IS NULL",
        placeholders.join(",")
    );
    let mut query = sqlx::query(&query_str);
    for id in title_ids {
        query = query.bind(id);
    }
    query = query.bind(session_token);
    query.execute(pool).await?;
    Ok(())
}

/// Append OOB HTML to an existing response body.
/// Collects the original body bytes, appends OOB, and returns a new response.
async fn append_oob_to_response(response: Response, oob_html: &str) -> Response {
    if oob_html.is_empty() {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    // Remove Content-Length since we're appending to the body
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);

    // Collect existing body bytes
    // Limit body read to 10 MB to prevent memory exhaustion
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to read response body for OOB append");
            return Response::from_parts(parts, Body::empty());
        }
    };

    // Append OOB HTML
    let mut combined = body_bytes.to_vec();
    combined.extend_from_slice(oob_html.as_bytes());

    Response::from_parts(parts, Body::from(combined))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_session_token() {
        let token = extract_session_token("session=abc123; other=value");
        assert_eq!(token, Some("abc123".to_string()));
    }

    #[test]
    fn test_extract_session_token_not_found() {
        let token = extract_session_token("other=value; foo=bar");
        assert_eq!(token, None);
    }

    #[test]
    fn test_extract_session_token_empty() {
        let token = extract_session_token("");
        assert_eq!(token, None);
    }

    #[test]
    fn test_extract_session_token_only_session() {
        let token = extract_session_token("session=xyz789");
        assert_eq!(token, Some("xyz789".to_string()));
    }

    #[test]
    fn test_resolved_feedback_html_contains_oob() {
        let html = resolved_feedback_html("Title resolved", 42);
        assert!(html.contains(r#"id="feedback-entry-42""#));
        assert!(html.contains("hx-swap-oob=\"true\""));
        assert!(html.contains("Title resolved"));
        assert!(html.contains("border-green-500"));
        assert!(html.contains("data-resolved-at="));
    }

    #[test]
    fn test_failed_feedback_html_contains_oob() {
        rust_i18n::set_locale("en");
        let html = failed_feedback_html("No metadata found", 99);
        assert!(html.contains(r#"id="feedback-entry-99""#));
        assert!(html.contains("hx-swap-oob=\"true\""));
        assert!(html.contains("No metadata found"));
        assert!(html.contains("border-amber-500"));
    }

    #[test]
    fn test_html_escape_in_feedback() {
        let html = resolved_feedback_html("<script>alert('xss')</script>", 1);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_render_oob_updates_empty() {
        let html = render_oob_updates(&[]);
        assert!(html.is_empty());
    }
}
