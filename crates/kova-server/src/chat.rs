//! Durable channel log; REST and WS share authorization and cursor semantics.
use crate::{
    auth::{Auth, AuthError, current_user},
    communities,
};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        rejection::JsonRejection,
        ws::{CloseFrame, Message, WebSocket},
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::get,
};
use kova_protocol::{ChatMessage, ChatPage};
use serde::Deserialize;
use sqlx::{Postgres, Row, Transaction};
use std::{sync::Arc, time::Duration};

const PAGE_SIZE: i64 = 100;
const COLUMNS: &str = "channel_id::text,seq,author_id::text,client_id::text,body,to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at";

pub(crate) fn routes() -> Router<Arc<Auth>> {
    Router::new()
        .route("/channels/{channel}/messages", get(history).post(send))
        .route("/channels/{channel}/events", get(subscribe))
        .layer(axum::extract::DefaultBodyLimit::max(32768))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    #[serde(default)]
    after: i64,
}

fn message(row: sqlx::postgres::PgRow) -> Result<ChatMessage, sqlx::Error> {
    Ok(ChatMessage {
        channel_id: row.try_get("channel_id")?,
        seq: row.try_get("seq")?,
        author_id: row.try_get("author_id")?,
        client_id: row.try_get("client_id")?,
        body: row.try_get("body")?,
        created_at: row.try_get("created_at")?,
    })
}

async fn access(
    tx: &mut Transaction<'_, Postgres>,
    channel: &str,
    user: &str,
) -> Result<i64, AuthError> {
    let community: String =
        sqlx::query_scalar("SELECT community_id::text FROM channels WHERE id::text=$1")
            .bind(channel)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or(AuthError::NotFound)?;
    communities::lock(tx, &community).await?;
    let member: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT FROM memberships WHERE community_id::text=$1 AND user_id::text=$2)",
    )
    .bind(community)
    .bind(user)
    .fetch_one(&mut **tx)
    .await?;
    if !member {
        return Err(AuthError::NotFound);
    }
    Ok(
        sqlx::query_scalar("SELECT last_seq FROM channels WHERE id::text=$1")
            .bind(channel)
            .fetch_one(&mut **tx)
            .await?,
    )
}

async fn page(
    tx: &mut Transaction<'_, Postgres>,
    channel: &str,
    after: i64,
    last: i64,
) -> Result<ChatPage, AuthError> {
    if after < 0 || after > last {
        return Err(AuthError::Invalid);
    }
    let rows = sqlx::query(&format!(
        "SELECT {COLUMNS} FROM messages WHERE channel_id::text=$1 AND seq>$2 ORDER BY seq LIMIT $3"
    ))
    .bind(channel)
    .bind(after)
    .bind(PAGE_SIZE)
    .fetch_all(&mut **tx)
    .await?;
    let items = rows
        .into_iter()
        .map(message)
        .collect::<Result<Vec<_>, _>>()?;
    let next_seq = items.last().map_or(after, |m| m.seq);
    Ok(ChatPage {
        items,
        next_seq,
        last_seq: last,
    })
}

async fn history(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(channel): Path<String>,
    Query(cursor): Query<Cursor>,
) -> Result<Json<ChatPage>, AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let mut tx = auth.pool.begin().await?;
    let last = access(&mut tx, &channel, &user.id).await?;
    let result = page(&mut tx, &channel, cursor.after, last).await?;
    tx.commit().await?;
    Ok(Json(result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendMessage {
    client_id: String,
    body: String,
}

async fn send(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(channel): Path<String>,
    body: Result<Json<SendMessage>, JsonRejection>,
) -> Result<(StatusCode, Json<ChatMessage>), AuthError> {
    let user = current_user(&auth.pool, &headers).await?;
    let Json(body) = body.map_err(|_| AuthError::Invalid)?;
    if !kova_protocol::valid_uuid(&body.client_id) || !kova_protocol::valid_message_body(&body.body)
    {
        return Err(AuthError::Invalid);
    }
    let mut tx = auth.pool.begin().await?;
    access(&mut tx, &channel, &user.id).await?;
    let existing = sqlx::query(&format!("SELECT {COLUMNS} FROM messages WHERE channel_id::text=$1 AND author_id::text=$2 AND client_id::text=$3"))
        .bind(&channel).bind(&user.id).bind(&body.client_id).fetch_optional(&mut *tx).await?;
    if let Some(existing) = existing {
        let existing = message(existing)?;
        if existing.body != body.body {
            return Err(AuthError::MessageConflict);
        }
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(existing)));
    }
    let seq: i64 = sqlx::query_scalar(
        "UPDATE channels SET last_seq=last_seq+1 WHERE id::text=$1 RETURNING last_seq",
    )
    .bind(&channel)
    .fetch_one(&mut *tx)
    .await?;
    let row = sqlx::query(&format!("INSERT INTO messages(channel_id,seq,author_id,client_id,body) VALUES($1::text::uuid,$2,$3::text::uuid,$4::text::uuid,$5) RETURNING {COLUMNS}"))
        .bind(channel).bind(seq).bind(user.id).bind(body.client_id).bind(body.body).fetch_one(&mut *tx).await?;
    let result = message(row)?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn subscribe(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Path(channel): Path<String>,
    Query(cursor): Query<Cursor>,
    ws: WebSocketUpgrade,
) -> Result<Response, AuthError> {
    let origins: Vec<_> = headers.get_all("Origin").iter().collect();
    if origins.len() != 1 || origins[0].as_bytes() != auth.origin.as_bytes() {
        return Err(AuthError::Origin);
    }
    let user = current_user(&auth.pool, &headers).await?;
    let mut tx = auth.pool.begin().await?;
    let last = access(&mut tx, &channel, &user.id).await?;
    if cursor.after < 0 || cursor.after > last {
        return Err(AuthError::Invalid);
    }
    tx.commit().await?;
    Ok(ws
        .max_message_size(8192)
        .max_frame_size(8192)
        .write_buffer_size(0)
        .max_write_buffer_size(3_000_000)
        .on_upgrade(move |socket| stream(socket, auth, headers, channel, cursor.after)))
}

async fn deliver(
    socket: &mut WebSocket,
    auth: &Auth,
    headers: &HeaderMap,
    channel: &str,
    after: i64,
) -> Result<i64, AuthError> {
    let user = current_user(&auth.pool, headers).await?;
    let mut tx = auth.pool.begin().await?;
    let last = access(&mut tx, channel, &user.id).await?;
    let result = page(&mut tx, channel, after, last).await?;
    let next = result.next_seq;
    // Keep the community lock until delivery: exclusion cannot commit between check and send.
    let text = serde_json::to_string(&result).map_err(|_| AuthError::Internal)?;
    tokio::time::timeout(
        Duration::from_secs(2),
        socket.send(Message::Text(text.into())),
    )
    .await
    .map_err(|_| AuthError::Internal)?
    .map_err(|_| AuthError::Internal)?;
    tx.commit().await?;
    Ok(next)
}

async fn stream(
    mut socket: WebSocket,
    auth: Arc<Auth>,
    headers: HeaderMap,
    channel: String,
    mut after: i64,
) {
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                match deliver(&mut socket, &auth, &headers, &channel, after).await {
                    Ok(next) => after = next,
                    Err(error) => {
                        let code = if matches!(error, AuthError::Session | AuthError::NotFound) { 1008 } else { 1011 };
                        let _ = tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Close(Some(CloseFrame { code, reason: "Flux interrompu ; reconnectez-vous pour reprendre.".into() })))).await;
                        break;
                    }
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {},
                    _ => break,
                }
            }
        }
    }
}
