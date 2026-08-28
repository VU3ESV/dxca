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

/// Telnet login backed by the same accounts as the web UI
/// (`docs/TELNET-INTERACTIVE.md` milestone 2).
///
/// Deliberately no session tokens: a telnet connection *is* the session, and
/// it ends when the socket does.
pub struct DbAuthenticator {
    db: std::sync::Arc<Db>,
}

impl DbAuthenticator {
    pub fn new(db: std::sync::Arc<Db>) -> Self {
        DbAuthenticator { db }
    }
}

impl dxca_connect::telnet::Authenticator for DbAuthenticator {
    fn authenticate(
        &self,
        callsign: &str,
        password: &str,
    ) -> Option<dxca_connect::telnet::TelnetIdentity> {
        let found = self.db.user_by_callsign(callsign).ok().flatten();
        let Some((user, hash)) = found else {
            // Verify against a throwaway hash anyway. Skipping the argon2
            // work for an unknown callsign would return in microseconds
            // instead of ~100 ms, which tells an attacker on the LAN which
            // callsigns hold accounts here.
            let _ = verify_password(password, DUMMY_HASH);
            return None;
        };
        if !verify_password(password, &hash) {
            return None;
        }
        Some(dxca_connect::telnet::TelnetIdentity {
            user_id: user.id,
            callsign: user.callsign,
            role: user.role,
        })
    }
}

/// A real argon2 hash (of a value nothing can log in with) used solely to
/// spend the same time on an unknown callsign as on a known one.
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$hR8dY7pO0T7v8Vz2yNDpNMEDIz3PYHBLnnHK/vZUKvI";

#[cfg(test)]
mod telnet_auth_tests {
    use super::*;

    /// The unknown-callsign timing defence only works if the dummy hash is
    /// a *parseable* argon2 hash. A malformed one makes `verify_password`
    /// bail out before doing any work, which is exactly the fast path the
    /// constant is there to avoid — and it would fail silently.
    #[test]
    fn dummy_hash_is_real_enough_to_cost_time() {
        assert!(
            PasswordHash::new(DUMMY_HASH).is_ok(),
            "DUMMY_HASH must parse or the timing defence is a no-op"
        );
        assert!(!verify_password("anything", DUMMY_HASH));

        // And it costs roughly what a real verification costs.
        let real = hash_password("correct horse").unwrap();
        let t0 = std::time::Instant::now();
        let _ = verify_password("wrong", &real);
        let known = t0.elapsed();
        let t1 = std::time::Instant::now();
        let _ = verify_password("wrong", DUMMY_HASH);
        let unknown = t1.elapsed();
        let ratio = unknown.as_secs_f64() / known.as_secs_f64().max(1e-9);
        assert!(
            (0.2..5.0).contains(&ratio),
            "unknown-callsign path took {unknown:?} vs {known:?} for a known one"
        );
    }
}
