mod accounts;
mod attempts;

pub use accounts::bootstrap;
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    error::Error,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;
use url::{Host, Url};

struct Auth {
    pool: PgPool,
    origin: String,
    secure: bool,
    dummy_hash: String,
    workers: Arc<Semaphore>,
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
        dummy_hash: accounts::hash_password("dummy")?,
        workers: Arc::new(Semaphore::new(2)),
        attempts: Mutex::new(attempts::Attempts::new()),
    });
    Ok(Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/me", get(me))
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

enum AuthError {
    Invalid,
    Session,
    Credentials,
    Origin,
    Limited,
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

#[derive(Deserialize)]
struct Login {
    handle: String,
    password: String,
}
#[derive(Serialize, sqlx::FromRow)]
struct User {
    id: String,
    handle: String,
    display_name: String,
}

fn cookie(auth: &Auth, value: &str, age: u32) -> String {
    let secure = if auth.secure { "; Secure" } else { "" };
    format!("kova_session={value}; HttpOnly; SameSite=Lax; Path=/; Max-Age={age}{secure}")
}

async fn login(
    State(auth): State<Arc<Auth>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Result<Json<Login>, JsonRejection>,
) -> Result<Response, AuthError> {
    let Json(login) = body.map_err(|_| AuthError::Invalid)?;
    if !accounts::valid_handle(&login.handle) || login.password.len() > 1024 {
        return Err(AuthError::Credentials);
    }
    if !auth
        .attempts
        .lock()
        .unwrap()
        .allow(peer.ip(), &login.handle)
    {
        return Err(AuthError::Limited);
    }
    let permit = auth
        .workers
        .clone()
        .try_acquire_owned()
        .map_err(|_| AuthError::Limited)?;
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT id::text, display_name, password_hash FROM users WHERE handle=$1")
            .bind(&login.handle)
            .fetch_optional(&auth.pool)
            .await?;
    let hash = row
        .as_ref()
        .map(|r| &r.2)
        .unwrap_or(&auth.dummy_hash)
        .clone();
    let verified = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&hash).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(login.password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .map_err(|_| AuthError::Internal)?;
    let (id, display_name, _) = row.filter(|_| verified).ok_or(AuthError::Credentials)?;
    let mut token = [0u8; 32];
    OsRng
        .try_fill_bytes(&mut token)
        .map_err(|_| AuthError::Internal)?;
    sqlx::query("INSERT INTO sessions (token_hash,user_id,expires_at) VALUES ($1,$2::text::uuid,now() + interval '168 hours')")
        .bind(Sha256::digest(token).to_vec()).bind(&id).execute(&auth.pool).await?;
    Ok((
        [(
            header::SET_COOKIE,
            cookie(&auth, &hex::encode(token), 604800),
        )],
        Json(User {
            id,
            handle: login.handle,
            display_name,
        }),
    )
        .into_response())
}

fn session_hash(headers: &HeaderMap) -> Result<Vec<u8>, AuthError> {
    let value = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| pair.trim().strip_prefix("kova_session="))
        .ok_or(AuthError::Session)?;
    let mut token = [0; 32];
    hex::decode_to_slice(value, &mut token).map_err(|_| AuthError::Session)?;
    Ok(Sha256::digest(token).to_vec())
}

async fn me(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Result<Json<User>, AuthError> {
    sqlx::query_as("SELECT u.id::text, u.handle, u.display_name FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.expires_at > now()")
        .bind(session_hash(&headers)?).fetch_optional(&auth.pool).await?.map(Json).ok_or(AuthError::Session)
}

async fn logout(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AuthError> {
    if !body.is_empty() {
        return Err(AuthError::Invalid);
    }
    let deleted = sqlx::query("DELETE FROM sessions WHERE token_hash=$1 AND expires_at > now()")
        .bind(session_hash(&headers)?)
        .execute(&auth.pool)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AuthError::Session);
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie(&auth, "", 0))],
    )
        .into_response())
}
