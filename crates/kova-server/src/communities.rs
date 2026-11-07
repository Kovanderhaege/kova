//! Local community permissions. Every targeted operation rechecks membership under a lock.
use crate::auth::{Auth, AuthError, current_user};
use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, patch, post},
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use std::sync::Arc;

pub(crate) fn routes() -> Router<Arc<Auth>> {
    Router::new()
        .route("/communities", get(list).post(create))
        .route("/communities/{community}", get(detail))
        .route(
            "/communities/{community}/channels",
            get(channels).post(create_channel),
        )
        .route("/communities/{community}/members", get(members))
        .route("/communities/{community}/invites", post(invite))
        .route("/invites/redeem", post(redeem))
        .route(
            "/communities/{community}/members/{user}",
            patch(set_role).delete(exclude),
        )
}

pub(crate) async fn consume_invite(
    tx: &mut Transaction<'_, Postgres>,
    hash: &[u8],
) -> Result<String, AuthError> {
    let community: String = sqlx::query_scalar("SELECT community_id::text FROM invites WHERE token_hash=$1 AND expires_at > clock_timestamp()")
        .bind(hash).fetch_optional(&mut **tx).await?.ok_or(AuthError::Invite)?;
    lock(tx, &community).await?;
    let consumed =
        sqlx::query("DELETE FROM invites WHERE token_hash=$1 AND expires_at > clock_timestamp()")
            .bind(hash)
            .execute(&mut **tx)
            .await?;
    if consumed.rows_affected() == 0 {
        return Err(AuthError::Invite);
    }
    Ok(community)
}

pub(crate) async fn join_member(
    tx: &mut Transaction<'_, Postgres>,
    community: &str,
    user: &str,
) -> Result<bool, AuthError> {
    // The caller holds the community lock until commit, including invite consumption.
    let excluded: bool = sqlx::query_scalar("SELECT EXISTS(SELECT FROM community_exclusions WHERE community_id::text=$1 AND user_id::text=$2)")
        .bind(community).bind(user).fetch_one(&mut **tx).await?;
    if excluded {
        return Err(AuthError::Forbidden);
    }
    Ok(sqlx::query("INSERT INTO memberships (community_id,user_id) VALUES ($1::text::uuid,$2::text::uuid) ON CONFLICT DO NOTHING")
        .bind(community).bind(user).execute(&mut **tx).await?.rows_affected() == 1)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedeemRequest {
    invite: String,
}

async fn redeem(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    body: Result<Json<RedeemRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let Json(body) = body.map_err(|_| AuthError::Invalid)?;
    let token = kova_identity::public_key(&body.invite).ok_or(AuthError::Invite)?;
    let mut tx = auth.pool.begin().await?;
    let community = consume_invite(&mut tx, &Sha256::digest(token)).await?;
    let joined = join_member(&mut tx, &community, &user.id).await?;
    tx.commit().await?;
    Ok((
        if joined {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(json!({"community_id":community})),
    ))
}

#[derive(Serialize, sqlx::FromRow)]
struct Community {
    id: String,
    name: String,
    owner_id: String,
    role: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Name {
    name: String,
}

fn valid_name(name: &str) -> Result<&str, AuthError> {
    let name = name.trim();
    if !(1..=80).contains(&name.chars().count()) || name.chars().any(char::is_control) {
        return Err(AuthError::Invalid);
    }
    Ok(name)
}

// Shared with signed invitation redemption: lock before reading membership or exclusion.
pub(crate) async fn lock(
    tx: &mut Transaction<'_, Postgres>,
    community: &str,
) -> Result<(), AuthError> {
    sqlx::query_scalar::<_, String>(
        "SELECT id::text FROM communities WHERE id::text=$1 FOR UPDATE",
    )
    .bind(community)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AuthError::NotFound)?;
    Ok(())
}

async fn access(
    tx: &mut Transaction<'_, Postgres>,
    community: &str,
    user: &str,
) -> Result<Community, AuthError> {
    lock(tx, community).await?;
    sqlx::query_as("SELECT c.id::text,c.name,c.owner_id::text,CASE WHEN c.owner_id=m.user_id THEN 'owner' ELSE m.role END AS role FROM communities c JOIN memberships m ON m.community_id=c.id WHERE c.id::text=$1 AND m.user_id::text=$2")
        .bind(community).bind(user).fetch_optional(&mut **tx).await?.ok_or(AuthError::NotFound)
}

fn require_manager(role: &str) -> Result<(), AuthError> {
    if matches!(role, "owner" | "moderator") {
        Ok(())
    } else {
        Err(AuthError::Forbidden)
    }
}

async fn list(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Result<Json<Value>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let items: Vec<Community> = sqlx::query_as("SELECT c.id::text,c.name,c.owner_id::text,CASE WHEN c.owner_id=m.user_id THEN 'owner' ELSE m.role END AS role FROM communities c JOIN memberships m ON m.community_id=c.id WHERE m.user_id::text=$1 ORDER BY c.name,c.id")
        .bind(user.id).fetch_all(&auth.pool).await?;
    Ok(Json(json!({"items":items})))
}

async fn create(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    body: Result<Json<Name>, JsonRejection>,
) -> Result<(StatusCode, Json<Community>), AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let Json(body) = body.map_err(|_| AuthError::Invalid)?;
    let name = valid_name(&body.name)?;
    let mut tx = auth.pool.begin().await?;
    let community: Community = sqlx::query_as("INSERT INTO communities(id,name,owner_id) VALUES(gen_random_uuid(),$1,$2::text::uuid) RETURNING id::text,name,owner_id::text,'owner'::text AS role")
        .bind(name).bind(&user.id).fetch_one(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO memberships(community_id,user_id) VALUES($1::text::uuid,$2::text::uuid)",
    )
    .bind(&community.id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO channels(id,community_id,name) VALUES(gen_random_uuid(),$1::text::uuid,'général')")
        .bind(&community.id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(community)))
}

async fn detail(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Community>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let mut tx = auth.pool.begin().await?;
    let community = access(&mut tx, &id, &user.id).await?;
    tx.commit().await?;
    Ok(Json(community))
}

#[derive(Serialize, sqlx::FromRow)]
struct Channel {
    id: String,
    community_id: String,
    name: String,
}

async fn channels(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let mut tx = auth.pool.begin().await?;
    access(&mut tx, &id, &user.id).await?;
    let items: Vec<Channel> = sqlx::query_as("SELECT id::text,community_id::text,name FROM channels WHERE community_id::text=$1 ORDER BY name,id")
        .bind(id).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"items":items})))
}

async fn create_channel(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<Name>, JsonRejection>,
) -> Result<(StatusCode, Json<Channel>), AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let Json(body) = body.map_err(|_| AuthError::Invalid)?;
    let name = valid_name(&body.name)?;
    let mut tx = auth.pool.begin().await?;
    require_manager(&access(&mut tx, &id, &user.id).await?.role)?;
    let channel = sqlx::query_as("INSERT INTO channels(id,community_id,name) VALUES(gen_random_uuid(),$1::text::uuid,$2) ON CONFLICT(community_id,name) DO NOTHING RETURNING id::text,community_id::text,name")
        .bind(id).bind(name).fetch_optional(&mut *tx).await?.ok_or(AuthError::Duplicate)?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(channel)))
}

#[derive(Serialize, sqlx::FromRow)]
struct Member {
    id: String,
    handle: String,
    display_name: String,
    role: String,
}

async fn members(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let mut tx = auth.pool.begin().await?;
    let community = access(&mut tx, &id, &user.id).await?;
    let items: Vec<Member> = sqlx::query_as("SELECT u.id::text,u.handle,u.display_name,CASE WHEN u.id::text=$2 THEN 'owner' ELSE m.role END AS role FROM memberships m JOIN users u ON u.id=m.user_id WHERE m.community_id::text=$1 ORDER BY u.handle,u.id")
        .bind(id).bind(community.owner_id).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"items":items})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InviteRequest {
    expires_in: i32,
}

async fn invite(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Result<Json<InviteRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let Json(body) = body.map_err(|_| AuthError::Invalid)?;
    if !(1..=604800).contains(&body.expires_in) {
        return Err(AuthError::Invalid);
    }
    let mut tx = auth.pool.begin().await?;
    require_manager(&access(&mut tx, &id, &user.id).await?.role)?;
    let mut token = [0; 32];
    OsRng
        .try_fill_bytes(&mut token)
        .map_err(|_| AuthError::Internal)?;
    sqlx::query(
        "DELETE FROM invites WHERE community_id::text=$1 AND expires_at<=clock_timestamp()",
    )
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO invites(token_hash,community_id,expires_at) VALUES($1,$2::text::uuid,clock_timestamp()+make_interval(secs=>$3::int))")
        .bind(Sha256::digest(token).to_vec()).bind(&id).bind(body.expires_in).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"invite":hex::encode(token),"community_id":id,"expires_in":body.expires_in})),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleRequest {
    role: String,
}

async fn set_role(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path((id, target)): Path<(String, String)>,
    body: Result<Json<RoleRequest>, JsonRejection>,
) -> Result<StatusCode, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let Json(body) = body.map_err(|_| AuthError::Invalid)?;
    if !matches!(body.role.as_str(), "member" | "moderator") {
        return Err(AuthError::Invalid);
    }
    let mut tx = auth.pool.begin().await?;
    let community = access(&mut tx, &id, &user.id).await?;
    if community.role != "owner" || target == community.owner_id {
        return Err(AuthError::Forbidden);
    }
    let updated = sqlx::query(
        "UPDATE memberships SET role=$3 WHERE community_id::text=$1 AND user_id::text=$2",
    )
    .bind(id)
    .bind(target)
    .bind(body.role)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AuthError::NotFound);
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn exclude(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path((id, target)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<StatusCode, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    if !body.is_empty() {
        return Err(AuthError::Invalid);
    }
    let mut tx = auth.pool.begin().await?;
    let community = access(&mut tx, &id, &user.id).await?;
    require_manager(&community.role)?;
    if target == community.owner_id || target == user.id {
        return Err(AuthError::Forbidden);
    }
    let role: String = sqlx::query_scalar(
        "SELECT role FROM memberships WHERE community_id::text=$1 AND user_id::text=$2",
    )
    .bind(&id)
    .bind(&target)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AuthError::NotFound)?;
    if community.role == "moderator" && role != "member" {
        return Err(AuthError::Forbidden);
    }
    sqlx::query("INSERT INTO community_exclusions(community_id,user_id,excluded_by) VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid)")
        .bind(&id).bind(&target).bind(user.id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM memberships WHERE community_id::text=$1 AND user_id::text=$2")
        .bind(id)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
