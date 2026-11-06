use super::{
    Auth, AuthError, me,
    proof::{public_key, verify_signature},
};
use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::HeaderMap,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Serialize, sqlx::FromRow)]
struct Device {
    device_id: String,
    revoked: bool,
}

pub(super) async fn list(
    state: State<Arc<Auth>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AuthError> {
    let Json(user) = me(state.clone(), headers).await?;
    let devices: Vec<Device> = sqlx::query_as("SELECT device_id, revoked_at IS NOT NULL AS revoked FROM devices WHERE identity_id=$1 ORDER BY device_id")
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
    let key = public_key(&req.device_public_key).ok_or(AuthError::Invalid)?;
    let device_id = hex::encode(Sha256::digest(key));
    let root: Vec<u8> = sqlx::query_scalar("SELECT root_public_key FROM identities WHERE id=$1")
        .bind(&user.global_identity_id)
        .fetch_one(&state.pool)
        .await?;
    let message = format!("kova-revoke-v1\n{}\n{}", user.global_identity_id, device_id);
    verify_signature(&root, &message, &req.signature)?;
    sqlx::query("INSERT INTO devices(identity_id,device_id,public_key,revoked_at) VALUES ($1,$2,$3,now())
        ON CONFLICT (identity_id,device_id) DO UPDATE SET revoked_at=coalesce(devices.revoked_at,excluded.revoked_at)")
        .bind(user.global_identity_id).bind(device_id).bind(key.as_slice()).execute(&state.pool).await?;
    Ok(Json(req))
}
