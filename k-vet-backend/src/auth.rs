//! Single-user authentication: argon2 credentials from the config file, server-side
//! sessions in Postgres (research R2).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use utoipa::ToSchema;

use crate::AppState;
use crate::error::{AppError, AppResult};

/// Session key holding the logged-in user name.
const SESSION_USER_KEY: &str = "username";
/// Failed logins allowed inside [`THROTTLE_WINDOW`] before the endpoint answers 429.
const MAX_FAILED_LOGINS: u32 = 10;
const THROTTLE_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionInfo {
    /// Whether the caller has a valid session.
    pub authenticated: bool,
    /// Name of the logged-in user, when authenticated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

/// Counts failed login attempts per source so brute forcing a single-user login is pointless.
#[derive(Debug, Default)]
pub struct LoginThrottle {
    attempts: Mutex<HashMap<String, (u32, Instant)>>,
}

impl LoginThrottle {
    /// Returns `false` when the caller has to wait before trying again.
    fn allow(&self, key: &str) -> bool {
        let Ok(mut attempts) = self.attempts.lock() else {
            // A poisoned lock must not lock the vet out of their own practice.
            return true;
        };
        match attempts.get(key) {
            Some((count, since)) if since.elapsed() < THROTTLE_WINDOW => *count < MAX_FAILED_LOGINS,
            Some(_) => {
                attempts.remove(key);
                true
            }
            None => true,
        }
    }

    fn record_failure(&self, key: &str) {
        let Ok(mut attempts) = self.attempts.lock() else {
            return;
        };
        let entry = attempts
            .entry(key.to_owned())
            .or_insert((0, Instant::now()));
        if entry.1.elapsed() >= THROTTLE_WINDOW {
            *entry = (0, Instant::now());
        }
        entry.0 = entry.0.saturating_add(1);
    }

    fn reset(&self, key: &str) {
        if let Ok(mut attempts) = self.attempts.lock() {
            attempts.remove(key);
        }
    }
}

/// Hashes a password with argon2id using the default (memory-hard) parameters.
pub fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| AppError::internal("hashing password", error))
}

/// Verifies a password against an argon2 encoded hash.
fn verify_password(password: &str, encoded_hash: &str) -> bool {
    match PasswordHash::new(encoded_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(error) => {
            tracing::error!(%error, "configured password hash cannot be parsed");
            false
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Session established", body = SessionInfo),
        (status = 401, description = "Invalid credentials"),
        (status = 429, description = "Too many failed attempts")
    )
)]
pub async fn login(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<LoginRequest>,
) -> AppResult<Json<SessionInfo>> {
    let throttle_key = body.username.to_lowercase();
    if !state.login_throttle.allow(&throttle_key) {
        return Err(AppError::BadRequest(
            "too many failed login attempts".to_owned(),
        ));
    }

    let expected_user = state.config.auth.username.as_str();
    let user_matches = body.username == expected_user;
    // Verify regardless of the user name so timing does not reveal whether it exists.
    let password_matches = verify_password(&body.password, &state.config.auth.password_hash);

    if !(user_matches && password_matches) {
        state.login_throttle.record_failure(&throttle_key);
        tracing::warn!(username = %body.username, "failed login attempt");
        return Err(AppError::Unauthorized);
    }

    state.login_throttle.reset(&throttle_key);
    session
        .insert(SESSION_USER_KEY, expected_user.to_owned())
        .await
        .map_err(|error| AppError::internal("storing session", error))?;
    session
        .cycle_id()
        .await
        .map_err(|error| AppError::internal("cycling session id", error))?;

    Ok(Json(SessionInfo {
        authenticated: true,
        username: Some(expected_user.to_owned()),
    }))
}

#[utoipa::path(
    post,
    path = "/api/auth/logout",
    tag = "auth",
    responses((status = 200, description = "Session destroyed", body = SessionInfo))
)]
pub async fn logout(session: Session) -> AppResult<Json<SessionInfo>> {
    session
        .flush()
        .await
        .map_err(|error| AppError::internal("destroying session", error))?;
    Ok(Json(SessionInfo {
        authenticated: false,
        username: None,
    }))
}

#[utoipa::path(
    get,
    path = "/api/auth/session",
    tag = "auth",
    responses((status = 200, description = "Current session", body = SessionInfo))
)]
pub async fn session_info(session: Session) -> AppResult<Json<SessionInfo>> {
    let username = current_user(&session).await?;
    Ok(Json(SessionInfo {
        authenticated: username.is_some(),
        username,
    }))
}

async fn current_user(session: &Session) -> AppResult<Option<String>> {
    session
        .get::<String>(SESSION_USER_KEY)
        .await
        .map_err(|error| AppError::internal("reading session", error))
}

/// Rejects unauthenticated requests to `/api/*` (everything except the auth endpoints).
pub async fn require_auth(session: Session, request: Request, next: Next) -> AppResult<Response> {
    match current_user(&session).await? {
        Some(_) => Ok(next.run(request).await),
        None => Err(AppError::Unauthorized),
    }
}

/// The three auth endpoints, which are reachable without a session.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/session", get(session_info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_verify_against_their_password() {
        let hash = hash_password("test1234").expect("hashing works");
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("test1234", &hash));
        assert!(!verify_password("test1235", &hash));
    }

    #[test]
    fn malformed_hash_never_authenticates() {
        assert!(!verify_password("test1234", "not-a-hash"));
    }

    #[test]
    fn throttle_blocks_after_too_many_failures() {
        let throttle = LoginThrottle::default();
        assert!(throttle.allow("vet"));
        for _ in 0..MAX_FAILED_LOGINS {
            throttle.record_failure("vet");
        }
        assert!(!throttle.allow("vet"));
        throttle.reset("vet");
        assert!(throttle.allow("vet"));
    }
}
