//! Live client/server replication flow; invoked by the disposable federation harness.
use age::secrecy::ExposeSecret;
use kova_client_core::{Identity, Instance, dm::DmStore};

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
    drop(alice_store);
    drop(bob_store);
    for path in [&alice_path, &bob_path] {
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(path.with_extension("lock")).unwrap();
    }
    std::fs::remove_dir(directory).unwrap();
}
