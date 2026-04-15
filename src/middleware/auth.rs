use axum::extract::FromRequestParts;
use axum::http::request;
use axum_extra::extract::CookieJar;
use std::convert::Infallible;

use crate::error::AppError;
use crate::models::session::SessionModel;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Role {
    Anonymous,
    Librarian,
    Admin,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Anonymous => write!(f, "anonymous"),
            Role::Librarian => write!(f, "librarian"),
            Role::Admin => write!(f, "admin"),
        }
    }
}

impl Role {
    pub fn from_db(s: &str) -> Self {
        match s {
            "admin" => Role::Admin,
            "librarian" => Role::Librarian,
            _ => Role::Anonymous,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub token: Option<String>,
    pub user_id: Option<u64>,
    pub role: Role,
}

impl Session {
    pub fn anonymous() -> Self {
        Session {
            token: None,
            user_id: None,
            role: Role::Anonymous,
        }
    }

    pub fn require_role(&self, min_role: Role) -> Result<(), AppError> {
        if self.role >= min_role {
            Ok(())
        } else if self.role == Role::Anonymous {
            Err(AppError::Unauthorized)
        } else {
            Err(AppError::Forbidden)
        }
    }

    /// Like `require_role`, but for GET handlers — if the user is Anonymous, the error
    /// preserves `return_path` so `/login` can bounce them back after sign-in.
    /// Authenticated-but-insufficient still produces `Forbidden` (no point returning to
    /// a page the user can't access anyway).
    pub fn require_role_with_return(
        &self,
        min_role: Role,
        return_path: &str,
    ) -> Result<(), AppError> {
        if self.role >= min_role {
            Ok(())
        } else if self.role == Role::Anonymous {
            Err(AppError::UnauthorizedWithReturn(return_path.to_string()))
        } else {
            Err(AppError::Forbidden)
        }
    }
}

impl FromRequestParts<crate::AppState> for Session {
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut request::Parts,
        state: &crate::AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, state)
            .await
            .unwrap_or_default();

        let Some(cookie) = jar.get("session") else {
            return Ok(Session::anonymous());
        };

        let token = cookie.value();
        let pool = &state.pool;

        // Clone the scalar out of the settings guard so it is NOT held across
        // the .await points below. Single fallback source via AppState helper.
        let timeout_secs = state.session_timeout_secs();

        match SessionModel::find_with_role(pool, token).await {
            Ok(Some(row)) => {
                let now = chrono::Utc::now();
                if SessionModel::is_expired(row.last_activity, now, timeout_secs) {
                    // Expired: soft-effect anonymous. Row stays; last_activity is
                    // NOT updated (so the session cannot revive itself).
                    return Ok(Session::anonymous());
                }

                // Valid session — refresh last_activity (fire and forget).
                let _ = SessionModel::update_last_activity(pool, token).await;

                Ok(Session {
                    token: Some(token.to_string()),
                    user_id: row.user_id,
                    role: Role::from_db(&row.role),
                })
            }
            _ => Ok(Session::anonymous()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_display() {
        assert_eq!(Role::Anonymous.to_string(), "anonymous");
        assert_eq!(Role::Librarian.to_string(), "librarian");
        assert_eq!(Role::Admin.to_string(), "admin");
    }

    #[test]
    fn test_role_from_db() {
        assert_eq!(Role::from_db("librarian"), Role::Librarian);
        assert_eq!(Role::from_db("admin"), Role::Admin);
        assert_eq!(Role::from_db("unknown"), Role::Anonymous);
    }

    #[test]
    fn test_role_ordering() {
        assert!(Role::Anonymous < Role::Librarian);
        assert!(Role::Librarian < Role::Admin);
    }

    #[test]
    fn test_require_role_librarian_ok() {
        let session = Session {
            token: Some("test".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
        };
        assert!(session.require_role(Role::Librarian).is_ok());
    }

    #[test]
    fn test_require_role_anonymous_returns_unauthorized() {
        let session = Session::anonymous();
        match session.require_role(Role::Librarian) {
            Err(AppError::Unauthorized) => {}
            other => panic!("expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn test_require_role_librarian_insufficient_returns_forbidden() {
        let session = Session {
            token: Some("t".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
        };
        match session.require_role(Role::Admin) {
            Err(AppError::Forbidden) => {}
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    #[test]
    fn test_require_role_with_return_anonymous_preserves_path() {
        let session = Session::anonymous();
        match session.require_role_with_return(Role::Librarian, "/loans") {
            Err(AppError::UnauthorizedWithReturn(next)) => {
                assert_eq!(next, "/loans");
            }
            other => panic!("expected UnauthorizedWithReturn, got {other:?}"),
        }
    }

    #[test]
    fn test_require_role_with_return_librarian_still_forbidden() {
        let session = Session {
            token: Some("t".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
        };
        match session.require_role_with_return(Role::Admin, "/admin") {
            Err(AppError::Forbidden) => {}
            other => panic!("expected Forbidden, got {other:?}"),
        }
    }

    /// AC #8 role × route matrix. For every combination of (user_role, min_role)
    /// assert the exact error variant (or Ok) so the Anonymous vs Forbidden split
    /// that drives the /login redirect vs 403 cannot regress silently.
    fn make_session(role: Role) -> Session {
        if role == Role::Anonymous {
            Session::anonymous()
        } else {
            Session {
                token: Some("t".to_string()),
                user_id: Some(1),
                role,
            }
        }
    }

    #[test]
    fn test_role_gating_matrix_anonymous_vs_librarian_min() {
        match make_session(Role::Anonymous).require_role(Role::Librarian) {
            Err(AppError::Unauthorized) => {}
            other => panic!("Anonymous/Librarian expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn test_role_gating_matrix_anonymous_vs_admin_min() {
        match make_session(Role::Anonymous).require_role(Role::Admin) {
            Err(AppError::Unauthorized) => {}
            other => panic!("Anonymous/Admin expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn test_role_gating_matrix_librarian_vs_librarian_min() {
        assert!(
            make_session(Role::Librarian)
                .require_role(Role::Librarian)
                .is_ok()
        );
    }

    #[test]
    fn test_role_gating_matrix_librarian_vs_admin_min() {
        match make_session(Role::Librarian).require_role(Role::Admin) {
            Err(AppError::Forbidden) => {}
            other => panic!("Librarian/Admin expected Forbidden, got {other:?}"),
        }
    }

    #[test]
    fn test_role_gating_matrix_admin_vs_librarian_min() {
        assert!(
            make_session(Role::Admin)
                .require_role(Role::Librarian)
                .is_ok()
        );
    }

    #[test]
    fn test_role_gating_matrix_admin_vs_admin_min() {
        assert!(make_session(Role::Admin).require_role(Role::Admin).is_ok());
    }

    // ─── Timeout boundary contract (AC 10 / Task 6) ─────────────
    // These exercise the logic the extractor runs on each request. The
    // extractor's side-effectful parts (DB + RwLock + fire-and-forget
    // update) are covered by E2E; here we pin the purely-computational
    // decision that turns a `SessionRow` + clock + timeout into a
    // Session::anonymous() vs an authenticated `Session`.
    fn decide(row_role: &str, last_activity_offset_secs: i64, timeout_secs: u64) -> Session {
        use crate::models::session::{SessionModel, SessionRow};
        let now = chrono::Utc::now();
        let row = SessionRow {
            token: "t".to_string(),
            user_id: Some(1),
            role: row_role.to_string(),
            last_activity: now - chrono::Duration::seconds(last_activity_offset_secs),
        };
        if SessionModel::is_expired(row.last_activity, now, timeout_secs) {
            Session::anonymous()
        } else {
            Session {
                token: Some(row.token),
                user_id: row.user_id,
                role: Role::from_db(&row.role),
            }
        }
    }

    #[test]
    fn test_extractor_decision_within_window_returns_librarian() {
        let s = decide("librarian", 30, 60);
        assert_eq!(s.role, Role::Librarian);
        assert!(s.token.is_some());
    }

    #[test]
    fn test_extractor_decision_past_timeout_returns_anonymous() {
        let s = decide("librarian", 90, 60);
        assert_eq!(s.role, Role::Anonymous);
        assert!(s.token.is_none());
    }

    #[test]
    fn test_extractor_decision_exact_boundary_still_authenticated() {
        // Elapsed == timeout → NOT expired (strict greater-than).
        let s = decide("admin", 60, 60);
        assert_eq!(s.role, Role::Admin);
    }

    #[test]
    fn test_require_role_admin_passes_librarian() {
        let session = Session {
            token: Some("test".to_string()),
            user_id: Some(1),
            role: Role::Admin,
        };
        assert!(session.require_role(Role::Librarian).is_ok());
    }
}
