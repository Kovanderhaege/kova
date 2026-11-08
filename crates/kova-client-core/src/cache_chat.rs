//! A cursor advances only when preceding messages are durable and consistent.
use super::Cache;
use crate::{
    Result,
    chat::{ChatScope, PendingMessage},
};
use kova_protocol::{ChatMessage, ChatPage};
use rusqlite::{OptionalExtension, params};
impl Cache {
    pub fn chat_cursor(&self, scope: &ChatScope) -> Result<i64> {
        Ok(self.0.query_row("SELECT COALESCE(MAX(seq),0) FROM chat_messages WHERE origin=?1 AND identity=?2 AND channel=?3", params![scope.origin,scope.identity,scope.channel], |r| r.get(0))?)
    }
    pub fn merge_chat_page(&self, scope: &ChatScope, page: &ChatPage) -> Result<()> {
        if page.items.len() > 100 || page.next_seq < 0 || page.last_seq < page.next_seq {
            return Err("Page de chat invalide".into());
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.0,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        let mut cursor = self.chat_cursor(scope)?;
        let mut previous = None;
        for message in &page.items {
            if message.channel_id != scope.channel
                || message.seq <= 0
                || !kova_protocol::valid_message_body(&message.body)
                || previous.is_some_and(|seq| message.seq != seq + 1)
            {
                return Err("Historique incohérent".into());
            }
            previous = Some(message.seq);
            let stored: Option<String> = tx.query_row("SELECT message FROM chat_messages WHERE origin=?1 AND identity=?2 AND channel=?3 AND seq=?4", params![scope.origin,scope.identity,scope.channel,message.seq], |r| r.get(0)).optional()?;
            if let Some(stored) = stored {
                if serde_json::from_str::<ChatMessage>(&stored)? != *message {
                    return Err("Message contradictoire".into());
                }
            } else {
                if message.seq != cursor + 1 {
                    return Err("Trou dans l'historique ; reprendre au curseur local".into());
                }
                tx.execute(
                    "INSERT INTO chat_messages VALUES(?1,?2,?3,?4,?5)",
                    params![
                        scope.origin,
                        scope.identity,
                        scope.channel,
                        message.seq,
                        serde_json::to_string(message)?
                    ],
                )?;
                cursor = message.seq;
            }
        }
        if page.items.last().is_some_and(|m| m.seq != page.next_seq)
            || page.next_seq > cursor
            || (page.items.is_empty() && page.next_seq != page.last_seq)
        {
            return Err("Curseur non contigu".into());
        }
        tx.commit()?;
        Ok(())
    }
    pub fn cached_messages(&self, scope: &ChatScope) -> Result<Vec<ChatMessage>> {
        let values = self.0.prepare("SELECT message FROM (SELECT seq,message FROM chat_messages WHERE origin=?1 AND identity=?2 AND channel=?3 ORDER BY seq DESC LIMIT 100) ORDER BY seq")?
            .query_map(params![scope.origin,scope.identity,scope.channel], |r| r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
        values
            .into_iter()
            .map(|v| Ok(serde_json::from_str(&v)?))
            .collect()
    }
    pub fn pending_message(&self, scope: &ChatScope) -> Result<Option<PendingMessage>> {
        let stored: Option<String> = self
            .0
            .query_row(
                "SELECT message FROM chat_pending WHERE origin=?1 AND identity=?2 AND channel=?3",
                params![scope.origin, scope.identity, scope.channel],
                |r| r.get(0),
            )
            .optional()?;
        stored
            .map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }
    pub fn prepare_message(&self, scope: &ChatScope, body: &str) -> Result<PendingMessage> {
        if !kova_protocol::valid_message_body(body) {
            return Err("Message invalide (1–4000 caractères)".into());
        }
        let tx = rusqlite::Transaction::new_unchecked(
            &self.0,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if let Some(pending) = self.pending_message(scope)? {
            if pending.body != body {
                return Err(
                    "Un envoi est en attente. Réessayez-le avant de rédiger un autre message."
                        .into(),
                );
            }
            return Ok(pending);
        }
        let message = PendingMessage {
            client_id: uuid::Uuid::new_v4().to_string(),
            body: body.into(),
        };
        tx.execute(
            "INSERT INTO chat_pending VALUES(?1,?2,?3,?4)",
            params![
                scope.origin,
                scope.identity,
                scope.channel,
                serde_json::to_string(&message)?
            ],
        )?;
        tx.commit()?;
        Ok(message)
    }
    pub fn acknowledge_message(&self, scope: &ChatScope, message: &ChatMessage) -> Result<()> {
        let tx = rusqlite::Transaction::new_unchecked(
            &self.0,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        if let Some(pending) = self.pending_message(scope)? {
            if message.channel_id != scope.channel
                || pending.client_id != message.client_id
                || pending.body != message.body
            {
                return Err("Accusé d'envoi incohérent".into());
            }
            tx.execute(
                "DELETE FROM chat_pending WHERE origin=?1 AND identity=?2 AND channel=?3",
                params![scope.origin, scope.identity, scope.channel],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> ChatScope {
        ChatScope::new(
            "http://localhost",
            "alice",
            "00000000-0000-0000-0000-000000000001",
        )
        .unwrap()
    }
    fn message(seq: i64) -> ChatMessage {
        ChatMessage {
            channel_id: scope().channel,
            seq,
            author_id: "author".into(),
            client_id: format!("id-{seq}"),
            body: "Bonjour 🦀".into(),
            created_at: "now".into(),
        }
    }
    #[test]
    fn contiguous_atomic_merge_replays_and_isolation() {
        let cache = Cache::open(std::path::Path::new(":memory:")).unwrap();
        let scope = scope();
        let page = ChatPage {
            items: vec![message(1), message(2)],
            next_seq: 2,
            last_seq: 2,
        };
        cache.merge_chat_page(&scope, &page).unwrap();
        cache.merge_chat_page(&scope, &page).unwrap();
        assert_eq!(cache.cached_messages(&scope).unwrap().len(), 2);
        let gap = ChatPage {
            items: vec![message(3), message(5)],
            next_seq: 5,
            last_seq: 5,
        };
        assert!(cache.merge_chat_page(&scope, &gap).is_err());
        assert_eq!(cache.chat_cursor(&scope).unwrap(), 2);
        let mut bad = page.clone();
        bad.items[0].body = "Modifié".into();
        assert!(cache.merge_chat_page(&scope, &bad).is_err());
        let mut other = scope.clone();
        other.origin = "http://localhost:1234".into();
        assert_eq!(cache.chat_cursor(&other).unwrap(), 0);
        other = scope.clone();
        other.identity = "bob".into();
        assert!(cache.cached_messages(&other).unwrap().is_empty());
        let empty = ChatPage {
            items: vec![],
            next_seq: 2,
            last_seq: 3,
        };
        assert!(cache.merge_chat_page(&scope, &empty).is_err());
    }
    #[test]
    fn pending_send_survives_restart_and_keeps_id() {
        let path = std::env::temp_dir().join(format!("kova-chat-{}.sqlite", uuid::Uuid::new_v4()));
        let scope = scope();
        let pending;
        {
            let cache = Cache::open(&path).unwrap();
            pending = cache.prepare_message(&scope, "Bonjour").unwrap();
            assert!(cache.prepare_message(&scope, "Autre texte").is_err());
        }
        {
            let cache = Cache::open(&path).unwrap();
            assert_eq!(cache.prepare_message(&scope, "Bonjour").unwrap(), pending);
            let mut ack = message(1);
            ack.client_id = pending.client_id;
            ack.body = pending.body;
            cache.acknowledge_message(&scope, &ack).unwrap();
            assert!(cache.pending_message(&scope).unwrap().is_none());
        }
        std::fs::remove_file(path).unwrap();
    }
}
