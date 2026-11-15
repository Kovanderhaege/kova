use super::*;
use age::secrecy::ExposeSecret;
use kova_protocol::dm::{DmAuthorizationState, DmEventPage, DmRevocation, StoredDmEvent};
use std::path::PathBuf;

struct Fixture {
    alice: Identity,
    bob: Identity,
    store: DmStore,
    id: String,
    path: PathBuf,
    password: age::secrecy::SecretString,
}

fn fixture() -> Fixture {
    let alice = Identity::generate().unwrap();
    let bob = Identity::generate().unwrap();
    let path =
        std::env::temp_dir().join(format!("kova-dm-renewal-{}.sqlite", uuid::Uuid::new_v4()));
    let password = age::x25519::Identity::generate().to_string();
    let mut store = DmStore::open(&path, password.expose_secret(), &alice).unwrap();
    let mut crypto = CryptoClient::restore_state(&store.state.crypto).unwrap();
    let peer_crypto =
        CryptoClient::new(&bob.global_id().unwrap(), &bob.device_id().unwrap()).unwrap();
    let creator = signed_bundle(&alice, crypto.key_package().unwrap()).unwrap();
    let recipient = signed_bundle(&bob, peer_crypto.key_package().unwrap()).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let welcome = crypto
        .create_conversation(&id, &hex::decode(&recipient.key_package).unwrap())
        .unwrap();
    let mut manifest = ConversationManifest {
        conversation_id: id.clone(),
        origin: "https://renewal.example".into(),
        creator,
        recipient,
        welcome: hex::encode(welcome),
        signature: String::new(),
    };
    manifest.signature = sign(&alice, &manifest.canonical()).unwrap();
    let authorization_state = DmAuthorizationState {
        authorizations: vec![manifest.creator.clone(), manifest.recipient.clone()],
        revocations: Vec::new(),
    };
    let mut state = store.state.clone();
    state.crypto = crypto.export_state().unwrap();
    state.conversations.insert(
        id.clone(),
        Conversation {
            manifest,
            cursor: 0,
            source_cursors: BTreeMap::new(),
            active_policy: None,
            authorization_state,
            pending_creation: false,
            events: BTreeMap::new(),
            messages: BTreeMap::new(),
            pending: Vec::new(),
        },
    );
    store.save(state).unwrap();
    Fixture {
        alice,
        bob,
        store,
        id,
        path,
        password,
    }
}

#[test]
fn typed_local_dm_page_keeps_topological_suffix_and_pending_state() {
    let mut fixture = fixture();
    let path = fixture.path.clone();
    let mut state = fixture.store.state.clone();
    let conversation = state.conversations.get_mut(&fixture.id).unwrap();
    let mut parent = Vec::new();
    let mut ids = Vec::new();
    for index in 0..3 {
        let id = uuid::Uuid::new_v4().to_string();
        conversation.events.insert(
            id.clone(),
            DmEvent {
                conversation_id: fixture.id.clone(),
                event_id: id.clone(),
                author_identity: fixture.alice.global_id().unwrap(),
                author_device: fixture.alice.device_id().unwrap(),
                parents: parent.clone(),
                ciphertext: "00".into(),
                signature: "00".into(),
            },
        );
        conversation
            .messages
            .insert(id.clone(), format!("message-{index}"));
        parent = vec![id.clone()];
        ids.push(id);
    }
    conversation.pending.push(ids[2].clone());
    fixture.store.save(state).unwrap();

    let page = fixture.store.local_messages(&fixture.id, 2).unwrap();
    assert_eq!(
        page.iter()
            .map(|message| message.body.as_str())
            .collect::<Vec<_>>(),
        ["message-1", "message-2"]
    );
    assert!(!page[0].pending);
    assert!(page[1].pending);
    assert_eq!(page[1].id, ids[2]);
    drop(fixture);
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(path.with_extension("lock")).unwrap();
}

fn private_key(identity: &Identity, field: &str) -> ring::signature::Ed25519KeyPair {
    let value = serde_json::to_value(identity).unwrap();
    let bytes: Vec<u8> = serde_json::from_value(value[field].clone()).unwrap();
    ring::signature::Ed25519KeyPair::from_pkcs8(&bytes).unwrap()
}

fn bundle_at(
    identity: &Identity,
    original: &DeviceBundle,
    issued_at: i64,
    expires_at: i64,
) -> DeviceBundle {
    let mut bundle = original.clone();
    bundle.issued_at = issued_at;
    bundle.expires_at = expires_at;
    let auth = kova_protocol::device_authorization_v2(
        &bundle.global_identity_id,
        &bundle.device_id,
        &bundle.device_public_key,
        issued_at,
        expires_at,
    );
    bundle.authorization_signature = hex::encode(
        private_key(identity, "root_pkcs8")
            .sign(auth.as_bytes())
            .as_ref(),
    );
    bundle.signature = hex::encode(
        private_key(identity, "device_pkcs8")
            .sign(bundle.canonical().as_bytes())
            .as_ref(),
    );
    bundle
}

fn resign_manifest(manifest: &mut ConversationManifest, alice: &Identity) {
    manifest.signature = sign(alice, &manifest.canonical()).unwrap();
}

fn revocation(identity: &Identity) -> DmRevocation {
    let proof = identity
        .revocation(
            identity.certificate().unwrap()["device_public_key"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    DmRevocation {
        global_identity_id: proof["global_identity_id"].as_str().unwrap().into(),
        device_public_key: proof["device_public_key"].as_str().unwrap().into(),
        signature: proof["signature"].as_str().unwrap().into(),
    }
}

fn clean_fixture(fixture: Fixture) {
    drop(fixture.store);
    std::fs::remove_file(&fixture.path).unwrap();
    std::fs::remove_file(fixture.path.with_extension("lock")).unwrap();
}

fn age_fixture_manifest(fixture: &mut Fixture, issued_at: i64, expires_at: i64) {
    let conversation = fixture
        .store
        .state
        .conversations
        .get_mut(&fixture.id)
        .unwrap();
    conversation.manifest.creator = bundle_at(
        &fixture.alice,
        &conversation.manifest.creator,
        issued_at,
        expires_at,
    );
    conversation.manifest.recipient = bundle_at(
        &fixture.bob,
        &conversation.manifest.recipient,
        issued_at,
        expires_at,
    );
    resign_manifest(&mut conversation.manifest, &fixture.alice);
    conversation.authorization_state.authorizations = vec![
        conversation.manifest.creator.clone(),
        conversation.manifest.recipient.clone(),
    ];
    let state = fixture.store.state.clone();
    fixture.store.save(state).unwrap();
}

#[test]
fn expired_authorization_offline_rejects_send_without_changing_encrypted_state() {
    let mut fixture = fixture();
    let now = chrono::Utc::now().timestamp();
    age_fixture_manifest(&mut fixture, now - 86_405, now - 5);
    let before = serde_json::to_vec(&fixture.store.state).unwrap();
    assert!(
        fixture
            .store
            .prepare(&fixture.alice, &fixture.id, "queued message")
            .is_err()
    );
    assert_eq!(serde_json::to_vec(&fixture.store.state).unwrap(), before);
    clean_fixture(fixture);
}

#[test]
fn associated_device_requires_root_intervention_and_preserves_its_keys() {
    use crate::pairing::{PendingDevice, approve, fingerprint};
    let mut fixture = fixture();
    let pending = PendingDevice::renew(&fixture.alice).unwrap();
    let response = approve(
        &fixture.alice,
        &pending.request,
        &fingerprint(&pending.request).unwrap(),
    )
    .unwrap();
    let linked = pending
        .complete(&response, &fixture.alice.root_public_key().unwrap())
        .unwrap();
    assert!(!linked.is_root());
    let mut serialized = serde_json::to_value(&linked).unwrap();
    serialized["linked_certificate"]["device_authorization"]["expires_at"] = serde_json::json!(1);
    let expired: Identity = serde_json::from_value(serialized).unwrap();
    let original = fixture.store.state.conversations[&fixture.id]
        .manifest
        .creator
        .clone();
    assert!(signed_bundle(&expired, hex::decode(&original.key_package).unwrap()).is_err());
    let renewal_request = PendingDevice::renew(&expired).unwrap();
    let hash = fingerprint(&renewal_request.request).unwrap();
    assert!(approve(&expired, &renewal_request.request, &hash).is_err());
    let response = approve(&fixture.alice, &renewal_request.request, &hash).unwrap();
    let renewed = renewal_request
        .complete(&response, &fixture.alice.root_public_key().unwrap())
        .unwrap();
    assert!(!renewed.is_root());
    assert_eq!(renewed.device_id().unwrap(), expired.device_id().unwrap());
    let bundle = signed_bundle(&renewed, hex::decode(&original.key_package).unwrap()).unwrap();
    assert!(kova_identity::dm::verify_renewal(&bundle, &original));
    fixture
        .store
        .merge_authorization_response(
            &fixture.id,
            DmAuthorizationState {
                authorizations: vec![bundle],
                revocations: Vec::new(),
            },
        )
        .unwrap();
    fixture
        .store
        .prepare(&renewed, &fixture.id, "same associated device")
        .unwrap();
    clean_fixture(fixture);
}

#[test]
fn renewal_keeps_manifest_mls_and_pending_ciphertext_across_restart() {
    let mut fixture = fixture();
    let conversation = &fixture.store.state.conversations[&fixture.id];
    let manifest = conversation.manifest.clone();
    let event_id = fixture
        .store
        .prepare(&fixture.alice, &fixture.id, "queued across renewal")
        .unwrap();
    let event = fixture.store.state.conversations[&fixture.id].events[&event_id].clone();
    let mls = fixture.store.state.crypto.clone();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let original = fixture.store.state.conversations[&fixture.id]
        .manifest
        .creator
        .clone();
    let issued_at = chrono::Utc::now().timestamp();
    let renewed = bundle_at(&fixture.alice, &original, issued_at, issued_at + 86_400);
    fixture
        .store
        .merge_authorization_response(
            &fixture.id,
            DmAuthorizationState {
                authorizations: vec![renewed],
                revocations: Vec::new(),
            },
        )
        .unwrap();
    let conversation = &fixture.store.state.conversations[&fixture.id];
    assert_eq!(conversation.manifest, manifest);
    assert_eq!(conversation.pending, vec![event_id.clone()]);
    assert_eq!(conversation.events[&event_id], event);
    assert_eq!(fixture.store.state.crypto, mls);
    drop(fixture.store);
    let reopened = DmStore::open(
        &fixture.path,
        fixture.password.expose_secret(),
        &fixture.alice,
    )
    .unwrap();
    let conversation = &reopened.state.conversations[&fixture.id];
    assert_eq!(conversation.manifest, manifest);
    assert_eq!(conversation.pending, vec![event_id.clone()]);
    assert_eq!(conversation.events[&event_id], event);
    assert_eq!(reopened.state.crypto, mls);
    fixture.store = reopened;
    clean_fixture(fixture);
}

#[test]
fn invalid_or_future_proofs_are_rejected_atomically() {
    let mut fixture = fixture();
    let original = fixture.store.state.conversations[&fixture.id]
        .manifest
        .creator
        .clone();
    let now = chrono::Utc::now().timestamp();
    let future = bundle_at(&fixture.alice, &original, now + 60, now + 86_460);
    let before = serde_json::to_vec(&fixture.store.state).unwrap();
    assert!(
        fixture
            .store
            .merge_authorization_response(
                &fixture.id,
                DmAuthorizationState {
                    authorizations: vec![future],
                    revocations: Vec::new()
                },
            )
            .is_err()
    );
    assert_eq!(serde_json::to_vec(&fixture.store.state).unwrap(), before);
    let mut invalid = bundle_at(&fixture.alice, &original, now, now + 86_400);
    let changed = if invalid.signature.starts_with("00") {
        "01"
    } else {
        "00"
    };
    invalid.signature.replace_range(0..2, changed);
    assert!(
        fixture
            .store
            .merge_authorization_response(
                &fixture.id,
                DmAuthorizationState {
                    authorizations: vec![invalid],
                    revocations: Vec::new()
                },
            )
            .is_err()
    );
    assert_eq!(serde_json::to_vec(&fixture.store.state).unwrap(), before);
    clean_fixture(fixture);
}

#[test]
fn concurrent_renewals_merge_monotonically_and_revocation_dominates() {
    let mut fixture = fixture();
    let now = chrono::Utc::now().timestamp();
    age_fixture_manifest(&mut fixture, now - 100, now + 86_300);
    let original = fixture.store.state.conversations[&fixture.id]
        .manifest
        .creator
        .clone();
    let older = bundle_at(&fixture.alice, &original, now - 60, now + 86_340);
    let newer = bundle_at(&fixture.alice, &original, now - 30, now + 86_370);
    let right_path =
        std::env::temp_dir().join(format!("kova-dm-order-{}.sqlite", uuid::Uuid::new_v4()));
    let right_password = age::x25519::Identity::generate().to_string();
    let mut right_store =
        DmStore::open(&right_path, right_password.expose_secret(), &fixture.alice).unwrap();
    right_store.save(fixture.store.state.clone()).unwrap();
    fixture
        .store
        .merge_authorization_response(
            &fixture.id,
            DmAuthorizationState {
                authorizations: vec![newer.clone()],
                revocations: Vec::new(),
            },
        )
        .unwrap();
    fixture
        .store
        .merge_authorization_response(
            &fixture.id,
            DmAuthorizationState {
                authorizations: vec![older],
                revocations: Vec::new(),
            },
        )
        .unwrap();
    right_store
        .merge_authorization_response(
            &fixture.id,
            DmAuthorizationState {
                authorizations: vec![bundle_at(&fixture.alice, &original, now - 60, now + 86_340)],
                revocations: Vec::new(),
            },
        )
        .unwrap();
    right_store
        .merge_authorization_response(
            &fixture.id,
            DmAuthorizationState {
                authorizations: vec![bundle_at(&fixture.alice, &original, now - 30, now + 86_370)],
                revocations: Vec::new(),
            },
        )
        .unwrap();
    let left_effective = &fixture.store.state.conversations[&fixture.id]
        .authorization_state
        .authorizations[0];
    let right_effective = &right_store.state.conversations[&fixture.id]
        .authorization_state
        .authorizations[0];
    assert_eq!(
        kova_identity::dm::authorization_order(left_effective),
        kova_identity::dm::authorization_order(right_effective)
    );
    assert_eq!(left_effective, right_effective);
    let proof = revocation(&fixture.alice);
    fixture
        .store
        .merge_authorization_response(
            &fixture.id,
            DmAuthorizationState {
                authorizations: vec![newer.clone()],
                revocations: vec![proof],
            },
        )
        .unwrap();
    assert!(!kova_identity::dm::conversation_authorized(
        &fixture.store.state.conversations[&fixture.id].manifest,
        &fixture.store.state.conversations[&fixture.id].authorization_state,
        now,
    ));
    // A later renewal is retained as evidence but cannot override the permanent revocation.
    fixture
        .store
        .merge_authorization_response(
            &fixture.id,
            DmAuthorizationState {
                authorizations: vec![newer],
                revocations: Vec::new(),
            },
        )
        .unwrap();
    assert!(!kova_identity::dm::conversation_authorized(
        &fixture.store.state.conversations[&fixture.id].manifest,
        &fixture.store.state.conversations[&fixture.id].authorization_state,
        now,
    ));
    drop(right_store);
    std::fs::remove_file(&right_path).unwrap();
    std::fs::remove_file(right_path.with_extension("lock")).unwrap();
    clean_fixture(fixture);
}

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
                authorization_state: kova_protocol::dm::DmAuthorizationState {
                    authorizations: vec![manifest.creator.clone(), manifest.recipient.clone()],
                    revocations: Vec::new(),
                },
                pending_creation: false,
                events: BTreeMap::new(),
                messages: BTreeMap::new(),
                pending: Vec::new(),
            },
        );
        store.save(state).unwrap();
    }
    assert!(
        a.sync_available("https://b.example", &id, |_| {
            panic!("unapproved origin must not be contacted")
        })
        .is_err()
    );
    assert!(
        a.sync_available("https://a.example", &id, |origin| {
            Ok(Session {
                origin: origin.into(),
                token: "expired-token".into(),
                expires_at: 0,
                profile: serde_json::json!({"global_identity_id": alice.global_id().unwrap()}),
            })
        })
        .is_err()
    );
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

#[test]
fn lost_creation_ack_retries_same_manifest_after_restart() {
    use std::io::{BufRead, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let alice = Identity::generate().unwrap();
    let bob = Identity::generate().unwrap();
    let path = std::env::temp_dir().join(format!("kova-creation-{}.sqlite", uuid::Uuid::new_v4()));
    let key = age::x25519::Identity::generate().to_string();
    let mut store = DmStore::open(&path, key.expose_secret(), &alice).unwrap();
    let mut crypto = CryptoClient::restore_state(&store.state.crypto).unwrap();
    let peer_crypto =
        CryptoClient::new(&bob.global_id().unwrap(), &bob.device_id().unwrap()).unwrap();
    let creator = signed_bundle(&alice, crypto.key_package().unwrap()).unwrap();
    let recipient = signed_bundle(&bob, peer_crypto.key_package().unwrap()).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let welcome = crypto
        .create_conversation(&id, &hex::decode(&recipient.key_package).unwrap())
        .unwrap();
    let mut manifest = ConversationManifest {
        conversation_id: id.clone(),
        origin: origin.clone(),
        creator,
        recipient,
        welcome: hex::encode(welcome),
        signature: String::new(),
    };
    manifest.signature = sign(&alice, &manifest.canonical()).unwrap();
    let expected = serde_json::to_value(&manifest).unwrap();
    let mut state = store.state.clone();
    state.crypto = crypto.export_state().unwrap();
    state.conversations.insert(
        id.clone(),
        Conversation {
            manifest: manifest.clone(),
            cursor: 0,
            source_cursors: BTreeMap::new(),
            active_policy: None,
            authorization_state: kova_protocol::dm::DmAuthorizationState {
                authorizations: vec![manifest.creator.clone(), manifest.recipient.clone()],
                revocations: Vec::new(),
            },
            pending_creation: true,
            events: BTreeMap::new(),
            messages: BTreeMap::new(),
            pending: Vec::new(),
        },
    );
    store.save(state).unwrap();
    let crypto_before = store.state.crypto.clone();
    let request_id = id.clone();
    let server = std::thread::spawn(move || {
        let mut creations = Vec::new();
        // First creation commits, but its response is lost. The retry returns
        // that same manifest; no history GET is valid until creation is confirmed.
        for attempt in 0..4 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut reader = std::io::BufReader::new(&mut socket);
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" {
                    break;
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            drop(reader);
            if attempt % 2 == 0 {
                assert!(request.starts_with(&format!(
                    "GET /api/v1/dm/conversations/{request_id}/replicas/history "
                )));
                let body = r#"{"error":{"message":"not found"}}"#;
                write!(socket, "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            } else {
                assert!(request.starts_with("POST /api/v1/dm/conversations "));
                let received: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(received, expected);
                creations.push(received);
                if attempt == 3 {
                    let body = expected.to_string();
                    write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
                }
            }
        }
        assert_eq!(creations.len(), 2);
        assert_eq!(creations[0], creations[1]);
    });
    let session = || Session {
        origin: origin.clone(),
        token: "test-token".into(),
        expires_at: chrono::Utc::now().timestamp() + 60,
        profile: serde_json::json!({"global_identity_id": alice.global_id().unwrap()}),
    };
    assert!(
        store
            .retry_available(&origin, &id, |_| Ok(session()))
            .is_err()
    );
    assert!(store.state.conversations[&id].pending_creation);
    drop(store);
    let mut store = DmStore::open(&path, key.expose_secret(), &alice).unwrap();
    assert_eq!(
        store
            .retry_available(&origin, &id, |_| Ok(session()))
            .unwrap(),
        origin
    );
    assert!(!store.state.conversations[&id].pending_creation);
    assert_eq!(store.state.crypto, crypto_before);
    server.join().unwrap();
    drop(store);
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(path.with_extension("lock")).unwrap();
}
