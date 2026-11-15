//! Connected Windows desktop recipe setup, called by desktop_recipe.py.

#[cfg(windows)]
#[test]
#[ignore = "requires the connected Windows desktop recipe fixture"]
fn creates_synthetic_alice_bob_profiles_and_replica_conversation() {
    use kova_client_core::{Identity, Instance, dm::DmStore, vault::Vault};
    use serde_json::json;

    fn required(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| panic!("missing fixture variable {name}"))
    }

    let recipe_id = required("KOVA_DESKTOP_RECIPE_ID");
    let directory = std::path::PathBuf::from(required("KOVA_DESKTOP_RECIPE_DIR"));
    let origin_a = required("KOVA_DESKTOP_RECIPE_ORIGIN_A");
    let origin_b = required("KOVA_DESKTOP_RECIPE_ORIGIN_B");
    let a = Instance::new(&origin_a).unwrap();
    let b = Instance::new(&origin_b).unwrap();

    let profile_suffix = &recipe_id[..12];
    let alice_profile = format!("kova-{profile_suffix}-alice");
    let bob_profile = format!("kova-{profile_suffix}-bob");
    let alice = Identity::generate().unwrap();
    let bob = Identity::generate().unwrap();
    let alice_vault = Vault::new(&alice_profile).unwrap();
    let bob_vault = Vault::new(&bob_profile).unwrap();
    alice_vault.install(&alice).unwrap();
    bob_vault.install(&bob).unwrap();

    let alice_a = a
        .login(
            &alice,
            Some((
                &required("KOVA_DESKTOP_RECIPE_INVITE_ALICE_A"),
                &format!("alice_{profile_suffix}"),
                "Recipe Alice",
            )),
        )
        .unwrap();
    let alice_b = b
        .login(
            &alice,
            Some((
                &required("KOVA_DESKTOP_RECIPE_INVITE_ALICE_B"),
                &format!("alice_{profile_suffix}"),
                "Recipe Alice",
            )),
        )
        .unwrap();
    let bob_a = a
        .login(
            &bob,
            Some((
                &required("KOVA_DESKTOP_RECIPE_INVITE_BOB_A"),
                &format!("bob_{profile_suffix}"),
                "Recipe Bob",
            )),
        )
        .unwrap();
    let bob_b = b
        .login(
            &bob,
            Some((
                &required("KOVA_DESKTOP_RECIPE_INVITE_BOB_B"),
                &format!("bob_{profile_suffix}"),
                "Recipe Bob",
            )),
        )
        .unwrap();
    for (vault, session) in [
        (&alice_vault, &alice_a),
        (&alice_vault, &alice_b),
        (&bob_vault, &bob_a),
        (&bob_vault, &bob_b),
    ] {
        vault.save_session(session).unwrap();
    }

    let alice_secret = alice_vault.dm_secret().unwrap();
    let bob_secret = bob_vault.dm_secret().unwrap();
    let mut alice_store = DmStore::open(
        &alice_vault.directory().unwrap().join("dm.sqlite"),
        alice_secret.as_str(),
        &alice,
    )
    .unwrap();
    let mut bob_store = DmStore::open(
        &bob_vault.directory().unwrap().join("dm.sqlite"),
        bob_secret.as_str(),
        &bob,
    )
    .unwrap();

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
            "desktop-recipe-initial-message",
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
    assert!(
        bob_store
            .render(Some(&conversation))
            .unwrap()
            .contains("desktop-recipe-initial-message")
    );

    alice_store
        .propose_replica(&a, &alice_a, &alice, &conversation, &origin_b)
        .unwrap();
    alice_store
        .approve_replica_proposal(&a, &alice_a, &alice, &conversation)
        .unwrap();
    bob_store
        .replica_proposal(&a, &bob_a, &conversation)
        .unwrap();
    bob_store
        .approve_replica_proposal(&a, &bob_a, &bob, &conversation)
        .unwrap();
    alice_store
        .sync_replicas(&a, &alice_a, &conversation, &origin_b)
        .unwrap();
    bob_store.sync(&b, &bob_b, &conversation).unwrap();
    assert!(
        bob_store
            .render(Some(&conversation))
            .unwrap()
            .contains("desktop-recipe-initial-message")
    );

    let state = json!({
        "recipe_id": recipe_id,
        "origins": {"a": origin_a, "b": origin_b},
        "profiles": {"alice": alice_profile, "bob": bob_profile},
        "identities": {
            "alice": {
                "global_identity_id": alice.global_id().unwrap(),
                "device_id": alice.device_id().unwrap(),
            },
            "bob": {
                "global_identity_id": bob.global_id().unwrap(),
                "device_id": bob.device_id().unwrap(),
            },
        },
        "communities": json!({
            "a": required("KOVA_DESKTOP_RECIPE_COMMUNITY_A"),
            "b": required("KOVA_DESKTOP_RECIPE_COMMUNITY_B"),
        }),
        "channels": json!({
            "a": required("KOVA_DESKTOP_RECIPE_CHANNEL_A"),
            "b": required("KOVA_DESKTOP_RECIPE_CHANNEL_B"),
        }),
        "conversation_id": conversation,
        "instructions": format!(
            "Synthetic identities remain in Windows Credential Manager and in %LOCALAPPDATA%\\Kova\\profiles\\{alice_profile} and %LOCALAPPDATA%\\Kova\\profiles\\{bob_profile}; they do not affect the default profile."
        ),
    });
    std::fs::write(
        directory.join("state.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();
}
