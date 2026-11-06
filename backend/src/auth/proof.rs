use super::{Auth, AuthError, User, accounts, cookie};
use axum::{
    Json,
    extract::{ConnectInfo, State, rejection::JsonRejection},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use rand::{RngCore, rngs::OsRng};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, sync::Arc};

pub(super) fn public_key(value: &str) -> Option<[u8; 32]> {
    let mut key = [0; 32];
    hex::decode_to_slice(value, &mut key).ok()?;
    (hex::encode(key) == value).then_some(key)
}

pub(super) fn identity_id(key: &[u8]) -> String {
    format!("kova1-{}", hex::encode(Sha256::digest(key)))
}

fn random_token() -> Result<[u8; 32], AuthError> {
    let mut token = [0; 32];
    OsRng
        .try_fill_bytes(&mut token)
        .map_err(|_| AuthError::Internal)?;
    Ok(token)
}

fn invite_hash(token: Option<&str>) -> Result<Option<Vec<u8>>, AuthError> {
    token
        .map(|v| {
            public_key(v)
                .map(|raw| Sha256::digest(raw).to_vec())
                .ok_or(AuthError::Invite)
        })
        .transpose()
}

#[derive(Deserialize)]
pub(super) struct ChallengeRequest {
    global_identity_id: String,
    device_id: String,
    operation: String,
    invite: Option<String>,
}

#[derive(sqlx::FromRow)]
struct Challenge {
    id: String,
    identity_id: String,
    device_id: String,
    origin: String,
    operation: String,
    invite_hash: Option<Vec<u8>>,
    nonce: String,
    expires_at: String,
}

impl Challenge {
    fn message(&self) -> String {
        format!(
            "kova-auth-v1\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.origin,
            self.operation,
            self.identity_id,
            self.device_id,
            self.invite_hash
                .as_ref()
                .map(hex::encode)
                .unwrap_or_default(),
            self.id,
            self.nonce,
            self.expires_at
        )
    }
}

pub(super) async fn challenge(
    State(auth): State<Arc<Auth>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Result<Json<ChallengeRequest>, JsonRejection>,
) -> Result<Response, AuthError> {
    let Json(req) = body.map_err(|_| AuthError::Invalid)?;
    if !req
        .global_identity_id
        .strip_prefix("kova1-")
        .is_some_and(|s| public_key(s).is_some())
        || public_key(&req.device_id).is_none()
        || !matches!(
            (req.operation.as_str(), req.invite.is_some()),
            ("login", false) | ("join", true)
        )
    {
        return Err(AuthError::Invalid);
    }
    if !auth
        .attempts
        .lock()
        .unwrap()
        .allow(peer.ip(), Some(&req.global_identity_id))
    {
        return Err(AuthError::Limited);
    }
    let invite = invite_hash(req.invite.as_deref())?;
    sqlx::query("DELETE FROM auth_challenges WHERE expires_at <= now()")
        .execute(&auth.pool)
        .await?;
    let row: Challenge = sqlx::query_as("INSERT INTO auth_challenges (identity_id, device_id, origin, operation, invite_hash, nonce)
        VALUES ($1,$2,$3,$4,$5,$6) RETURNING id::text,identity_id,device_id,origin,operation,invite_hash,nonce,to_char(expires_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') as expires_at")
        .bind(req.global_identity_id).bind(req.device_id).bind(&auth.origin).bind(req.operation).bind(invite).bind(hex::encode(random_token()?)).fetch_one(&auth.pool).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"challenge_id":row.id,"challenge":row.message(),"expires_at":row.expires_at})),
    )
        .into_response())
}

#[derive(Deserialize)]
struct Profile {
    handle: String,
    display_name: String,
}
#[derive(Deserialize)]
pub(super) struct VerifyRequest {
    challenge_id: String,
    signature: String,
    root_public_key: String,
    device_public_key: String,
    device_authorization: Authorization,
    profile: Option<Profile>,
    invite: Option<String>,
}
#[derive(Deserialize)]
struct Authorization {
    expires_at: i64,
    signature: String,
}

pub(super) fn verify_signature(
    key: &[u8],
    message: &str,
    signature: &str,
) -> Result<(), AuthError> {
    let mut bytes = [0; 64];
    hex::decode_to_slice(signature, &mut bytes).map_err(|_| AuthError::Credentials)?;
    UnparsedPublicKey::new(&ED25519, key)
        .verify(message.as_bytes(), &bytes)
        .map_err(|_| AuthError::Credentials)
}

pub(super) async fn verify(
    State(auth): State<Arc<Auth>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Result<Json<VerifyRequest>, JsonRejection>,
) -> Result<Response, AuthError> {
    let Json(req) = body.map_err(|_| AuthError::Invalid)?;
    if !auth.attempts.lock().unwrap().allow(peer.ip(), None) {
        return Err(AuthError::Limited);
    }
    let root = public_key(&req.root_public_key).ok_or(AuthError::Credentials)?;
    let device = public_key(&req.device_public_key).ok_or(AuthError::Credentials)?;
    let identity = identity_id(&root);
    let device_id = hex::encode(Sha256::digest(device));
    let mut tx = auth.pool.begin().await?;
    let ch: Challenge = sqlx::query_as("DELETE FROM auth_challenges WHERE id::text=$1 AND expires_at > clock_timestamp() AND origin=$2
        RETURNING id::text,identity_id,device_id,origin,operation,invite_hash,nonce,to_char(expires_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') as expires_at")
        .bind(&req.challenge_id).bind(&auth.origin).fetch_optional(&mut *tx).await?.ok_or(AuthError::Credentials)?;
    if root == device
        || identity != ch.identity_id
        || device_id != ch.device_id
        || invite_hash(req.invite.as_deref())? != ch.invite_hash
    {
        return Err(AuthError::Credentials);
    }
    let expiry = req.device_authorization.expires_at;
    // A bounded Unix timestamp also keeps PostgreSQL timestamp conversion in range.
    if !(1..=253402300799).contains(&expiry) {
        return Err(AuthError::Credentials);
    }
    let attestation = format!(
        "kova-device-v1\n{identity}\n{device_id}\n{}\n{expiry}",
        req.device_public_key
    );
    verify_signature(&root, &attestation, &req.device_authorization.signature)?;
    verify_signature(&device, &ch.message(), &req.signature)?;

    let mut joined = false;
    if let Some(hash) = ch.invite_hash {
        let community: String = sqlx::query_scalar("DELETE FROM invites WHERE token_hash=$1 AND expires_at > clock_timestamp() RETURNING community_id::text")
            .bind(hash).fetch_optional(&mut *tx).await?.ok_or(AuthError::Invite)?;
        sqlx::query("INSERT INTO identities VALUES ($1,$2) ON CONFLICT (id) DO NOTHING")
            .bind(&identity)
            .bind(root.as_slice())
            .execute(&mut *tx)
            .await?;
        // Serialize first joins to distinct communities by the same identity.
        sqlx::query("SELECT id FROM identities WHERE id=$1 FOR UPDATE")
            .bind(&identity)
            .execute(&mut *tx)
            .await?;
        let existing: Option<String> =
            sqlx::query_scalar("SELECT id::text FROM users WHERE global_identity_id=$1")
                .bind(&identity)
                .fetch_optional(&mut *tx)
                .await?;
        let user = if let Some(id) = existing {
            id
        } else {
            let profile = req.profile.ok_or(AuthError::Invalid)?;
            if !accounts::valid_handle(&profile.handle)
                || !(1..=64).contains(&profile.display_name.trim().chars().count())
            {
                return Err(AuthError::Invalid);
            }
            sqlx::query_scalar("INSERT INTO users (id,handle,display_name,global_identity_id) VALUES (gen_random_uuid(),$1,$2,$3) ON CONFLICT (handle) DO NOTHING RETURNING id::text")
                .bind(profile.handle).bind(profile.display_name.trim()).bind(&identity).fetch_optional(&mut *tx).await?.ok_or(AuthError::Conflict)?
        };
        joined = sqlx::query("INSERT INTO memberships (community_id,user_id) VALUES ($1::text::uuid,$2::text::uuid) ON CONFLICT DO NOTHING")
            .bind(community).bind(user).execute(&mut *tx).await?.rows_affected() == 1;
    }
    let user: User = sqlx::query_as("SELECT id::text,global_identity_id,handle,display_name FROM users WHERE global_identity_id=$1")
        .bind(&identity).fetch_optional(&mut *tx).await?.ok_or(AuthError::Credentials)?;
    sqlx::query("INSERT INTO devices (identity_id,device_id,public_key) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(&identity).bind(&device_id).bind(device.as_slice()).execute(&mut *tx).await?;
    let token = random_token()?;
    let age: i32 = sqlx::query_scalar("INSERT INTO sessions (token_hash,user_id,identity_id,device_id,expires_at)
        SELECT $1,$2::text::uuid,identity_id,device_id,least(now()+interval '168 hours',to_timestamp($5::bigint))
        FROM devices WHERE identity_id=$3 AND device_id=$4 AND revoked_at IS NULL AND to_timestamp($5::bigint)>clock_timestamp()
        RETURNING floor(extract(epoch FROM expires_at-now()))::int")
        .bind(Sha256::digest(token).to_vec()).bind(&user.id).bind(identity).bind(device_id).bind(expiry)
        .fetch_optional(&mut *tx).await?.ok_or(AuthError::Credentials)?;
    tx.commit().await?;
    Ok((
        if joined {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        [(
            header::SET_COOKIE,
            cookie(&auth, &hex::encode(token), age as u32),
        )],
        Json(user),
    )
        .into_response())
}
