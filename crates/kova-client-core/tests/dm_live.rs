//! Run by the disposable PostgreSQL harness in kova-server/tests/dm.py.
use age::secrecy::ExposeSecret;
use kova_client_core::{Identity, Instance, dm::DmStore};

#[test]
#[ignore = "requires the disposable PostgreSQL instance created by dm.py"]
fn private_messages_offline_restart_and_device_verification() {
    let origin = std::env::var("KOVA_TEST_ORIGIN").unwrap();
    let instance = Instance::new(&origin).unwrap();
    let alice = Identity::generate().unwrap();
    let bob = Identity::generate().unwrap();
    let alice_session = instance
        .login(
            &alice,
            Some((
                &std::env::var("KOVA_DM_INVITE_ALICE").unwrap(),
                "rust_dm_alice",
                "Rust Alice",
            )),
        )
        .unwrap();
    let bob_session = instance
        .login(
            &bob,
            Some((
                &std::env::var("KOVA_DM_INVITE_BOB").unwrap(),
                "rust_dm_bob",
                "Rust Bob",
            )),
        )
        .unwrap();
    let directory = std::env::temp_dir().join(format!("kova-dm-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let alice_path = directory.join("alice.sqlite");
    let bob_path = directory.join("bob.sqlite");
    let alice_key = age::x25519::Identity::generate().to_string();
    let bob_key = age::x25519::Identity::generate().to_string();
    let conversation;
    {
        let mut a = DmStore::open(&alice_path, alice_key.expose_secret(), &alice).unwrap();
        let mut b = DmStore::open(&bob_path, bob_key.expose_secret(), &bob).unwrap();
        b.publish(&instance, &bob_session, &bob).unwrap();
        drop(b); // Bob is offline before creation and first message.
        assert!(
            a.create(
                &instance,
                &alice_session,
                &alice,
                &bob.global_id().unwrap(),
                &alice.device_id().unwrap()
            )
            .is_err()
        );
        conversation = a
            .create(
                &instance,
                &alice_session,
                &alice,
                &bob.global_id().unwrap(),
                &bob.device_id().unwrap(),
            )
            .unwrap();
        a.send(
            &instance,
            &alice_session,
            &alice,
            &conversation,
            "secret-alice-offline-🦀",
        )
        .unwrap();
    }
    {
        let mut b = DmStore::open(&bob_path, bob_key.expose_secret(), &bob).unwrap();
        assert!(b.publish(&instance, &bob_session, &bob).is_err()); // Consumed packages cannot be republished.
        assert!(
            b.accept(
                &instance,
                &bob_session,
                &bob,
                &conversation,
                &alice.global_id().unwrap(),
                &bob.device_id().unwrap()
            )
            .is_err()
        );
        b.accept(
            &instance,
            &bob_session,
            &bob,
            &conversation,
            &alice.global_id().unwrap(),
            &alice.device_id().unwrap(),
        )
        .unwrap();
        b.sync(&instance, &bob_session, &conversation).unwrap();
        assert!(
            b.render(Some(&conversation))
                .unwrap()
                .contains("secret-alice-offline-🦀")
        );
        b.prepare(&bob, &conversation, "secret-bob-pending-restart")
            .unwrap();
        assert!(
            b.render(Some(&conversation))
                .unwrap()
                .contains("en attente")
        );
    }
    {
        let mut b = DmStore::open(&bob_path, bob_key.expose_secret(), &bob).unwrap();
        b.retry(&instance, &bob_session, &conversation).unwrap();
        b.retry(&instance, &bob_session, &conversation).unwrap();
        b.sync(&instance, &bob_session, &conversation).unwrap();
        let mut a = DmStore::open(&alice_path, alice_key.expose_secret(), &alice).unwrap();
        a.sync(&instance, &alice_session, &conversation).unwrap();
        a.sync(&instance, &alice_session, &conversation).unwrap();
        assert_eq!(
            a.render(Some(&conversation))
                .unwrap()
                .matches("secret-bob-pending-restart")
                .count(),
            1
        );
        a.send(
            &instance,
            &alice_session,
            &alice,
            &conversation,
            "secret-alice-after-restart",
        )
        .unwrap();
        b.sync(&instance, &bob_session, &conversation).unwrap();
        assert!(
            b.render(Some(&conversation))
                .unwrap()
                .contains("secret-alice-after-restart")
        );
        let events = reqwest::blocking::Client::new()
            .get(format!(
                "{origin}/api/v1/dm/conversations/{conversation}/events"
            ))
            .bearer_auth(
                serde_json::to_value(&alice_session).unwrap()["token"]
                    .as_str()
                    .unwrap(),
            )
            .send()
            .unwrap()
            .text()
            .unwrap();
        assert!(!events.contains("secret-alice") && !events.contains("secret-bob"));
        assert!(!b.render(None).unwrap().is_empty());
    }
    for path in [&alice_path, &bob_path] {
        let bytes = std::fs::read(path).unwrap();
        for secret in ["secret-alice", "secret-bob"] {
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|part| part == secret.as_bytes())
            );
        }
    }
    for entry in std::fs::read_dir(&directory).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    std::fs::remove_dir(directory).unwrap();
}
