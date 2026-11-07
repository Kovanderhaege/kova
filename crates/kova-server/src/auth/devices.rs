use super::{Auth, AuthError, me};
use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::HeaderMap,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Serialize, sqlx::FromRow)]
struct Device {
    device_id: String,
    public_key: String,
    revoked: bool,
    authorization_payload: Option<String>,
    authorization_signature: Option<String>,
    authorization_expires_at: Option<i64>,
    revocation_signature: Option<String>,
}

pub(super) async fn list(
    state: State<Arc<Auth>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AuthError> {
    let Json(user) = me(state.clone(), headers).await?;
    let devices: Vec<Device> = sqlx::query_as("SELECT device_id, encode(public_key,'hex') AS public_key, revoked_at IS NOT NULL AS revoked, authorization_payload, encode(authorization_signature,'hex') AS authorization_signature, authorization_expires_at, encode(revocation_signature,'hex') AS revocation_signature FROM devices WHERE identity_id=$1 ORDER BY device_id")
        .bind(user.global_identity_id).fetch_all(&state.pool).await?;
    Ok(Json(json!({"items":devices})))
}

#[derive(Deserialize, Serialize)]
pub(super) struct Revocation {
    device_public_key: String,
    signature: String,
}

// A root-signed permanent revocation can be submitted again on each instance.
// Record unknown devices as well, so an older certificate cannot enroll them later.
pub(super) async fn revoke(
    state: State<Arc<Auth>>,
    headers: HeaderMap,
    body: Result<Json<Revocation>, JsonRejection>,
) -> Result<Json<Revocation>, AuthError> {
    let Json(user) = me(state.clone(), headers).await?;
    let Json(req) = body.map_err(|_| AuthError::Invalid)?;
    super::revocations::record(
        &state.pool,
        &user.global_identity_id,
        &req.device_public_key,
        &req.signature,
    )
    .await?;
    Ok(Json(req))
}
