//! Local storage and participant authorization for opaque one-to-one DM data.
use crate::auth::{Auth, AuthError, current_device, current_user};
use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::HeaderMap,
    routing::get,
};
use kova_protocol::dm::{
    ConversationManifest, DeviceBundle, DmEvent, DmEventPage, StoredDmEvent, valid_device_bundle,
    valid_dm_event, valid_uuid,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, types::Json as DbJson};
use std::{sync::Arc, time::SystemTime};

const MAX_PAGE: i64 = 100;

pub(crate) fn routes() -> Router<Arc<Auth>> {
    Router::new()
        .route("/dm/key-packages", get(get_package).post(put_package))
        .route("/dm/key-packages/{identity}/{device}", get(public_package))
        .route(
            "/dm/conversations",
            get(list_conversations).post(create_conversation),
        )
        .route("/dm/conversations/{conversation}", get(get_conversation))
        .route(
            "/dm/conversations/{conversation}/authorizations",
            get(get_authorizations).post(renew_authorization),
        )
        .route(
            "/dm/conversations/{conversation}/events",
            get(list_events).post(put_event),
        )
        .layer(axum::extract::DefaultBodyLimit::max(200_000))
}

fn unix_now() -> Result<i64, AuthError> {
    let seconds = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| AuthError::Internal)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| AuthError::Internal)
}

fn package_digest(bundle: &DeviceBundle) -> Result<String, AuthError> {
    let package = hex::decode(&bundle.key_package).map_err(|_| AuthError::Invalid)?;
    Ok(hex::encode(Sha256::digest(package)))
}

async fn lock_package(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    digest: &str,
) -> Result<(), AuthError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("dm-key-package:{digest}"))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn get_package(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
) -> Result<Json<DeviceBundle>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let device = current_device(&auth.pool, &headers).await?;
    package(&auth.pool, &user.global_identity_id, &device).await
}

async fn public_package(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path((identity, device)): Path<(String, String)>,
) -> Result<Json<DeviceBundle>, AuthError> {
    current_user(&auth.pool, &headers).await?;
    package(&auth.pool, &identity, &device).await
}

async fn package(
    pool: &sqlx::PgPool,
    identity: &str,
    device: &str,
) -> Result<Json<DeviceBundle>, AuthError> {
    let value: Option<DbJson<DeviceBundle>> = sqlx::query_scalar(
        "SELECT bundle FROM dm_key_packages WHERE identity_id=$1 AND device_id=$2",
    )
    .bind(identity)
    .bind(device)
    .fetch_optional(pool)
    .await?;
    value
        .map(|DbJson(bundle)| Json(bundle))
        .ok_or(AuthError::NotFound)
}

async fn put_package(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    body: Result<Json<DeviceBundle>, JsonRejection>,
) -> Result<Json<DeviceBundle>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let device = current_device(&auth.pool, &headers).await?;
    let Json(bundle) = body.map_err(|_| AuthError::Invalid)?;
    let now = unix_now()?;
    if !valid_device_bundle(&bundle)
        || bundle.global_identity_id != user.global_identity_id
        || bundle.device_id != device
        || !kova_identity::dm::verify_device(&bundle, now)
    {
        return Err(AuthError::Invalid);
    }
    let mut tx = auth.pool.begin().await?;
    let digest = package_digest(&bundle)?;
    lock_package(&mut tx, &digest).await?;
    let used: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT FROM dm_used_key_packages WHERE package_hash=$1)")
            .bind(&digest)
            .fetch_one(&mut *tx)
            .await?;
    if used {
        return Err(AuthError::MessageConflict);
    }
    sqlx::query("INSERT INTO dm_key_packages(identity_id,device_id,bundle) VALUES($1,$2,$3) ON CONFLICT(identity_id,device_id) DO UPDATE SET bundle=EXCLUDED.bundle,received_at=now()")
        .bind(&bundle.global_identity_id)
        .bind(&bundle.device_id)
        .bind(DbJson(bundle.clone()))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(bundle))
}

async fn create_conversation(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    body: Result<Json<ConversationManifest>, JsonRejection>,
) -> Result<Json<ConversationManifest>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let device = current_device(&auth.pool, &headers).await?;
    let Json(manifest) = body.map_err(|_| AuthError::Invalid)?;
    let now = unix_now()?;
    if manifest.origin != auth.origin
        || manifest.creator.global_identity_id != user.global_identity_id
        || manifest.creator.device_id != device
        || !kova_identity::dm::verify_conversation(&manifest, now)
    {
        return Err(AuthError::Invalid);
    }

    let mut tx = auth.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&manifest.conversation_id)
        .execute(&mut *tx)
        .await?;
    let existing: Option<DbJson<ConversationManifest>> =
        sqlx::query_scalar("SELECT manifest FROM dm_conversations WHERE id=$1::uuid FOR UPDATE")
            .bind(&manifest.conversation_id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(DbJson(existing)) = existing {
        if existing == manifest {
            tx.commit().await?;
            return Ok(Json(existing));
        }
        return Err(AuthError::MessageConflict);
    }

    let digest = package_digest(&manifest.recipient)?;
    lock_package(&mut tx, &digest).await?;
    let recipient: Option<DbJson<DeviceBundle>> = sqlx::query_scalar(
        "SELECT bundle FROM dm_key_packages WHERE identity_id=$1 AND device_id=$2 FOR UPDATE",
    )
    .bind(&manifest.recipient.global_identity_id)
    .bind(&manifest.recipient.device_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(DbJson(recipient)) = recipient else {
        return Err(AuthError::NotFound);
    };
    let used: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT FROM dm_used_key_packages WHERE package_hash=$1)")
            .bind(&digest)
            .fetch_one(&mut *tx)
            .await?;
    if recipient != manifest.recipient || used || !participants_active(&mut tx, &manifest).await? {
        return Err(AuthError::Forbidden);
    }
    let authorization_state = kova_protocol::dm::DmAuthorizationState {
        authorizations: vec![manifest.creator.clone(), manifest.recipient.clone()],
        revocations: Vec::new(),
    };
    sqlx::query("INSERT INTO dm_conversations(id,origin,creator_identity,creator_device,recipient_identity,recipient_device,manifest,authorization_state) VALUES($1::uuid,$2,$3,$4,$5,$6,$7,$8)")
        .bind(&manifest.conversation_id)
        .bind(&manifest.origin)
        .bind(&manifest.creator.global_identity_id)
        .bind(&manifest.creator.device_id)
        .bind(&manifest.recipient.global_identity_id)
        .bind(&manifest.recipient.device_id)
        .bind(DbJson(manifest.clone()))
        .bind(DbJson(authorization_state))
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO dm_used_key_packages(package_hash,identity_id,device_id,conversation_id) VALUES($1,$2,$3,$4::uuid)")
        .bind(digest)
        .bind(&manifest.recipient.global_identity_id)
        .bind(&manifest.recipient.device_id)
        .bind(&manifest.conversation_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM dm_key_packages WHERE identity_id=$1 AND device_id=$2")
        .bind(&manifest.recipient.global_identity_id)
        .bind(&manifest.recipient.device_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(manifest))
}

async fn device_is_active(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &str,
    device: &str,
) -> Result<bool, AuthError> {
    let active: Option<bool> = sqlx::query_scalar(
        "SELECT authorization_expires_at > floor(extract(epoch from now()))::bigint FROM devices WHERE identity_id=$1 AND device_id=$2 AND revoked_at IS NULL FOR SHARE",
    )
    .bind(identity)
    .bind(device)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(active.unwrap_or(false))
}

async fn participants_active(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    manifest: &ConversationManifest,
) -> Result<bool, AuthError> {
    let mut participants = [&manifest.creator, &manifest.recipient];
    participants.sort_by(|left, right| {
        (&left.global_identity_id, &left.device_id)
            .cmp(&(&right.global_identity_id, &right.device_id))
    });
    for participant in participants {
        if !device_is_active(tx, &participant.global_identity_id, &participant.device_id).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    #[serde(default)]
    after: i64,
    #[serde(default = "default_page_size")]
    limit: i64,
}

fn default_page_size() -> i64 {
    MAX_PAGE
}

async fn list_conversations(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ConversationManifest>>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let device = current_device(&auth.pool, &headers).await?;
    let rows = sqlx::query_scalar::<_, DbJson<ConversationManifest>>(
        "SELECT manifest FROM dm_conversations WHERE (creator_identity=$1 AND creator_device=$2) OR (recipient_identity=$1 AND recipient_device=$2) ORDER BY created_at,id",
    )
    .bind(user.global_identity_id)
    .bind(device)
    .fetch_all(&auth.pool)
    .await?;
    Ok(Json(
        rows.into_iter().map(|DbJson(manifest)| manifest).collect(),
    ))
}

async fn get_conversation(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(conversation): Path<String>,
) -> Result<Json<ConversationManifest>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let device = current_device(&auth.pool, &headers).await?;
    if !valid_uuid(&conversation) {
        return Err(AuthError::NotFound);
    }
    let manifest: Option<DbJson<ConversationManifest>> = sqlx::query_scalar(
        "SELECT manifest FROM dm_conversations WHERE id=$1::uuid AND ((creator_identity=$2 AND creator_device=$3) OR (recipient_identity=$2 AND recipient_device=$3))",
    )
    .bind(conversation)
    .bind(user.global_identity_id)
    .bind(device)
    .fetch_optional(&auth.pool)
    .await?;
    manifest
        .map(|DbJson(manifest)| Json(manifest))
        .ok_or(AuthError::NotFound)
}

async fn get_authorizations(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(conversation): Path<String>,
) -> Result<Json<kova_protocol::dm::DmAuthorizationState>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let device = current_device(&auth.pool, &headers).await?;
    let mut tx = auth.pool.begin().await?;
    let manifest =
        authorized_manifest(&mut tx, &conversation, &user.global_identity_id, &device).await?;
    let state = authorization_state(&mut tx, &conversation, &manifest).await?;
    tx.commit().await?;
    Ok(Json(state))
}

async fn renew_authorization(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(conversation): Path<String>,
    body: Result<Json<DeviceBundle>, JsonRejection>,
) -> Result<Json<kova_protocol::dm::DmAuthorizationState>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let device = current_device(&auth.pool, &headers).await?;
    let Json(bundle) = body.map_err(|_| AuthError::Invalid)?;
    if bundle.global_identity_id != user.global_identity_id || bundle.device_id != device {
        return Err(AuthError::Forbidden);
    }
    let mut tx = auth.pool.begin().await?;
    let manifest =
        authorized_manifest(&mut tx, &conversation, &user.global_identity_id, &device).await?;
    let original = [&manifest.creator, &manifest.recipient]
        .into_iter()
        .find(|participant| {
            participant.device_id == device
                && participant.global_identity_id == user.global_identity_id
        })
        .ok_or(AuthError::Forbidden)?;
    if !kova_identity::dm::verify_renewal(&bundle, original)
        || !kova_identity::dm::verify_device(&bundle, unix_now()?)
    {
        return Err(AuthError::Invalid);
    }
    let state = authorization_state(&mut tx, &conversation, &manifest).await?;
    if state.revocations.iter().any(|proof| {
        proof.global_identity_id == bundle.global_identity_id
            && proof.device_public_key == bundle.device_public_key
    }) {
        return Err(AuthError::Forbidden);
    }
    let state = merge_authorizations(
        &mut tx,
        &conversation,
        &manifest,
        &kova_protocol::dm::DmAuthorizationState {
            authorizations: vec![bundle],
            revocations: Vec::new(),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Json(state))
}

pub(crate) async fn authorization_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    conversation: &str,
    manifest: &ConversationManifest,
) -> Result<kova_protocol::dm::DmAuthorizationState, AuthError> {
    let mut state: kova_protocol::dm::DmAuthorizationState = sqlx::query_scalar::<_, DbJson<_>>(
        "SELECT authorization_state FROM dm_conversations WHERE id=$1::uuid FOR UPDATE",
    )
    .bind(conversation)
    .fetch_one(&mut **tx)
    .await?
    .0;
    for bundle in [&manifest.creator, &manifest.recipient] {
        if !state.authorizations.iter().any(|known| {
            known.device_id == bundle.device_id
                && known.global_identity_id == bundle.global_identity_id
        }) {
            state.authorizations.push(bundle.clone());
        }
    }
    let rows = sqlx::query("SELECT r.identity_id, encode(d.public_key,'hex') AS public_key, encode(r.signature,'hex') AS signature FROM identity_revocations r JOIN devices d USING(identity_id,device_id) WHERE (r.identity_id,r.device_id) IN (($1,$2),($3,$4))")
        .bind(&manifest.creator.global_identity_id).bind(&manifest.creator.device_id)
        .bind(&manifest.recipient.global_identity_id).bind(&manifest.recipient.device_id)
        .fetch_all(&mut **tx).await?;
    for row in rows {
        let proof = kova_protocol::dm::DmRevocation {
            global_identity_id: row.try_get("identity_id")?,
            device_public_key: row.try_get("public_key")?,
            signature: row.try_get("signature")?,
        };
        if !state.revocations.iter().any(|known| {
            known.global_identity_id == proof.global_identity_id
                && known.device_public_key == proof.device_public_key
        }) {
            state.revocations.push(proof);
        }
    }
    state.authorizations.sort_by(|a, b| {
        (&a.global_identity_id, &a.device_id).cmp(&(&b.global_identity_id, &b.device_id))
    });
    state.revocations.sort_by(|a, b| {
        (&a.global_identity_id, &a.device_public_key)
            .cmp(&(&b.global_identity_id, &b.device_public_key))
    });
    if !kova_identity::dm::verify_authorization_state(&state, manifest) {
        return Err(AuthError::Internal);
    }
    Ok(state)
}

pub(crate) async fn merge_authorizations(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    conversation: &str,
    manifest: &ConversationManifest,
    incoming: &kova_protocol::dm::DmAuthorizationState,
) -> Result<kova_protocol::dm::DmAuthorizationState, AuthError> {
    if !kova_identity::dm::verify_authorization_state(incoming, manifest) {
        return Err(AuthError::Invalid);
    }
    let current_time = unix_now()?;
    if incoming
        .authorizations
        .iter()
        .any(|bundle| bundle.issued_at > current_time)
    {
        return Err(AuthError::Invalid);
    }
    let mut current = authorization_state(tx, conversation, manifest).await?;
    for candidate in &incoming.authorizations {
        match current.authorizations.iter_mut().find(|known| {
            known.device_id == candidate.device_id
                && known.global_identity_id == candidate.global_identity_id
        }) {
            Some(known)
                if kova_identity::dm::authorization_order(candidate)
                    > kova_identity::dm::authorization_order(known) =>
            {
                *known = candidate.clone()
            }
            Some(_) => {}
            None => current.authorizations.push(candidate.clone()),
        }
    }
    for proof in &incoming.revocations {
        if !current.revocations.iter().any(|known| {
            known.global_identity_id == proof.global_identity_id
                && known.device_public_key == proof.device_public_key
        }) {
            current.revocations.push(proof.clone());
        }
    }
    current.authorizations.sort_by(|a, b| {
        (&a.global_identity_id, &a.device_id).cmp(&(&b.global_identity_id, &b.device_id))
    });
    current.revocations.sort_by(|a, b| {
        (&a.global_identity_id, &a.device_public_key)
            .cmp(&(&b.global_identity_id, &b.device_public_key))
    });
    if !kova_identity::dm::verify_authorization_state(&current, manifest) {
        return Err(AuthError::Invalid);
    }
    sqlx::query("UPDATE dm_conversations SET authorization_state=$2 WHERE id=$1::uuid")
        .bind(conversation)
        .bind(DbJson(current.clone()))
        .execute(&mut **tx)
        .await?;
    for bundle in &current.authorizations {
        if let Some(proof) = current.revocations.iter().find(|proof| {
            proof.global_identity_id == bundle.global_identity_id
                && proof.device_public_key == bundle.device_public_key
        }) {
            let signature = hex::decode(&proof.signature).map_err(|_| AuthError::Invalid)?;
            let payload =
                kova_protocol::device_revocation(&bundle.global_identity_id, &bundle.device_id);
            sqlx::query("UPDATE devices SET revoked_at=coalesce(revoked_at,now()),revocation_signature=$3 WHERE identity_id=$1 AND device_id=$2")
                .bind(&bundle.global_identity_id).bind(&bundle.device_id).bind(&signature)
                .execute(&mut **tx).await?;
            sqlx::query("INSERT INTO identity_revocations(identity_id,device_id,event_id,payload,signature) VALUES($1,$2,$3,$4,$5) ON CONFLICT(identity_id,device_id) DO NOTHING")
                .bind(&bundle.global_identity_id).bind(&bundle.device_id)
                .bind(hex::encode(Sha256::digest(payload.as_bytes()))).bind(payload).bind(signature)
                .execute(&mut **tx).await?;
            continue;
        }
        let payload = kova_protocol::device_authorization_v2(
            &bundle.global_identity_id,
            &bundle.device_id,
            &bundle.device_public_key,
            bundle.issued_at,
            bundle.expires_at,
        );
        sqlx::query("UPDATE devices SET authorization_payload=$3,authorization_signature=$4,authorization_expires_at=$5 WHERE identity_id=$1 AND device_id=$2 AND revoked_at IS NULL AND (authorization_expires_at IS NULL OR authorization_expires_at < $5)")
            .bind(&bundle.global_identity_id)
            .bind(&bundle.device_id)
            .bind(payload)
            .bind(hex::decode(&bundle.authorization_signature).map_err(|_| AuthError::Invalid)?)
            .bind(bundle.expires_at)
            .execute(&mut **tx)
            .await?;
    }
    Ok(current)
}

async fn list_events(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(conversation): Path<String>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<DmEventPage>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let device = current_device(&auth.pool, &headers).await?;
    let mut tx = auth.pool.begin().await?;
    let manifest =
        authorized_manifest(&mut tx, &conversation, &user.global_identity_id, &device).await?;
    if !replica_is_active(&mut tx, &conversation, &auth.origin, &manifest).await? {
        return Err(AuthError::Forbidden);
    }
    if !kova_identity::dm::verify_conversation_historical(&manifest) {
        return Err(AuthError::Internal);
    }
    if cursor.after < 0 || !(1..=MAX_PAGE).contains(&cursor.limit) {
        return Err(AuthError::Invalid);
    }
    let last_cursor: i64 =
        sqlx::query_scalar("SELECT last_cursor FROM dm_conversations WHERE id=$1::uuid")
            .bind(&conversation)
            .fetch_one(&mut *tx)
            .await?;
    if cursor.after > last_cursor {
        return Err(AuthError::Invalid);
    }
    let rows = sqlx::query("SELECT cursor,event FROM dm_events WHERE conversation_id=$1::uuid AND cursor>$2 ORDER BY cursor LIMIT $3")
        .bind(&conversation)
        .bind(cursor.after)
        .bind(cursor.limit)
        .fetch_all(&mut *tx)
        .await?;
    let items = rows
        .into_iter()
        .map(|row| {
            let cursor = row.try_get("cursor")?;
            let DbJson(event) = row.try_get::<DbJson<DmEvent>, _>("event")?;
            Ok(StoredDmEvent { cursor, event })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    if items
        .iter()
        .any(|item| !kova_identity::dm::verify_event(&item.event, &manifest))
    {
        return Err(AuthError::Internal);
    }
    let next_cursor = items.last().map_or(cursor.after, |item| item.cursor);
    tx.commit().await?;
    Ok(Json(DmEventPage { items, next_cursor }))
}

async fn authorized_manifest(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    conversation: &str,
    identity: &str,
    device: &str,
) -> Result<ConversationManifest, AuthError> {
    if !valid_uuid(conversation) {
        return Err(AuthError::NotFound);
    }
    let manifest: Option<DbJson<ConversationManifest>> = sqlx::query_scalar(
        "SELECT manifest FROM dm_conversations WHERE id=$1::uuid AND ((creator_identity=$2 AND creator_device=$3) OR (recipient_identity=$2 AND recipient_device=$3)) FOR UPDATE",
    )
    .bind(conversation)
    .bind(identity)
    .bind(device)
    .fetch_optional(&mut **tx)
    .await?;
    manifest
        .map(|DbJson(manifest)| manifest)
        .ok_or(AuthError::NotFound)
}

async fn put_event(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(conversation): Path<String>,
    body: Result<Json<DmEvent>, JsonRejection>,
) -> Result<Json<StoredDmEvent>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let device = current_device(&auth.pool, &headers).await?;
    let Json(event) = body.map_err(|_| AuthError::Invalid)?;
    if event.conversation_id != conversation
        || event.author_identity != user.global_identity_id
        || event.author_device != device
        || !valid_dm_event(&event)
    {
        return Err(AuthError::Invalid);
    }
    let mut tx = auth.pool.begin().await?;
    let manifest =
        authorized_manifest(&mut tx, &conversation, &user.global_identity_id, &device).await?;
    if !replica_is_active(&mut tx, &conversation, &auth.origin, &manifest).await? {
        return Err(AuthError::Forbidden);
    }
    if !kova_identity::dm::verify_conversation_historical(&manifest)
        || !kova_identity::dm::verify_event(&event, &manifest)
    {
        return Err(AuthError::Invalid);
    }
    let authorizations = authorization_state(&mut tx, &conversation, &manifest).await?;
    if !kova_identity::dm::conversation_authorized(&manifest, &authorizations, unix_now()?)
        || !participants_not_revoked(&mut tx, &manifest).await?
    {
        return Err(AuthError::Forbidden);
    }
    let row = sqlx::query(
        "SELECT cursor,event FROM dm_events WHERE conversation_id=$1::uuid AND event_id=$2::uuid",
    )
    .bind(&conversation)
    .bind(&event.event_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(row) = row {
        let cursor: i64 = row.try_get("cursor")?;
        let DbJson(existing) = row.try_get::<DbJson<DmEvent>, _>("event")?;
        if existing != event {
            return Err(AuthError::MessageConflict);
        }
        tx.commit().await?;
        return Ok(Json(StoredDmEvent { cursor, event }));
    }
    for parent in &event.parents {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT FROM dm_events WHERE conversation_id=$1::uuid AND event_id=$2::uuid)",
        )
        .bind(&conversation)
        .bind(parent)
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(AuthError::MissingParents);
        }
    }
    let cursor: i64 = sqlx::query_scalar(
        "UPDATE dm_conversations SET last_cursor=last_cursor+1 WHERE id=$1::uuid RETURNING last_cursor",
    )
    .bind(&conversation)
    .fetch_one(&mut *tx)
    .await?;
    let ciphertext = hex::decode(&event.ciphertext).map_err(|_| AuthError::Invalid)?;
    let signature = hex::decode(&event.signature).map_err(|_| AuthError::Invalid)?;
    sqlx::query("INSERT INTO dm_events(conversation_id,cursor,event_id,author_identity,author_device,parents,ciphertext,signature,event) VALUES($1::uuid,$2,$3::uuid,$4,$5,$6::text[]::uuid[],$7,$8,$9)")
        .bind(&conversation)
        .bind(cursor)
        .bind(&event.event_id)
        .bind(&event.author_identity)
        .bind(&event.author_device)
        .bind(&event.parents)
        .bind(ciphertext)
        .bind(signature)
        .bind(DbJson(event.clone()))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(StoredDmEvent { cursor, event }))
}

pub(crate) async fn participants_not_revoked(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    manifest: &ConversationManifest,
) -> Result<bool, AuthError> {
    let mut participants = [&manifest.creator, &manifest.recipient];
    participants.sort_by_key(|bundle| (&bundle.global_identity_id, &bundle.device_id));
    for bundle in participants {
        let active: Option<bool> = sqlx::query_scalar(
            "SELECT revoked_at IS NULL FROM devices WHERE identity_id=$1 AND device_id=$2 FOR SHARE",
        )
        .bind(&bundle.global_identity_id)
        .bind(&bundle.device_id)
        .fetch_optional(&mut **tx)
        .await?;
        if !active.unwrap_or(false) {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn replica_is_active(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    conversation: &str,
    origin: &str,
    manifest: &ConversationManifest,
) -> Result<bool, AuthError> {
    let policy: Option<DbJson<kova_protocol::replication::ReplicaPolicy>> = sqlx::query_scalar(
        "SELECT policy FROM dm_replica_policies WHERE conversation_id=$1::uuid AND active",
    )
    .bind(conversation)
    .fetch_optional(&mut **tx)
    .await?;
    match policy {
        None => Ok(manifest.origin == origin),
        Some(DbJson(policy)) => Ok(kova_identity::replication::verify_policy(&policy, manifest)
            && policy
                .replicas
                .iter()
                .any(|replica| replica.origin == origin)),
    }
}
