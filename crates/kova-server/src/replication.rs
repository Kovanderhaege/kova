use crate::auth::{Auth, AuthError, current_device, current_user};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State, rejection::JsonRejection},
    http::HeaderMap,
    routing::get,
};
use kova_protocol::{
    dm::{ConversationManifest, DmEvent, StoredDmEvent, valid_uuid},
    replication::{ReplicaApproval, ReplicaPolicy},
};
use ring::rand::SecureRandom;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, types::Json as DbJson};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) fn routes() -> Router<Arc<Auth>> {
    Router::new()
        .route("/federation/v1/identity", get(server_identity))
        .route(
            "/federation/v1/dm/sync",
            axum::routing::post(federation_sync),
        )
        .route(
            "/dm/conversations/{conversation}/replicas",
            get(active_policy),
        )
        .route(
            "/dm/conversations/{conversation}/replicas/history",
            get(policy_history),
        )
        .route(
            "/dm/conversations/{conversation}/replicas/proposal",
            get(pending_policy),
        )
        .route(
            "/dm/conversations/{conversation}/replicas/approve",
            axum::routing::post(approve_policy),
        )
        .route(
            "/dm/conversations/{conversation}/replicas/sync",
            axum::routing::post(sync_replica),
        )
        .layer(axum::extract::DefaultBodyLimit::max(FEDERATION_MAX_BODY))
}

#[derive(serde::Serialize)]
struct ServerIdentity {
    origin: String,
    public_key: String,
}

async fn server_identity(State(auth): State<Arc<Auth>>) -> Result<Json<ServerIdentity>, AuthError> {
    let public_key = auth
        .federation_public_key
        .clone()
        .ok_or(AuthError::Forbidden)?;
    Ok(Json(ServerIdentity {
        origin: auth.origin.clone(),
        public_key,
    }))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FederationSyncRequest {
    manifest: ConversationManifest,
    policy: ReplicaPolicy,
    policies: Vec<ReplicaPolicy>,
    #[serde(default)]
    authorizations: kova_protocol::dm::DmAuthorizationState,
    source_origin: String,
    source_after: i64,
    remote_after: i64,
    events: Vec<StoredDmEvent>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FederationSyncResponse {
    acknowledged_cursor: i64,
    items: Vec<StoredDmEvent>,
    next_cursor: i64,
    #[serde(default)]
    authorizations: kova_protocol::dm::DmAuthorizationState,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedFederationResponse {
    request_nonce: String,
    request_hash: String,
    response: FederationSyncResponse,
    signature: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncRequest {
    origin: String,
}

#[derive(Serialize)]
struct SyncResult {
    received: u64,
    sent: u64,
}

#[derive(Serialize)]
struct FederationHeaders {
    signer: String,
    timestamp: String,
    nonce: String,
    signature: String,
}

const FEDERATION_PATH: &str = "/api/v1/federation/v1/dm/sync";
const FEDERATION_MAX_EVENTS: usize = 10;
const FEDERATION_MAX_BODY: usize = 2 * 1024 * 1024;
const FEDERATION_MAX_POLICY_HISTORY: usize = 64;

fn now() -> Result<i64, AuthError> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthError::Internal)?
            .as_secs(),
    )
    .map_err(|_| AuthError::Internal)
}

fn valid_replica_origin(origin: &str) -> bool {
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    let local = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain("localhost")) => true,
        _ => false,
    };
    url.origin().ascii_serialization() == origin
        && (url.scheme() == "https" || (local && url.scheme() == "http"))
}

fn federation_canonical(
    source: &str,
    destination: &str,
    method: &str,
    path: &str,
    body: &[u8],
    timestamp: &str,
    nonce: &str,
) -> String {
    format!(
        "kova-federation-dm-v1\n{source}\n{destination}\n{method}\n{path}\n{}\n{timestamp}\n{nonce}",
        hex::encode(Sha256::digest(body))
    )
}

fn sign_federation(
    auth: &Auth,
    destination: &str,
    method: &str,
    body: &[u8],
    timestamp: &str,
    nonce: &str,
) -> Result<FederationHeaders, AuthError> {
    let private = auth
        .federation_private_key
        .as_ref()
        .ok_or(AuthError::Forbidden)?;
    let pair =
        ring::signature::Ed25519KeyPair::from_pkcs8(private).map_err(|_| AuthError::Internal)?;
    let signature = pair.sign(
        federation_canonical(
            &auth.origin,
            destination,
            method,
            FEDERATION_PATH,
            body,
            timestamp,
            nonce,
        )
        .as_bytes(),
    );
    Ok(FederationHeaders {
        signer: auth
            .federation_public_key
            .clone()
            .ok_or(AuthError::Internal)?,
        timestamp: timestamp.to_owned(),
        nonce: nonce.to_owned(),
        signature: hex::encode(signature.as_ref()),
    })
}

fn verify_federation(
    auth: &Auth,
    headers: &HeaderMap,
    body: &[u8],
    source: &str,
    signer_key: &str,
) -> Result<String, AuthError> {
    let single_header = |name| {
        let mut values = headers.get_all(name).iter();
        let value = values
            .next()
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthError::Credentials)?;
        if values.next().is_some() {
            return Err(AuthError::Credentials);
        }
        Ok(value)
    };
    let signer = single_header("x-kova-signer")?;
    let timestamp = single_header("x-kova-timestamp")?;
    let nonce = single_header("x-kova-nonce")?;
    let signature = single_header("x-kova-signature")?;
    let issued_at = timestamp
        .parse::<i64>()
        .map_err(|_| AuthError::Credentials)?;
    if signer != signer_key
        || nonce.len() != 32
        || !nonce
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || (now()?.abs_diff(issued_at)) > 60
    {
        return Err(AuthError::Credentials);
    }
    let key = kova_identity::public_key(signer).ok_or(AuthError::Credentials)?;
    if !kova_identity::verify_signature(
        &key,
        &federation_canonical(
            source,
            &auth.origin,
            "POST",
            FEDERATION_PATH,
            body,
            timestamp,
            nonce,
        ),
        signature,
    ) {
        return Err(AuthError::Credentials);
    }
    Ok(nonce.to_owned())
}

fn response_canonical(
    source: &str,
    destination: &str,
    nonce: &str,
    request_hash: &str,
    response: &FederationSyncResponse,
) -> Result<String, AuthError> {
    let bytes = serde_json::to_vec(response).map_err(|_| AuthError::Internal)?;
    Ok(format!(
        "kova-federation-response-v1\n{source}\n{destination}\n{nonce}\n{request_hash}\n{}",
        hex::encode(Sha256::digest(bytes))
    ))
}

fn sign_response(
    auth: &Auth,
    destination: &str,
    nonce: String,
    request_hash: String,
    response: FederationSyncResponse,
) -> Result<Json<SignedFederationResponse>, AuthError> {
    let private = auth
        .federation_private_key
        .as_ref()
        .ok_or(AuthError::Forbidden)?;
    let pair =
        ring::signature::Ed25519KeyPair::from_pkcs8(private).map_err(|_| AuthError::Internal)?;
    let canonical =
        response_canonical(&auth.origin, destination, &nonce, &request_hash, &response)?;
    let signature = hex::encode(pair.sign(canonical.as_bytes()).as_ref());
    Ok(Json(SignedFederationResponse {
        request_nonce: nonce,
        request_hash,
        response,
        signature,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalRequest {
    policy: ReplicaPolicy,
    approval: ReplicaApproval,
}

async fn participant_manifest(
    auth: &Auth,
    headers: &HeaderMap,
    conversation: &str,
) -> Result<(String, String, ConversationManifest), AuthError> {
    let user = current_user(&auth.pool, headers).await?;
    let device = current_device(&auth.pool, headers).await?;
    if !kova_protocol::dm::valid_uuid(conversation) {
        return Err(AuthError::NotFound);
    }
    let manifest: Option<DbJson<ConversationManifest>> = sqlx::query_scalar(
        "SELECT manifest FROM dm_conversations WHERE id=$1::uuid AND ((creator_identity=$2 AND creator_device=$3) OR (recipient_identity=$2 AND recipient_device=$3))",
    )
    .bind(conversation)
    .bind(&user.global_identity_id)
    .bind(&device)
    .fetch_optional(&auth.pool)
    .await?;
    let Some(DbJson(manifest)) = manifest else {
        return Err(AuthError::NotFound);
    };
    Ok((user.global_identity_id, device, manifest))
}

async fn active_policy(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(conversation): Path<String>,
) -> Result<Json<ReplicaPolicy>, AuthError> {
    let (identity, device, manifest) = participant_manifest(&auth, &headers, &conversation).await?;
    let policy: Option<DbJson<ReplicaPolicy>> = sqlx::query_scalar(
        "SELECT policy FROM dm_replica_policies WHERE conversation_id=$1::uuid AND active",
    )
    .bind(&conversation)
    .fetch_optional(&auth.pool)
    .await?;
    let Some(DbJson(policy)) = policy else {
        return Err(AuthError::NotFound);
    };
    if !kova_identity::replication::verify_policy(&policy, &manifest)
        || !policy
            .approvals
            .iter()
            .any(|a| a.identity == identity && a.device == device)
    {
        return Err(AuthError::NotFound);
    }
    Ok(Json(policy))
}

async fn policy_history(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(conversation): Path<String>,
) -> Result<Json<Vec<ReplicaPolicy>>, AuthError> {
    let (_, _, manifest) = participant_manifest(&auth, &headers, &conversation).await?;
    let rows = sqlx::query(
        "SELECT policy FROM dm_replica_policies WHERE conversation_id=$1::uuid AND version <= (SELECT MAX(version) FROM dm_replica_policies WHERE conversation_id=$1::uuid AND active) ORDER BY version",
    )
    .bind(&conversation)
    .fetch_all(&auth.pool)
    .await?;
    let mut policies = Vec::with_capacity(rows.len());
    let mut previous_hash = None;
    for row in rows {
        let DbJson(policy) = row.try_get::<DbJson<ReplicaPolicy>, _>("policy")?;
        if !kova_identity::replication::verify_policy(&policy, &manifest)
            || policy.previous_hash != previous_hash
            || policy.version as usize != policies.len() + 1
        {
            return Err(AuthError::Internal);
        }
        previous_hash = Some(kova_identity::replication::policy_hash(&policy));
        policies.push(policy);
    }
    if policies.is_empty() {
        return Err(AuthError::NotFound);
    }
    Ok(Json(policies))
}

async fn pending_policy(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(conversation): Path<String>,
) -> Result<Json<ReplicaPolicy>, AuthError> {
    let (identity, device, _) = participant_manifest(&auth, &headers, &conversation).await?;
    let policy: Option<DbJson<ReplicaPolicy>> = sqlx::query_scalar(
        "SELECT policy FROM dm_replica_policies WHERE conversation_id=$1::uuid AND NOT active AND version=(SELECT COALESCE(MAX(version),0)+1 FROM dm_replica_policies WHERE conversation_id=$1::uuid AND active)",
    )
    .bind(&conversation)
    .fetch_optional(&auth.pool)
    .await?;
    let Some(DbJson(policy)) = policy else {
        return Err(AuthError::NotFound);
    };
    let _ = (identity, device);
    Ok(Json(policy))
}

async fn approve_policy(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(conversation): Path<String>,
    body: Result<Json<ApprovalRequest>, JsonRejection>,
) -> Result<Json<ReplicaPolicy>, AuthError> {
    let (identity, device, manifest) = participant_manifest(&auth, &headers, &conversation).await?;
    let Json(mut request) = body.map_err(|_| AuthError::Invalid)?;
    let policy = &request.policy;
    let now = now()?;
    if policy.version > FEDERATION_MAX_POLICY_HISTORY as u64
        || policy.conversation_id != conversation
        || !kova_identity::replication::valid_policy(policy, &manifest)
        || !kova_identity::dm::verify_conversation_historical(&manifest)
        || !request.policy.approvals.is_empty()
        || request.approval.identity != identity
        || request.approval.device != device
        || !kova_identity::replication::verify_approval(policy, &request.approval, &manifest)
        || !configured_descriptors(&auth, policy)
    {
        return Err(AuthError::Invalid);
    }
    let mut tx = auth.pool.begin().await?;
    sqlx::query("SELECT id FROM dm_conversations WHERE id=$1::uuid FOR UPDATE")
        .bind(&conversation)
        .fetch_one(&mut *tx)
        .await?;
    let authorization_state =
        crate::dm::authorization_state(&mut tx, &conversation, &manifest).await?;
    if !kova_identity::dm::conversation_authorized(&manifest, &authorization_state, now) {
        return Err(AuthError::Forbidden);
    }
    if !device_active(
        &mut tx,
        &request.approval.identity,
        &request.approval.device,
        now,
    )
    .await?
    {
        return Err(AuthError::Forbidden);
    }
    let active: Option<(i64, String)> = sqlx::query_as(
        "SELECT version,policy_hash FROM dm_replica_policies WHERE conversation_id=$1::uuid AND active FOR UPDATE",
    ).bind(&conversation).fetch_optional(&mut *tx).await?;
    let expected_version = active
        .as_ref()
        .map_or(1_u64, |(version, _)| (*version as u64) + 1);
    if let Some(DbJson(current)) = sqlx::query_scalar::<_, DbJson<ReplicaPolicy>>(
        "SELECT policy FROM dm_replica_policies WHERE conversation_id=$1::uuid AND active",
    )
    .bind(&conversation)
    .fetch_optional(&mut *tx)
    .await?
        && current.canonical() == policy.canonical()
    {
        if current.approvals.iter().any(|a| a == &request.approval) {
            tx.commit().await?;
            return Ok(Json(current));
        }
        return Err(AuthError::MessageConflict);
    }
    if policy.version != expected_version
        || policy.previous_hash.as_deref() != active.as_ref().map(|(_, hash)| hash.as_str())
    {
        return Err(AuthError::MessageConflict);
    }

    let hash = kova_identity::replication::policy_hash(policy);
    let candidate: Option<DbJson<ReplicaPolicy>> = sqlx::query_scalar(
        "SELECT policy FROM dm_replica_policies WHERE conversation_id=$1::uuid AND version=$2 AND NOT active FOR UPDATE",
    ).bind(&conversation).bind(policy.version as i64).fetch_optional(&mut *tx).await?;
    if let Some(DbJson(existing)) = candidate {
        if existing.canonical() != policy.canonical() || existing.replicas != policy.replicas {
            return Err(AuthError::MessageConflict);
        }
        if existing.approvals.iter().any(|a| a.identity == identity) {
            if existing.approvals.iter().any(|a| a == &request.approval) {
                tx.commit().await?;
                return Ok(Json(existing));
            }
            return Err(AuthError::MessageConflict);
        }
        request.policy.approvals = existing.approvals;
    }
    request.policy.approvals.push(request.approval);
    request
        .policy
        .approvals
        .sort_by(|a, b| a.identity.cmp(&b.identity));
    let active_now = request.policy.approvals.len() == 2;
    if active_now {
        // Recheck the enrolled devices while holding the conversation lock.
        for approval in &request.policy.approvals {
            if !device_active(&mut tx, &approval.identity, &approval.device, now).await? {
                return Err(AuthError::Forbidden);
            }
        }
        sqlx::query(
            "UPDATE dm_replica_policies SET active=false WHERE conversation_id=$1::uuid AND active",
        )
        .bind(&conversation)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("INSERT INTO dm_replica_policies(conversation_id,version,policy_hash,policy,active) VALUES($1::uuid,$2,$3,$4,$5) ON CONFLICT(conversation_id,version) DO UPDATE SET policy=EXCLUDED.policy,policy_hash=EXCLUDED.policy_hash,active=EXCLUDED.active")
        .bind(&conversation).bind(request.policy.version as i64).bind(hash)
        .bind(DbJson(request.policy.clone())).bind(active_now).execute(&mut *tx).await?;
    if active_now {
        sqlx::query("DELETE FROM dm_replica_conflicts WHERE conversation_id=$1::uuid")
            .bind(&conversation)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(request.policy))
}

async fn device_active(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &str,
    device: &str,
    now: i64,
) -> Result<bool, AuthError> {
    let active: Option<bool> = sqlx::query_scalar(
        "SELECT authorization_expires_at > $3 FROM devices WHERE identity_id=$1 AND device_id=$2 AND revoked_at IS NULL FOR SHARE",
    ).bind(identity).bind(device).bind(now).fetch_optional(&mut **tx).await?;
    Ok(active.unwrap_or(false))
}

fn configured_descriptors(auth: &Auth, policy: &ReplicaPolicy) -> bool {
    let Some(local_key) = auth.federation_public_key.as_ref() else {
        return false;
    };
    let peers = &auth.federation_peers;
    policy.replicas.iter().all(|replica| {
        if !valid_replica_origin(&replica.origin) {
            return false;
        }
        if replica.origin == auth.origin {
            replica.public_key == *local_key
        } else {
            peers
                .get(&replica.origin)
                .is_some_and(|key| key == &replica.public_key)
        }
    })
}

async fn sync_replica(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(conversation): Path<String>,
    body: Result<Json<SyncRequest>, JsonRejection>,
) -> Result<Json<SyncResult>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let device = current_device(&auth.pool, &headers).await?;
    let Json(request) = body.map_err(|_| AuthError::Invalid)?;
    if !valid_uuid(&conversation)
        || !valid_replica_origin(&request.origin)
        || request.origin == auth.origin
    {
        return Err(AuthError::Invalid);
    }
    let mut tx = auth.pool.begin().await?;
    let manifest: Option<DbJson<ConversationManifest>> = sqlx::query_scalar(
        "SELECT manifest FROM dm_conversations WHERE id=$1::uuid AND ((creator_identity=$2 AND creator_device=$3) OR (recipient_identity=$2 AND recipient_device=$3)) FOR UPDATE",
    ).bind(&conversation).bind(&user.global_identity_id).bind(&device).fetch_optional(&mut *tx).await?;
    let Some(DbJson(manifest)) = manifest else {
        return Err(AuthError::NotFound);
    };
    let policy: Option<DbJson<ReplicaPolicy>> = sqlx::query_scalar(
        "SELECT policy FROM dm_replica_policies WHERE conversation_id=$1::uuid AND active FOR UPDATE",
    ).bind(&conversation).fetch_optional(&mut *tx).await?;
    let Some(DbJson(policy)) = policy else {
        return Err(AuthError::Forbidden);
    };
    let policy_rows = sqlx::query("SELECT policy FROM dm_replica_policies WHERE conversation_id=$1::uuid AND version <= $2 ORDER BY version")
        .bind(&conversation).bind(policy.version as i64).fetch_all(&mut *tx).await?;
    if policy_rows.len() > FEDERATION_MAX_POLICY_HISTORY {
        return Err(AuthError::Forbidden);
    }
    let mut policies = Vec::with_capacity(policy_rows.len());
    let mut previous_hash = None;
    for row in policy_rows {
        let DbJson(historical) = row.try_get::<DbJson<ReplicaPolicy>, _>("policy")?;
        if historical.previous_hash != previous_hash
            || historical.version != policies.len() as u64 + 1
            || !kova_identity::replication::verify_policy(&historical, &manifest)
        {
            return Err(AuthError::Internal);
        }
        previous_hash = Some(kova_identity::replication::policy_hash(&historical));
        policies.push(historical);
    }
    if policies.last() != Some(&policy) {
        return Err(AuthError::Internal);
    }
    let authorizations = crate::dm::authorization_state(&mut tx, &conversation, &manifest).await?;
    if !kova_identity::replication::verify_policy(&policy, &manifest)
        || !policy.replicas.iter().any(|r| {
            r.origin == auth.origin && Some(&r.public_key) == auth.federation_public_key.as_ref()
        })
        || sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT FROM dm_replica_conflicts WHERE conversation_id=$1::uuid)",
        )
        .bind(&conversation)
        .fetch_one(&mut *tx)
        .await?
    {
        return Err(AuthError::Forbidden);
    }
    let current_has_peer = policy.replicas.iter().any(|r| {
        r.origin == request.origin && auth.federation_peers.get(&r.origin) == Some(&r.public_key)
    });
    let revoked = !authorizations.revocations.is_empty()
        || !crate::dm::participants_not_revoked(&mut tx, &manifest).await?;
    let historical_control_only = !current_has_peer
        && policies.iter().any(|historical| {
            historical.replicas.iter().any(|replica| {
                replica.origin == request.origin
                    && auth.federation_peers.get(&replica.origin) == Some(&replica.public_key)
            })
        });
    if !current_has_peer && !historical_control_only {
        return Err(AuthError::Forbidden);
    }
    let control_only = revoked || historical_control_only;
    let peer_key = auth
        .federation_peers
        .get(&request.origin)
        .ok_or(AuthError::Forbidden)?
        .clone();
    let row: Option<(i64, i64)> = sqlx::query_as("SELECT received_cursor,acknowledged_cursor FROM dm_replica_cursors WHERE conversation_id=$1::uuid AND origin=$2")
        .bind(&conversation).bind(&request.origin).fetch_optional(&mut *tx).await?;
    let (received_cursor, acknowledged_cursor) = row.unwrap_or((0, 0));
    sqlx::query("INSERT INTO dm_replica_cursors(conversation_id,origin) VALUES($1::uuid,$2) ON CONFLICT DO NOTHING")
        .bind(&conversation).bind(&request.origin).execute(&mut *tx).await?;
    let rows = if control_only {
        Vec::new()
    } else {
        sqlx::query("SELECT cursor,event FROM dm_events WHERE conversation_id=$1::uuid AND cursor>$2 ORDER BY cursor LIMIT $3")
            .bind(&conversation).bind(acknowledged_cursor).bind(FEDERATION_MAX_EVENTS as i64).fetch_all(&mut *tx).await?
    };
    let events = rows
        .into_iter()
        .map(|row| {
            let cursor = row.try_get("cursor")?;
            let DbJson(event) = row.try_get::<DbJson<DmEvent>, _>("event")?;
            Ok::<_, sqlx::Error>(StoredDmEvent { cursor, event })
        })
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit().await?;

    let peer_origin = request.origin;
    let request = FederationSyncRequest {
        manifest,
        policy,
        policies,
        authorizations,
        source_origin: auth.origin.clone(),
        source_after: acknowledged_cursor,
        remote_after: received_cursor,
        events,
    };
    let body_bytes = serde_json::to_vec(&request).map_err(|_| AuthError::Internal)?;
    if body_bytes.len() > FEDERATION_MAX_BODY {
        return Err(AuthError::Invalid);
    }
    let timestamp = now()?.to_string();
    let nonce = random_nonce()?;
    let auth_headers =
        sign_federation(&auth, &peer_origin, "POST", &body_bytes, &timestamp, &nonce)?;
    let url = format!("{}{FEDERATION_PATH}", peer_origin);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| AuthError::Internal)?;
    let mut response = client
        .post(url)
        .header("Origin", &peer_origin)
        .header("Content-Type", "application/json")
        .header("x-kova-signer", auth_headers.signer)
        .header("x-kova-timestamp", auth_headers.timestamp)
        .header("x-kova-nonce", auth_headers.nonce)
        .header("x-kova-signature", auth_headers.signature)
        .body(body_bytes)
        .send()
        .await
        .map_err(|_| AuthError::Internal)?;
    if !response.status().is_success() {
        return Err(AuthError::Forbidden);
    }
    let mut response_bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| AuthError::Internal)? {
        if response_bytes.len().saturating_add(chunk.len()) > FEDERATION_MAX_BODY {
            return Err(AuthError::Invalid);
        }
        response_bytes.extend_from_slice(&chunk);
    }
    let signed: SignedFederationResponse =
        serde_json::from_slice(&response_bytes).map_err(|_| AuthError::Invalid)?;
    if signed.request_nonce != nonce {
        return Err(AuthError::Credentials);
    }
    let request_hash = hex::encode(Sha256::digest(
        &serde_json::to_vec(&request).map_err(|_| AuthError::Internal)?,
    ));
    if signed.request_hash != request_hash {
        return Err(AuthError::Credentials);
    }
    let canonical = response_canonical(
        &peer_origin,
        &auth.origin,
        &nonce,
        &request_hash,
        &signed.response,
    )?;
    let key = kova_identity::public_key(&peer_key).ok_or(AuthError::Internal)?;
    if !kova_identity::verify_signature(&key, &canonical, &signed.signature) {
        return Err(AuthError::Credentials);
    }
    if control_only
        && (!signed.response.items.is_empty()
            || signed.response.next_cursor != received_cursor
            || signed.response.acknowledged_cursor < acknowledged_cursor)
    {
        return Err(AuthError::Invalid);
    }
    if control_only && !revoked {
        return Ok(Json(SyncResult {
            received: 0,
            sent: 0,
        }));
    }
    apply_remote_page(
        &auth,
        &conversation,
        &peer_origin,
        &signed.response,
        ExchangeCheckpoint {
            received_cursor,
            acknowledged_cursor,
            sent_cursor: request
                .events
                .last()
                .map_or(acknowledged_cursor, |item| item.cursor),
            policy_hash: kova_identity::replication::policy_hash(&request.policy),
        },
    )
    .await?;
    let sent = (signed.response.acknowledged_cursor - acknowledged_cursor) as u64;
    Ok(Json(SyncResult {
        received: signed.response.items.len() as u64,
        sent,
    }))
}

fn random_nonce() -> Result<String, AuthError> {
    let mut bytes = [0u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| AuthError::Internal)?;
    Ok(hex::encode(bytes))
}

async fn federation_sync(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<SignedFederationResponse>, AuthError> {
    if body.len() > FEDERATION_MAX_BODY {
        return Err(AuthError::Invalid);
    }
    let request: FederationSyncRequest =
        serde_json::from_slice(&body).map_err(|_| AuthError::Invalid)?;
    let request_key = request
        .policy
        .replicas
        .iter()
        .find(|r| r.origin == request.source_origin)
        .map(|r| r.public_key.as_str())
        .ok_or(AuthError::Forbidden)?;
    let nonce = verify_federation(&auth, &headers, &body, &request.source_origin, request_key)?;
    if request.source_origin == auth.origin
        || !valid_replica_origin(&request.source_origin)
        || !valid_uuid(&request.manifest.conversation_id)
        || auth.federation_peers.get(&request.source_origin) != Some(&request_key.to_string())
        || !kova_identity::replication::verify_policy(&request.policy, &request.manifest)
        || !configured_descriptors(&auth, &request.policy)
        || request.policies.is_empty()
        || request.policies.len() > FEDERATION_MAX_POLICY_HISTORY
        || request.policies.last() != Some(&request.policy)
        || !kova_identity::dm::verify_conversation_historical(&request.manifest)
        || !request
            .policy
            .replicas
            .iter()
            .any(|r| r.origin == request.source_origin)
        || request.events.len() > FEDERATION_MAX_EVENTS
        || request.source_after < 0
        || request.remote_after < 0
    {
        return Err(AuthError::Forbidden);
    }
    let mut expected_previous = None;
    for (index, policy) in request.policies.iter().enumerate() {
        if policy.version != index as u64 + 1
            || policy.previous_hash != expected_previous
            || !kova_identity::replication::verify_policy(policy, &request.manifest)
        {
            return Err(AuthError::Invalid);
        }
        expected_previous = Some(kova_identity::replication::policy_hash(policy));
    }
    let local_descriptor_present = request.policy.replicas.iter().any(|r| {
        r.origin == auth.origin && Some(&r.public_key) == auth.federation_public_key.as_ref()
    });
    if !local_descriptor_present
        && (!request.events.is_empty()
            || !request.policies.iter().any(|policy| {
                policy.replicas.iter().any(|replica| {
                    replica.origin == auth.origin
                        && Some(&replica.public_key) == auth.federation_public_key.as_ref()
                })
            }))
    {
        return Err(AuthError::Forbidden);
    }
    let request_hash = hex::encode(Sha256::digest(&body));
    let source_origin = request.source_origin.clone();
    let response = apply_federation_request(&auth, request, &nonce).await?;
    sign_response(&auth, &source_origin, nonce, request_hash, response)
}

async fn apply_federation_request(
    auth: &Auth,
    request: FederationSyncRequest,
    nonce: &str,
) -> Result<FederationSyncResponse, AuthError> {
    let conversation = request.manifest.conversation_id.clone();
    let mut tx = auth.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&conversation)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM dm_federation_nonces WHERE used_at < now() - interval '7 days'")
        .execute(&mut *tx)
        .await?;
    let inserted = sqlx::query(
        "INSERT INTO dm_federation_nonces(peer_key,nonce) VALUES($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(
        &request
            .policy
            .replicas
            .iter()
            .find(|r| r.origin == request.source_origin)
            .ok_or(AuthError::Forbidden)?
            .public_key,
    )
    .bind(nonce)
    .execute(&mut *tx)
    .await?;
    if inserted.rows_affected() != 1 {
        return Err(AuthError::Credentials);
    }
    let existing: Option<DbJson<ConversationManifest>> =
        sqlx::query_scalar("SELECT manifest FROM dm_conversations WHERE id=$1::uuid FOR UPDATE")
            .bind(&conversation)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(DbJson(current)) = &existing {
        if current != &request.manifest {
            return Err(AuthError::MessageConflict);
        }
    } else {
        if !request
            .policy
            .replicas
            .iter()
            .any(|replica| replica.origin == auth.origin)
        {
            return Err(AuthError::Forbidden);
        }
        ensure_manifest_devices(&mut tx, &request.manifest).await?;
        sqlx::query("INSERT INTO dm_conversations(id,origin,creator_identity,creator_device,recipient_identity,recipient_device,manifest) VALUES($1::uuid,$2,$3,$4,$5,$6,$7)")
            .bind(&conversation).bind(&request.manifest.origin)
            .bind(&request.manifest.creator.global_identity_id).bind(&request.manifest.creator.device_id)
            .bind(&request.manifest.recipient.global_identity_id).bind(&request.manifest.recipient.device_id)
            .bind(DbJson(request.manifest.clone())).execute(&mut *tx).await?;
    }
    crate::dm::merge_authorizations(
        &mut tx,
        &conversation,
        &request.manifest,
        &request.authorizations,
    )
    .await?;
    let current: Option<(i64, String)> = sqlx::query_as("SELECT version,policy_hash FROM dm_replica_policies WHERE conversation_id=$1::uuid AND active FOR UPDATE")
        .bind(&conversation).fetch_optional(&mut *tx).await?;
    let current_version = current
        .as_ref()
        .map_or(0_u64, |(version, _)| *version as u64);
    if let Some((version, hash)) = &current {
        // An offline peer's older head is not evidence of a conflicting branch.
        if request.policy.version < *version as u64 {
            return Err(AuthError::MessageConflict);
        }
        if request
            .policies
            .get(*version as usize - 1)
            .is_none_or(|known| {
                known.version != *version as u64
                    || kova_identity::replication::policy_hash(known) != *hash
            })
        {
            let competing = request
                .policies
                .get(*version as usize - 1)
                .map(kova_identity::replication::policy_hash)
                .unwrap_or_else(|| kova_identity::replication::policy_hash(&request.policy));
            sqlx::query("INSERT INTO dm_replica_conflicts(conversation_id,conflicting_hash) VALUES($1::uuid,$2) ON CONFLICT(conversation_id) DO UPDATE SET conflicting_hash=EXCLUDED.conflicting_hash,detected_at=now()")
                .bind(&conversation).bind(competing).execute(&mut *tx).await?;
            tx.commit().await?;
            return Err(AuthError::MessageConflict);
        }
    } else if request
        .policies
        .first()
        .is_none_or(|first| first.version != 1 || first.previous_hash.is_some())
    {
        return Err(AuthError::MessageConflict);
    }
    let mut previous_hash = current.as_ref().map(|(_, hash)| hash.clone());
    let mut expected_version = current_version + 1;
    for policy in request.policies.iter().skip(current_version as usize) {
        if policy.version != expected_version {
            return Err(AuthError::MessageConflict);
        }
        if policy.version > 1 && policy.previous_hash != previous_hash {
            return Err(AuthError::MessageConflict);
        }
        if policy.version <= current_version {
            continue;
        }
        let hash = kova_identity::replication::policy_hash(policy);
        sqlx::query(
            "UPDATE dm_replica_policies SET active=false WHERE conversation_id=$1::uuid AND active",
        )
        .bind(&conversation)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO dm_replica_policies(conversation_id,version,policy_hash,policy,active) VALUES($1::uuid,$2,$3,$4,true) ON CONFLICT(conversation_id,version) DO UPDATE SET policy_hash=EXCLUDED.policy_hash,policy=EXCLUDED.policy,active=true WHERE NOT dm_replica_policies.active")
            .bind(&conversation).bind(policy.version as i64).bind(&hash).bind(DbJson(policy.clone())).execute(&mut *tx).await?;
        previous_hash = Some(hash);
        expected_version += 1;
    }
    if current.is_none() || request.policy.version > current_version {
        sqlx::query("DELETE FROM dm_replica_conflicts WHERE conversation_id=$1::uuid")
            .bind(&conversation)
            .execute(&mut *tx)
            .await?;
    }
    let conflicted: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT FROM dm_replica_conflicts WHERE conversation_id=$1::uuid)",
    )
    .bind(&conversation)
    .fetch_one(&mut *tx)
    .await?;
    if conflicted {
        return Err(AuthError::Forbidden);
    }
    // Commit control state before processing event batches. A malformed or
    // incomplete batch must never roll back a valid renewal or revocation.
    tx.commit().await?;
    let mut tx = auth.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&conversation)
        .execute(&mut *tx)
        .await?;
    let manifest: ConversationManifest = sqlx::query_scalar::<_, DbJson<_>>(
        "SELECT manifest FROM dm_conversations WHERE id=$1::uuid FOR UPDATE",
    )
    .bind(&conversation)
    .fetch_one(&mut *tx)
    .await?
    .0;
    let active_hash: Option<String> = sqlx::query_scalar(
        "SELECT policy_hash FROM dm_replica_policies WHERE conversation_id=$1::uuid AND active",
    )
    .bind(&conversation)
    .fetch_optional(&mut *tx)
    .await?;
    if active_hash.as_deref()
        != Some(kova_identity::replication::policy_hash(&request.policy).as_str())
    {
        return Err(AuthError::Forbidden);
    }
    let authorizations = crate::dm::authorization_state(&mut tx, &conversation, &manifest).await?;
    if !authorizations.revocations.is_empty()
        || !crate::dm::participants_not_revoked(&mut tx, &manifest).await?
    {
        let source_cursor: i64 = sqlx::query_scalar(
            "SELECT received_cursor FROM dm_replica_cursors WHERE conversation_id=$1::uuid AND origin=$2",
        )
        .bind(&conversation)
        .bind(&request.source_origin)
        .fetch_optional(&mut *tx)
        .await?
        .unwrap_or(0);
        tx.commit().await?;
        return Ok(FederationSyncResponse {
            acknowledged_cursor: source_cursor,
            items: Vec::new(),
            next_cursor: request.remote_after,
            authorizations,
        });
    }
    sqlx::query("INSERT INTO dm_replica_cursors(conversation_id,origin) VALUES($1::uuid,$2) ON CONFLICT DO NOTHING")
        .bind(&conversation).bind(&request.source_origin).execute(&mut *tx).await?;
    let expected: i64 = sqlx::query_scalar("SELECT received_cursor FROM dm_replica_cursors WHERE conversation_id=$1::uuid AND origin=$2 FOR UPDATE")
        .bind(&conversation).bind(&request.source_origin).fetch_one(&mut *tx).await?;
    if request.source_after > expected {
        return Err(AuthError::MessageConflict);
    }
    let mut received = expected;
    for item in &request.events {
        if !kova_identity::dm::verify_event(&item.event, &request.manifest) {
            return Err(AuthError::Invalid);
        }
        if item.cursor <= received {
            let stored = sqlx::query_scalar::<_, DbJson<DmEvent>>(
                "SELECT event FROM dm_events WHERE conversation_id=$1::uuid AND event_id=$2::uuid",
            )
            .bind(&conversation)
            .bind(&item.event.event_id)
            .fetch_optional(&mut *tx)
            .await?;
            if stored.as_ref().map(|DbJson(value)| value) != Some(&item.event) {
                return Err(AuthError::MessageConflict);
            }
        } else {
            if item.cursor != received + 1 {
                return Err(AuthError::MessageConflict);
            }
            insert_event(&mut tx, &item.event, &conversation).await?;
            received = item.cursor;
        }
    }
    sqlx::query("UPDATE dm_replica_cursors SET received_cursor=$3 WHERE conversation_id=$1::uuid AND origin=$2")
        .bind(&conversation).bind(&request.source_origin).bind(received).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO dm_replica_cursors(conversation_id,origin) VALUES($1::uuid,$2) ON CONFLICT DO NOTHING")
        .bind(&conversation).bind(&request.source_origin).execute(&mut *tx).await?;
    let last_cursor: i64 =
        sqlx::query_scalar("SELECT last_cursor FROM dm_conversations WHERE id=$1::uuid")
            .bind(&conversation)
            .fetch_one(&mut *tx)
            .await?;
    if request.remote_after > last_cursor {
        return Err(AuthError::Invalid);
    }
    let local_is_replica = request
        .policy
        .replicas
        .iter()
        .any(|r| r.origin == auth.origin);
    let rows = if local_is_replica {
        sqlx::query("SELECT cursor,event FROM dm_events WHERE conversation_id=$1::uuid AND cursor>$2 ORDER BY cursor LIMIT $3")
        .bind(&conversation).bind(request.remote_after).bind(FEDERATION_MAX_EVENTS as i64).fetch_all(&mut *tx).await?
    } else {
        Vec::new()
    };
    let items = rows
        .into_iter()
        .map(|row| {
            let cursor = row.try_get("cursor")?;
            let DbJson(event) = row.try_get::<DbJson<DmEvent>, _>("event")?;
            Ok::<_, sqlx::Error>(StoredDmEvent { cursor, event })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = items
        .last()
        .map_or(request.remote_after, |item| item.cursor);
    sqlx::query("UPDATE dm_replica_cursors SET acknowledged_cursor=GREATEST(acknowledged_cursor,$3) WHERE conversation_id=$1::uuid AND origin=$2")
        .bind(&conversation).bind(&request.source_origin).bind(request.remote_after).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(FederationSyncResponse {
        acknowledged_cursor: received,
        items,
        next_cursor,
        authorizations,
    })
}

struct ExchangeCheckpoint {
    received_cursor: i64,
    acknowledged_cursor: i64,
    sent_cursor: i64,
    policy_hash: String,
}

async fn apply_remote_page(
    auth: &Auth,
    conversation: &str,
    remote_origin: &str,
    response: &FederationSyncResponse,
    checkpoint: ExchangeCheckpoint,
) -> Result<(), AuthError> {
    if response.acknowledged_cursor < checkpoint.acknowledged_cursor
        || response.acknowledged_cursor > checkpoint.sent_cursor
        || response.items.len() > FEDERATION_MAX_EVENTS
    {
        return Err(AuthError::MessageConflict);
    }
    let mut tx = auth.pool.begin().await?;
    let manifest: Option<DbJson<ConversationManifest>> =
        sqlx::query_scalar("SELECT manifest FROM dm_conversations WHERE id=$1::uuid FOR UPDATE")
            .bind(conversation)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(DbJson(manifest)) = manifest else {
        return Err(AuthError::NotFound);
    };
    crate::dm::merge_authorizations(&mut tx, conversation, &manifest, &response.authorizations)
        .await?;
    let active_policy: Option<DbJson<ReplicaPolicy>> = sqlx::query_scalar(
        "SELECT policy FROM dm_replica_policies WHERE conversation_id=$1::uuid AND active FOR UPDATE",
    ).bind(conversation).fetch_optional(&mut *tx).await?;
    let Some(DbJson(active_policy)) = active_policy else {
        return Err(AuthError::Forbidden);
    };
    if kova_identity::replication::policy_hash(&active_policy) != checkpoint.policy_hash
        || !active_policy.replicas.iter().any(|r| {
            r.origin == auth.origin && Some(&r.public_key) == auth.federation_public_key.as_ref()
        })
        || !active_policy.replicas.iter().any(|r| {
            r.origin == remote_origin
                && auth.federation_peers.get(remote_origin) == Some(&r.public_key)
        })
        || sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT FROM dm_replica_conflicts WHERE conversation_id=$1::uuid)",
        )
        .bind(conversation)
        .fetch_one(&mut *tx)
        .await?
    {
        return Err(AuthError::Forbidden);
    }
    tx.commit().await?;
    let mut tx = auth.pool.begin().await?;
    sqlx::query("SELECT id FROM dm_conversations WHERE id=$1::uuid FOR UPDATE")
        .bind(conversation)
        .fetch_one(&mut *tx)
        .await?;
    let active_hash: Option<String> = sqlx::query_scalar(
        "SELECT policy_hash FROM dm_replica_policies WHERE conversation_id=$1::uuid AND active",
    )
    .bind(conversation)
    .fetch_optional(&mut *tx)
    .await?;
    if active_hash.as_deref() != Some(checkpoint.policy_hash.as_str()) {
        return Err(AuthError::Forbidden);
    }
    let authorizations = crate::dm::authorization_state(&mut tx, conversation, &manifest).await?;
    if !authorizations.revocations.is_empty()
        || !crate::dm::participants_not_revoked(&mut tx, &manifest).await?
    {
        tx.commit().await?;
        return Ok(());
    }
    let current: i64 = sqlx::query_scalar("SELECT received_cursor FROM dm_replica_cursors WHERE conversation_id=$1::uuid AND origin=$2 FOR UPDATE")
        .bind(conversation).bind(remote_origin).fetch_optional(&mut *tx).await?.unwrap_or(0);
    if current != checkpoint.received_cursor {
        return Err(AuthError::MessageConflict);
    }
    let last_cursor: i64 =
        sqlx::query_scalar("SELECT last_cursor FROM dm_conversations WHERE id=$1::uuid")
            .bind(conversation)
            .fetch_one(&mut *tx)
            .await?;
    if response.acknowledged_cursor > last_cursor {
        return Err(AuthError::Invalid);
    }
    let mut received = current;
    for item in &response.items {
        if item.cursor != received + 1 || !kova_identity::dm::verify_event(&item.event, &manifest) {
            return Err(AuthError::Invalid);
        }
        insert_event(&mut tx, &item.event, conversation).await?;
        received = item.cursor;
    }
    if received != response.next_cursor {
        return Err(AuthError::Invalid);
    }
    sqlx::query("UPDATE dm_replica_cursors SET received_cursor=$3,acknowledged_cursor=GREATEST(acknowledged_cursor,$4) WHERE conversation_id=$1::uuid AND origin=$2")
        .bind(conversation).bind(remote_origin).bind(received).bind(response.acknowledged_cursor).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn insert_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    event: &DmEvent,
    conversation: &str,
) -> Result<(), AuthError> {
    let existing = sqlx::query(
        "SELECT event FROM dm_events WHERE conversation_id=$1::uuid AND event_id=$2::uuid",
    )
    .bind(conversation)
    .bind(&event.event_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(row) = existing {
        let DbJson(value) = row.try_get::<DbJson<DmEvent>, _>("event")?;
        return if value == *event {
            Ok(())
        } else {
            Err(AuthError::MessageConflict)
        };
    }
    for parent in &event.parents {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT FROM dm_events WHERE conversation_id=$1::uuid AND event_id=$2::uuid)")
            .bind(conversation).bind(parent).fetch_one(&mut **tx).await?;
        if !exists {
            return Err(AuthError::MissingParents);
        }
    }
    let local_cursor: i64 = sqlx::query_scalar("UPDATE dm_conversations SET last_cursor=last_cursor+1 WHERE id=$1::uuid RETURNING last_cursor")
        .bind(conversation).fetch_one(&mut **tx).await?;
    let ciphertext = hex::decode(&event.ciphertext).map_err(|_| AuthError::Invalid)?;
    let signature = hex::decode(&event.signature).map_err(|_| AuthError::Invalid)?;
    sqlx::query("INSERT INTO dm_events(conversation_id,cursor,event_id,author_identity,author_device,parents,ciphertext,signature,event) VALUES($1::uuid,$2,$3::uuid,$4,$5,$6::text[]::uuid[],$7,$8,$9)")
        .bind(conversation).bind(local_cursor).bind(&event.event_id).bind(&event.author_identity).bind(&event.author_device)
        .bind(&event.parents).bind(ciphertext).bind(signature).bind(DbJson(event.clone())).execute(&mut **tx).await?;
    Ok(())
}

async fn ensure_manifest_devices(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    manifest: &ConversationManifest,
) -> Result<(), AuthError> {
    let mut bundles = [&manifest.creator, &manifest.recipient];
    bundles.sort_by_key(|bundle| (&bundle.global_identity_id, &bundle.device_id));
    for bundle in bundles {
        let root = hex::decode(&bundle.root_public_key).map_err(|_| AuthError::Invalid)?;
        sqlx::query(
            "INSERT INTO identities(id,root_public_key) VALUES($1,$2) ON CONFLICT(id) DO NOTHING",
        )
        .bind(&bundle.global_identity_id)
        .bind(&root)
        .execute(&mut **tx)
        .await?;
        let stored: Vec<u8> =
            sqlx::query_scalar("SELECT root_public_key FROM identities WHERE id=$1")
                .bind(&bundle.global_identity_id)
                .fetch_one(&mut **tx)
                .await?;
        if stored != root {
            return Err(AuthError::Forbidden);
        }
        let public = hex::decode(&bundle.device_public_key).map_err(|_| AuthError::Invalid)?;
        let auth_signature =
            hex::decode(&bundle.authorization_signature).map_err(|_| AuthError::Invalid)?;
        let authorization = kova_protocol::device_authorization_v2(
            &bundle.global_identity_id,
            &bundle.device_id,
            &bundle.device_public_key,
            bundle.issued_at,
            bundle.expires_at,
        );
        sqlx::query("INSERT INTO devices(identity_id,device_id,public_key,authorization_payload,authorization_signature,authorization_expires_at) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(identity_id,device_id) DO NOTHING")
            .bind(&bundle.global_identity_id).bind(&bundle.device_id).bind(public).bind(authorization).bind(auth_signature).bind(bundle.expires_at).execute(&mut **tx).await?;
    }
    Ok(())
}
