//! Accounts and sessions (plan §5): argon2 password hashes, random
//! 256-bit session tokens in an HttpOnly cookie, sessions stored hashed in
//! SQLite with expiry. No JWT, no external identity — a LAN service.

use crate::db::{Db, User};
use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const COOKIE_NAME: &str = "dxca_session";
pub const SESSION_TTL_SECS: i64 = 30 * 24 * 3600;

pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("hash: {e}"))
}

pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    PasswordHash::new(stored_hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

fn token_hash(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    format!("{:x}", h.finalize())
}

/// Create a session for the user; returns the Set-Cookie value.
pub fn start_session(db: &Db, user_id: i64) -> Result<String, String> {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    db.create_session(&token_hash(&token), user_id, SESSION_TTL_SECS)?;
    Ok(format!(
        "{COOKIE_NAME}={token}; HttpOnly; Path=/; Max-Age={SESSION_TTL_SECS}; SameSite=Lax"
    ))
}

/// The Set-Cookie value that clears the session cookie.
pub fn clear_cookie() -> String {
    format!("{COOKIE_NAME}=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax")
}

/// Extract the session token from a Cookie header value.
fn token_from_cookie_header(header: &str) -> Option<&str> {
    header.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == COOKIE_NAME).then_some(value)
    })
}

/// Resolve the authenticated user from request headers, if any.
pub fn user_from_headers(db: &Db, headers: &axum::http::HeaderMap) -> Option<User> {
    let cookie = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    let token = token_from_cookie_header(cookie)?;
    db.session_user(&token_hash(token)).ok().flatten()
}

/// End the session named by the request's cookie, if any.
pub fn end_session(db: &Db, headers: &axum::http::HeaderMap) {
    if let Some(cookie) = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        && let Some(token) = token_from_cookie_header(cookie)
    {
        let _ = db.delete_session(&token_hash(token));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hashing_roundtrip() {
        let hash = hash_password("hunter2").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password("hunter2", &hash));
        assert!(!verify_password("wrong", &hash));
        assert!(!verify_password("hunter2", "not-a-hash"));
    }

    #[test]
    fn cookie_parsing() {
        assert_eq!(
            token_from_cookie_header("foo=1; dxca_session=abc123; bar=2"),
            Some("abc123")
        );
        assert_eq!(token_from_cookie_header("foo=1"), None);
    }
}
