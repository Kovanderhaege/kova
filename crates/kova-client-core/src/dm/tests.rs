use super::*;
use age::secrecy::ExposeSecret;
use kova_protocol::dm::{DmEventPage, StoredDmEvent};

#[test]
fn rejected_ciphertext_does_not_consume_ratchet_or_cursor() {
    let alice = Identity::generate().unwrap();
    let bob = Identity::generate().unwrap();
    let a_path = std::env::temp_dir().join(format!("kova-dm-a-{}.sqlite", uuid::Uuid::new_v4()));
    let b_path = std::env::temp_dir().join(format!("kova-dm-b-{}.sqlite", uuid::Uuid::new_v4()));
    let key = age::x25519::Identity::generate().to_string();
    let mut a = DmStore::open(&a_path, key.expose_secret(), &alice).unwrap();
    let mut b = DmStore::open(&b_path, key.expose_secret(), &bob).unwrap();
    let mut ca = CryptoClient::restore_state(&a.state.crypto).unwrap();
    let mut cb = CryptoClient::restore_state(&b.state.crypto).unwrap();
    let creator = signed_bundle(&alice, ca.key_package().unwrap()).unwrap();
    let recipient = signed_bundle(&bob, cb.key_package().unwrap()).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let welcome = ca
        .create_conversation(&id, &hex::decode(&recipient.key_package).unwrap())
        .unwrap();
    cb.join_conversation(&id, &welcome).unwrap();
    let mut manifest = ConversationManifest {
        conversation_id: id.clone(),
        origin: "https://a.example".into(),
        creator,
        recipient,
        welcome: hex::encode(welcome),
        signature: String::new(),
    };
    manifest.signature = sign(&alice, &manifest.canonical()).unwrap();
    verify_members(&ca, &manifest).unwrap();
    verify_members(&cb, &manifest).unwrap();
    // A third party cannot replace the MLS roster while keeping identity strings.
    let substituted =
        CryptoClient::new(&alice.global_id().unwrap(), &alice.device_id().unwrap()).unwrap();
    let mut wrong = manifest.clone();
    wrong.creator = signed_bundle(&alice, substituted.key_package().unwrap()).unwrap();
    assert!(verify_members(&cb, &wrong).is_err());
    for (store, crypto) in [(&mut a, ca), (&mut b, cb)] {
        let mut state = store.state.clone();
        state.crypto = crypto.export_state().unwrap();
        state.conversations.insert(
            id.clone(),
            Conversation {
                manifest: manifest.clone(),
                cursor: 0,
                source_cursors: BTreeMap::new(),
                active_policy: None,
                pending_creation: false,
                events: BTreeMap::new(),
                messages: BTreeMap::new(),
                pending: Vec::new(),
            },
        );
        store.save(state).unwrap();
    }
    let event_id = a.prepare(&alice, &id, "secret-unit-message").unwrap();
    let event = a.state.conversations[&id].events[&event_id].clone();
    let page = |event| DmEventPage {
        items: vec![StoredDmEvent { cursor: 1, event }],
        next_cursor: 1,
    };
    let mut corrupt = event.clone();
    corrupt.ciphertext.replace_range(0..2, "ff");
    corrupt.signature = sign(&alice, &corrupt.canonical()).unwrap();
    assert!(b.merge(&id, page(corrupt)).is_err());
    assert_eq!(b.state.conversations[&id].cursor, 0);
    let mut missing = event.clone();
    missing.parents = vec![uuid::Uuid::new_v4().to_string()];
    missing.signature = sign(&alice, &missing.canonical()).unwrap();
    assert!(b.merge(&id, page(missing)).is_err());
    b.merge(&id, page(event.clone())).unwrap();
    assert!(b.render(Some(&id)).unwrap().contains("secret-unit-message"));
    let crypto_before_duplicate = b.state.crypto.clone();
    b.merge_from(
        &id,
        "https://b.example",
        DmEventPage {
            items: vec![StoredDmEvent {
                cursor: 1,
                event: event.clone(),
            }],
            next_cursor: 1,
        },
    )
    .unwrap();
    assert_eq!(b.state.conversations[&id].cursor, 1);
    assert_eq!(
        b.state.conversations[&id].source_cursors["https://a.example"],
        1
    );
    assert_eq!(
        b.state.conversations[&id].source_cursors["https://b.example"],
        1
    );
    assert_eq!(
        b.state.crypto, crypto_before_duplicate,
        "duplicate replica copy must not advance MLS ratchets"
    );
    assert!(b.merge(&id, page(event.clone())).is_err()); // Cursor replay.
    a.merge(&id, page(event)).unwrap(); // Echo after an acknowledgement was lost.
    assert!(a.state.conversations[&id].pending.is_empty());
    drop(a);
    drop(b);
    let b = DmStore::open(&b_path, key.expose_secret(), &bob).unwrap();
    assert_eq!(b.state.conversations[&id].cursor, 1);
    assert_eq!(
        b.state.conversations[&id].source_cursors["https://a.example"],
        1
    );
    assert_eq!(
        b.state.conversations[&id].source_cursors["https://b.example"],
        1
    );
    assert!(b.render(Some(&id)).unwrap().contains("secret-unit-message"));
    drop(b);
    for path in [a_path, b_path] {
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.windows(19).any(|w| w == b"secret-unit-message"));
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(path.with_extension("lock")).unwrap();
    }
}
