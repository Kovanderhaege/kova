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
    sqlx::query("INSERT INTO dm_conversations(id,origin,creator_identity,creator_device,recipient_identity,recipient_device,manifest) VALUES($1::uuid,$2,$3,$4,$5,$6,$7)")
        .bind(&manifest.conversation_id)
        .bind(&manifest.origin)
        .bind(&manifest.creator.global_identity_id)
        .bind(&manifest.creator.device_id)
        .bind(&manifest.recipient.global_identity_id)
        .bind(&manifest.recipient.device_id)
        .bind(DbJson(manifest.clone()))
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
    if !kova_identity::dm::verify_conversation_historical(&manifest)
        || !kova_identity::dm::verify_event(&event, &manifest)
    {
        return Err(AuthError::Invalid);
    }
    if !participants_active(&mut tx, &manifest).await? {
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
