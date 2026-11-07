use super::{
    Auth, AuthError, me,
    proof::{public_key, verify_signature},
};
use axum::{
    Json,
    extract::{ConnectInfo, State, rejection::JsonRejection},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, sync::Arc};

#[derive(Deserialize)]
pub(super) struct Proof {
    global_identity_id: String,
    device_public_key: String,
    signature: String,
}

pub(super) async fn record(
    pool: &sqlx::PgPool,
    identity: &str,
    public: &str,
    signature: &str,
) -> Result<(), AuthError> {
    let key = public_key(public).ok_or(AuthError::Invalid)?;
    let device_id = hex::encode(Sha256::digest(key));
    let root: Vec<u8> = sqlx::query_scalar("SELECT root_public_key FROM identities WHERE id=$1")
        .bind(identity)
        .fetch_optional(pool)
        .await?
        .ok_or(AuthError::Credentials)?;
    let payload = kova_protocol::device_revocation(identity, &device_id);
    verify_signature(&root, &payload, signature)?;
    let bytes = hex::decode(signature).map_err(|_| AuthError::Invalid)?;
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO devices(identity_id,device_id,public_key,revoked_at,revocation_signature) VALUES($1,$2,$3,now(),$4) ON CONFLICT(identity_id,device_id) DO UPDATE SET revoked_at=coalesce(devices.revoked_at,excluded.revoked_at),revocation_signature=excluded.revocation_signature")
        .bind(identity).bind(&device_id).bind(key.as_slice()).bind(&bytes).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO identity_revocations(identity_id,device_id,event_id,payload,signature) VALUES($1,$2,$3,$4,$5) ON CONFLICT(identity_id,device_id) DO NOTHING")
        .bind(identity).bind(device_id).bind(hex::encode(Sha256::digest(payload.as_bytes()))).bind(payload).bind(bytes).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub(super) async fn publish(
    State(auth): State<Arc<Auth>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Result<Json<Proof>, JsonRejection>,
) -> Result<Json<Value>, AuthError> {
    let Json(proof) = body.map_err(|_| AuthError::Invalid)?;
    if !auth
        .attempts
        .lock()
        .unwrap()
        .allow(peer.ip(), Some(&proof.global_identity_id))
    {
        return Err(AuthError::Limited);
    }
    record(
        &auth.pool,
        &proof.global_identity_id,
        &proof.device_public_key,
        &proof.signature,
    )
    .await?;
    Ok(Json(json!({"recorded":true})))
}
pub(super) async fn list(
    state: State<Arc<Auth>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AuthError> {
    let Json(user) = me(state.clone(), headers).await?;
    // Device proofs are already exposed by /devices; journal retains canonical events.
    let rows: Vec<(String,String,String,String)>=sqlx::query_as("SELECT r.event_id,r.payload,encode(r.signature,'hex'),encode(d.public_key,'hex') FROM identity_revocations r JOIN devices d USING(identity_id,device_id) WHERE r.identity_id=$1 ORDER BY r.event_id LIMIT 1025")
        .bind(user.global_identity_id).fetch_all(&state.pool).await?;
    if rows.len() > 1024 {
        return Err(AuthError::Limited);
    }
    Ok(Json(
        json!({"items":rows.into_iter().map(|(event_id,payload,signature,device_public_key)|json!({"event_id":event_id,"payload":payload,"signature":signature,"device_public_key":device_public_key})).collect::<Vec<_>>()}),
    ))
}
