//! Invoked by tests/chat.py with disposable server credentials, never a user vault.
use kova_client_core::{Cache, Instance, Session};
use std::{
    env,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires the disposable PostgreSQL instance created by chat.py"]
fn public_chat_live_roundtrip() {
    let origin = env::var("KOVA_TEST_ORIGIN").unwrap();
    let channel = env::var("KOVA_TEST_CHANNEL").unwrap();
    let session: Session = serde_json::from_str(&env::var("KOVA_TEST_SESSION").unwrap()).unwrap();
    let instance = Instance::new(&origin).unwrap();
    let scope = instance.chat_scope(&session, &channel).unwrap();
    let path = env::temp_dir().join(format!("kova-chat-live-{}.sqlite", uuid::Uuid::new_v4()));
    let original;
    let pending;
    {
        let cache = Cache::open(&path).unwrap();
        instance.sync_messages(&session, &cache, &channel).unwrap();
        original = cache.chat_cursor(&scope).unwrap();
        pending = cache
            .prepare_message(&scope, "Rust 🦀 — réponse perdue")
            .unwrap();
        let ack = instance.send_message(&session, &channel, &pending).unwrap();
        assert_eq!(ack.seq, original + 1);
        // Deliberately lose the acknowledgement and restart the cache/connection.
    }
    {
        let cache = Cache::open(&path).unwrap();
        assert_eq!(cache.pending_message(&scope).unwrap().unwrap(), pending);
        let ack = instance.send_message(&session, &channel, &pending).unwrap();
        assert_eq!(ack.seq, original + 1);
        cache.acknowledge_message(&scope, &ack).unwrap();
        let mut stream = instance
            .subscribe_chat(&session, &channel, original)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while cache.chat_cursor(&scope).unwrap() < ack.seq {
            assert!(Instant::now() < deadline);
            if let Some(page) = stream.read_page().unwrap() {
                cache.merge_chat_page(&scope, &page).unwrap();
            }
        }
        drop(stream);
        let pending = cache
            .prepare_message(&scope, "Rust — pendant la coupure")
            .unwrap();
        let next = instance.send_message(&session, &channel, &pending).unwrap();
        cache.acknowledge_message(&scope, &next).unwrap();
        let mut resumed = instance
            .subscribe_chat(&session, &channel, cache.chat_cursor(&scope).unwrap())
            .unwrap();
        loop {
            assert!(Instant::now() < deadline);
            if let Some(page) = resumed.read_page().unwrap() {
                cache.merge_chat_page(&scope, &page).unwrap();
                if page.next_seq == next.seq {
                    break;
                }
            }
        }
        assert_eq!(cache.chat_cursor(&scope).unwrap(), original + 2);
        assert!(cache.pending_message(&scope).unwrap().is_none());
        let messages = cache.cached_messages(&scope).unwrap();
        assert_eq!(
            messages
                .iter()
                .filter(|m| m.client_id == pending.client_id)
                .count(),
            1
        );
    }
    std::fs::remove_file(path).unwrap();
}
