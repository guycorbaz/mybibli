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
        } else {
            Err(AppError::Unauthorized)
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

        match SessionModel::find_with_role(pool, token).await {
            Ok(Some(row)) => {
                // Update last activity (fire and forget)
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
    fn test_require_role_anonymous_fails() {
        let session = Session::anonymous();
        assert!(session.require_role(Role::Librarian).is_err());
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
