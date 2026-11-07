mod accounts;
mod attempts;
mod devices;
mod proof;
mod revocations;

pub use accounts::bootstrap;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    error::Error,
    sync::{Arc, Mutex},
};

use url::{Host, Url};

struct Auth {
    pool: PgPool,
    origin: String,
    secure: bool,

    attempts: Mutex<attempts::Attempts>,
}

pub fn routes(pool: PgPool, origin: &str) -> Result<Router, Box<dyn Error>> {
    let url = Url::parse(origin)?;
    let secure = url.scheme() == "https";
    let local = match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain("localhost")) => true,
        _ => false,
    };
    if url.origin().ascii_serialization() != origin
        || !(secure || (local && url.scheme() == "http"))
    {
        return Err(
            "KOVA_ORIGIN doit être une origine HTTPS exacte ou HTTP de boucle locale.".into(),
        );
    }
    let auth = Arc::new(Auth {
        pool,
        origin: origin.to_owned(),
        secure,

        attempts: Mutex::new(attempts::Attempts::new()),
    });
    Ok(Router::new()
        .route("/auth/challenges", post(proof::challenge))
        .route("/auth/verify", post(proof::verify))
        .route("/auth/logout", post(logout))
        .route("/me", get(me))
        .route("/identity/devices", get(devices::list))
        .route(
            "/identity/revocations",
            get(revocations::list).post(revocations::publish),
        )
        .route("/identity/devices/revoke", post(devices::revoke))
        .layer(DefaultBodyLimit::max(8192))
        .layer(middleware::from_fn_with_state(auth.clone(), protect))
        .with_state(auth))
}

async fn protect(State(auth): State<Arc<Auth>>, request: Request, next: Next) -> Response {
    let mut origins = request.headers().get_all(header::ORIGIN).iter();
    let mut response = if request.method() == Method::POST
        && (origins.next().map(|v| v.as_bytes()) != Some(auth.origin.as_bytes())
            || origins.next().is_some())
    {
        AuthError::Origin.into_response()
    } else {
        next.run(request).await
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

#[derive(Debug)]
enum AuthError {
    Invalid,
    Session,
    Credentials,
    Origin,
    Limited,
    Invite,
    Conflict,
    Internal,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        use AuthError::*;
        let (status, code, message) = match self {
            Invalid => (
                StatusCode::BAD_REQUEST,
                "invalid_body",
                "Corps de requête invalide.",
            ),
            Session => (
                StatusCode::UNAUTHORIZED,
                "invalid_session",
                "Session absente ou expirée.",
            ),
            Credentials => (
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "Identifiants invalides.",
            ),
            Origin => (StatusCode::FORBIDDEN, "invalid_origin", "Origine refusée."),
            Limited => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Réessayez plus tard.",
            ),
            Invite => (
                StatusCode::BAD_REQUEST,
                "invalid_invite",
                "Invitation invalide ou expirée.",
            ),
            Conflict => (
                StatusCode::CONFLICT,
                "handle_taken",
                "Identifiant local déjà utilisé.",
            ),
            Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Erreur interne.",
            ),
        };
        (
            status,
            Json(serde_json::json!({"error":{"code":code,"message":message}})),
        )
            .into_response()
    }
}
impl From<sqlx::Error> for AuthError {
    fn from(_: sqlx::Error) -> Self {
        Self::Internal
    }
}

#[derive(Serialize, sqlx::FromRow)]
struct User {
    global_identity_id: String,
    id: String,
    handle: String,
    display_name: String,
}

fn cookie(auth: &Auth, value: &str, age: u32) -> String {
    let secure = if auth.secure { "; Secure" } else { "" };
    format!("kova_session={value}; HttpOnly; SameSite=Lax; Path=/; Max-Age={age}{secure}")
}

fn session_credentials(headers: &HeaderMap) -> Result<(Vec<u8>, &'static str), AuthError> {
    let auth_values: Vec<_> = headers.get_all(header::AUTHORIZATION).iter().collect();
    let cookies: Vec<_> = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().strip_prefix("kova_session="))
        .collect();
    let (value, transport) = match (auth_values.as_slice(), cookies.as_slice()) {
        ([authorization], []) => (
            authorization
                .to_str()
                .ok()
                .and_then(|v| v.strip_prefix("Bearer "))
                .ok_or(AuthError::Session)?,
            "bearer",
        ),
        ([], [cookie]) => (*cookie, "cookie"),
        _ => return Err(AuthError::Session),
    };
    let mut token = [0; 32];
    hex::decode_to_slice(value, &mut token).map_err(|_| AuthError::Session)?;
    Ok((Sha256::digest(token).to_vec(), transport))
}

async fn me(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Result<Json<User>, AuthError> {
    let (hash, transport) = session_credentials(&headers)?;
    sqlx::query_as("SELECT u.id::text, u.global_identity_id, u.handle, u.display_name FROM sessions s JOIN users u ON u.id=s.user_id JOIN devices d ON (d.identity_id,d.device_id)=(s.identity_id,s.device_id) WHERE d.revoked_at IS NULL AND s.token_hash=$1 AND s.expires_at > now() AND s.transport=$2")
        .bind(hash).bind(transport).fetch_optional(&auth.pool).await?.map(Json).ok_or(AuthError::Session)
}

async fn logout(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AuthError> {
    if !body.is_empty() {
        return Err(AuthError::Invalid);
    }
    let (hash, transport) = session_credentials(&headers)?;
    let deleted = sqlx::query(
        "DELETE FROM sessions WHERE token_hash=$1 AND expires_at > now() AND transport=$2",
    )
    .bind(hash)
    .bind(transport)
    .execute(&auth.pool)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AuthError::Session);
    }
    if transport == "bearer" {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie(&auth, "", 0))],
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_ambiguous_authorization_headers() {
        let mut headers = HeaderMap::new();
        let bearer = format!("Bearer {}", "01".repeat(32)).parse().unwrap();
        headers.append(header::AUTHORIZATION, bearer);
        assert_eq!(session_credentials(&headers).unwrap().1, "bearer");
        headers.append(
            header::AUTHORIZATION,
            format!("Bearer {}", "02".repeat(32)).parse().unwrap(),
        );
        assert!(session_credentials(&headers).is_err());
    }
}
