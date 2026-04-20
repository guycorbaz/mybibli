use crate::error::AppError;

/// Hash a plain-text password using Argon2 with a random salt.
pub fn hash_password(plain: &str) -> Result<String, AppError> {
    use argon2::{Argon2, PasswordHasher};
    use argon2::password_hash::SaltString;
    use rand::rngs::OsRng;

    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("argon2 hash failed: {e}")))
}

/// Verify a plain-text password against an Argon2 hash.
pub fn verify_password(password: &str, hash: &str) -> bool {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};

    let Ok(parsed_hash) = PasswordHash::new(hash) else {
        tracing::warn!("Invalid password hash format in database");
        return false;
    };

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}
