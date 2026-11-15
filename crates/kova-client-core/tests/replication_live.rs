//! Live client/server replication flow; invoked by the disposable federation harness.
use age::secrecy::ExposeSecret;
use kova_client_core::{Identity, Instance, chat::PendingMessage, dm::DmStore};

fn join(
    instance: &Instance,
    identity: &Identity,
    invite: &str,
    handle: &str,
) -> kova_client_core::Session {
    instance
        .login(identity, Some((invite, handle, handle)))
        .unwrap()
}

#[test]
#[ignore = "requires the disposable A/B federation harness"]
fn bilateral_policy_syncs_mls_events_across_two_origins() {
    let origin_a = std::env::var("KOVA_REPLICATION_ORIGIN_A").unwrap();
    let origin_b = std::env::var("KOVA_REPLICATION_ORIGIN_B").unwrap();
    let a = Instance::new(&origin_a).unwrap();
    let b = Instance::new(&origin_b).unwrap();
    let alice = Identity::generate().unwrap();
    let bob = Identity::generate().unwrap();
    let alice_a = join(
        &a,
        &alice,
        &std::env::var("KOVA_REPLICATION_INVITE_ALICE_A").unwrap(),
        "replica_alice_a",
    );
    let alice_b = join(
        &b,
        &alice,
        &std::env::var("KOVA_REPLICATION_INVITE_ALICE_B").unwrap(),
        "replica_alice_b",
    );
    let bob_a = join(
        &a,
        &bob,
        &std::env::var("KOVA_REPLICATION_INVITE_BOB_A").unwrap(),
        "replica_bob_a",
    );
    let bob_b = join(
        &b,
        &bob,
        &std::env::var("KOVA_REPLICATION_INVITE_BOB_B").unwrap(),
        "replica_bob_b",
    );

    let directory =
        std::env::temp_dir().join(format!("kova-replication-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let alice_path = directory.join("alice.sqlite");
    let bob_path = directory.join("bob.sqlite");
    let alice_key = age::x25519::Identity::generate().to_string();
    let bob_key = age::x25519::Identity::generate().to_string();

    let mut alice_store = DmStore::open(&alice_path, alice_key.expose_secret(), &alice).unwrap();
    let mut bob_store = DmStore::open(&bob_path, bob_key.expose_secret(), &bob).unwrap();

    let community_a = a.communities(&alice_a).unwrap()["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let communities_b = b.communities(&alice_b).unwrap();
    assert!(
        !communities_b["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|community| community["id"] == community_a)
    );
    assert!(b.channels(&alice_b, &community_a).is_err());
    let channel_a = a.channels(&alice_a, &community_a).unwrap()["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    a.send_message(
        &alice_a,
        &channel_a,
        &PendingMessage {
            client_id: uuid::Uuid::new_v4().to_string(),
            body: "alice-public-on-a".into(),
        },
    )
    .unwrap();
    let public_messages = a.messages(&bob_a, &channel_a, 0).unwrap();
    assert!(
        public_messages
            .items
            .iter()
            .any(|message| message.body == "alice-public-on-a")
    );

    bob_store.publish(&a, &bob_a, &bob).unwrap();
    let conversation = alice_store
        .create(
            &a,
            &alice_a,
            &alice,
            &bob.global_id().unwrap(),
            &bob.device_id().unwrap(),
        )
        .unwrap();
    alice_store
        .send(
            &a,
            &alice_a,
            &alice,
            &conversation,
            "alice-origin-a-message",
        )
        .unwrap();
    bob_store
        .accept(
            &a,
            &bob_a,
            &bob,
            &conversation,
            &alice.global_id().unwrap(),
            &alice.device_id().unwrap(),
        )
        .unwrap();
    bob_store.sync(&a, &bob_a, &conversation).unwrap();

    // Build and inspect the exact candidate before either participant signs it.
    alice_store
        .propose_replica(&a, &alice_a, &alice, &conversation, &origin_b)
        .unwrap();
    alice_store
        .replica_proposal(&a, &alice_a, &conversation)
        .unwrap();
    let first_approval = alice_store
        .approve_replica_proposal(&a, &alice_a, &alice, &conversation)
        .unwrap();
    assert_eq!(first_approval.approvals.len(), 1);
    bob_store
        .replica_proposal(&a, &bob_a, &conversation)
        .unwrap();
    let active = bob_store
        .approve_replica_proposal(&a, &bob_a, &bob, &conversation)
        .unwrap();
    assert_eq!(active.approvals.len(), 2);
    assert!(kova_identity::replication::verify_policy(
        &active,
        &a.dm_conversation(&alice_a, &conversation).unwrap()
    ));

    // A→B seeds B with the approved policy, manifest, and ciphertext. Then Alice
    // sends directly through B, and B→A converges without a central relay.
    alice_store
        .sync_replicas(&a, &alice_a, &conversation, &origin_b)
        .unwrap();
    alice_store
        .send(
            &b,
            &alice_b,
            &alice,
            &conversation,
            "alice-through-b-message",
        )
        .unwrap();
    bob_store.sync(&b, &bob_b, &conversation).unwrap();
    bob_store
        .send(&b, &bob_b, &bob, &conversation, "bob-through-b-message")
        .unwrap();
    bob_store
        .sync_replicas(&b, &bob_b, &conversation, &origin_a)
        .unwrap();
    alice_store.sync(&a, &alice_a, &conversation).unwrap();

    for message in [
        "alice-origin-a-message",
        "alice-through-b-message",
        "bob-through-b-message",
    ] {
        assert!(
            alice_store
                .render(Some(&conversation))
                .unwrap()
                .contains(message)
        );
        assert!(
            bob_store
                .render(Some(&conversation))
                .unwrap()
                .contains(message)
        );
    }
    let alice_history = alice_store.render(Some(&conversation)).unwrap();
    let bob_history = bob_store.render(Some(&conversation)).unwrap();
    assert_eq!(
        alice_history.matches("· reçu par le serveur").count(),
        bob_history.matches("· reçu par le serveur").count(),
        "both local caches must converge on the same number of events"
    );

    // Leave one acknowledged event on A only so retrying on B must replay the
    // missing causal ancestor before the durable pending event.
    alice_store
        .send(&a, &alice_a, &alice, &conversation, "primary-only-parent")
        .unwrap();

    // Coordinate a real primary outage with the Python federation harness.
    let handshake = std::env::var("KOVA_REPLICATION_FAILOVER_HANDSHAKE").unwrap();
    let handshake = std::path::PathBuf::from(handshake);
    let pending = alice_store
        .prepare(&alice, &conversation, "retry-on-approved-replica")
        .unwrap();
    drop(alice_store);
    let mut alice_store = DmStore::open(&alice_path, alice_key.expose_secret(), &alice).unwrap();
    std::fs::write(handshake.join("ready"), b"").unwrap();
    wait_for_file(&handshake.join("stopped"));
    let alice_a_session = serde_json::to_value(&alice_a).unwrap();
    let alice_b_session = serde_json::to_value(&alice_b).unwrap();
    let selected = alice_store
        .retry_available(&origin_a, &conversation, |origin| {
            let session = if origin == origin_a {
                &alice_a_session
            } else if origin == origin_b {
                &alice_b_session
            } else {
                return Err("unexpected replica".into());
            };
            Ok(serde_json::from_value(session.clone()).unwrap())
        })
        .unwrap();
    assert_eq!(selected, origin_b);
    assert!(
        alice_store
            .render(Some(&conversation))
            .unwrap()
            .contains("retry-on-approved-replica")
    );
    std::fs::write(handshake.join("restart"), b"").unwrap();
    wait_for_file(&handshake.join("resumed"));
    let selected = alice_store
        .retry_available(&origin_a, &conversation, |origin| {
            let session = if origin == origin_a {
                &alice_a_session
            } else if origin == origin_b {
                &alice_b_session
            } else {
                return Err("unexpected replica".into());
            };
            Ok(serde_json::from_value(session.clone()).unwrap())
        })
        .unwrap();
    assert_eq!(selected, origin_a);
    let final_history = alice_store.render(Some(&conversation)).unwrap();
    assert_eq!(
        final_history.matches("retry-on-approved-replica").count(),
        1
    );
    assert!(!final_history.contains("en attente"));
    let page: kova_protocol::dm::DmEventPage = reqwest::blocking::Client::new()
        .get(format!(
            "{origin_a}/api/v1/dm/conversations/{conversation}/events"
        ))
        .bearer_auth(alice_a_session["token"].as_str().unwrap())
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        page.items
            .iter()
            .filter(|item| item.event.event_id == pending)
            .count(),
        1
    );
    bob_store.sync(&a, &bob_a, &conversation).unwrap();
    let bob_after_failover = bob_store.render(Some(&conversation)).unwrap();
    assert!(bob_after_failover.contains("primary-only-parent"));
    assert!(bob_after_failover.contains("retry-on-approved-replica"));

    for invalid_session in [&alice_b_session, &serde_json::to_value(&bob_a).unwrap()] {
        let mut requested = Vec::new();
        assert!(
            alice_store
                .sync_available(&origin_a, &conversation, |origin| {
                    requested.push(origin.to_owned());
                    Ok(serde_json::from_value(invalid_session.clone()).unwrap())
                })
                .is_err()
        );
        assert_eq!(requested, vec![origin_a.clone()]);
    }

    // Only a missing/expired credential permits trying another approved origin.
    let selected = alice_store
        .sync_available(&origin_a, &conversation, |origin| {
            if origin == origin_a {
                Err(kova_client_core::SessionUnavailable.into())
            } else {
                assert_eq!(origin, origin_b);
                Ok(serde_json::from_value(alice_b_session.clone()).unwrap())
            }
        })
        .unwrap();
    assert_eq!(selected, origin_b);
    let mut expired = alice_a_session.clone();
    expired["expires_at"] = serde_json::json!(0);
    assert_eq!(
        alice_store
            .sync_available(&origin_a, &conversation, |origin| {
                Ok(serde_json::from_value(if origin == origin_a {
                    expired.clone()
                } else {
                    assert_eq!(origin, origin_b);
                    alice_b_session.clone()
                })
                .unwrap())
            })
            .unwrap(),
        origin_b
    );

    // An authorization denial must stop selection rather than bypass it on B.
    let mut denied = alice_a_session.clone();
    denied["token"] = serde_json::json!("00".repeat(32));
    let mut requested = Vec::new();
    assert!(
        alice_store
            .sync_available(&origin_a, &conversation, |origin| {
                requested.push(origin.to_owned());
                Ok(serde_json::from_value(denied.clone()).unwrap())
            })
            .is_err()
    );
    assert_eq!(requested, vec![origin_a.clone()]);
    let mut requested = false;
    assert!(
        alice_store
            .sync_available("https://unapproved.example", &conversation, |_| {
                requested = true;
                Err(kova_client_core::SessionUnavailable.into())
            })
            .is_err()
    );
    assert!(!requested);
    drop(alice_store);
    drop(bob_store);
    for path in [&alice_path, &bob_path] {
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(path.with_extension("lock")).unwrap();
    }
    std::fs::remove_dir(directory).unwrap();
}

fn wait_for_file(path: &std::path::Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
    while !path.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
