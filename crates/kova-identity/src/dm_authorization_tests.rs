use crate::{
    dm::*,
    replication_tests::{fixture, key},
};
use kova_protocol::dm::{DeviceBundle, DmAuthorizationState, DmRevocation};
use ring::signature::Ed25519KeyPair;

fn renew(
    original: &DeviceBundle,
    root: &Ed25519KeyPair,
    device: &Ed25519KeyPair,
    issued: i64,
    expires: i64,
) -> DeviceBundle {
    let mut bundle = original.clone();
    bundle.issued_at = issued;
    bundle.expires_at = expires;
    bundle.authorization_signature = hex::encode(
        root.sign(
            kova_protocol::device_authorization_v2(
                &bundle.global_identity_id,
                &bundle.device_id,
                &bundle.device_public_key,
                issued,
                expires,
            )
            .as_bytes(),
        )
        .as_ref(),
    );
    bundle.signature = hex::encode(device.sign(bundle.canonical().as_bytes()).as_ref());
    bundle
}

#[test]
fn expiration_requires_both_renewals_without_changing_historical_manifest() {
    let (manifest, alice, bob) = fixture();
    let mut state = DmAuthorizationState::default();
    assert!(conversation_authorized(&manifest, &state, 86_499));
    assert!(!conversation_authorized(&manifest, &state, 86_500));
    assert!(verify_conversation_historical(&manifest));
    state
        .authorizations
        .push(renew(&manifest.creator, &key(1), &alice, 86_400, 172_800));
    assert!(!conversation_authorized(&manifest, &state, 86_500));
    state
        .authorizations
        .push(renew(&manifest.recipient, &key(3), &bob, 86_400, 172_800));
    assert!(conversation_authorized(&manifest, &state, 86_500));
    assert!(!conversation_authorized(&manifest, &state, 172_800));
    assert!(verify_conversation_historical(&manifest));
}

#[test]
fn same_manifest_survives_multiple_daily_expirations() {
    let (manifest, alice, bob) = fixture();
    let mut state = DmAuthorizationState::default();
    for day in 1..=3 {
        let issued = 100 + day * 86_400;
        assert!(!conversation_authorized(&manifest, &state, issued));
        state.authorizations = vec![
            renew(&manifest.creator, &key(1), &alice, issued, issued + 86_400),
            renew(&manifest.recipient, &key(3), &bob, issued, issued + 86_400),
        ];
        assert!(conversation_authorized(&manifest, &state, issued));
        assert!(conversation_authorized(&manifest, &state, issued + 86_399));
        assert!(!conversation_authorized(&manifest, &state, issued + 86_400));
    }
    assert!(verify_conversation_historical(&manifest));
}

#[test]
fn renewal_rejects_key_package_substitution_invalid_root_and_long_validity() {
    let (manifest, alice, _) = fixture();
    let valid = renew(&manifest.creator, &key(1), &alice, 86_400, 172_800);
    assert!(verify_renewal(&valid, &manifest.creator));
    let invalid_root = renew(&manifest.creator, &key(5), &alice, 86_400, 172_800);
    assert!(!verify_renewal(&invalid_root, &manifest.creator));
    let long = renew(&manifest.creator, &key(1), &alice, 86_400, 172_801);
    assert!(!verify_renewal(&long, &manifest.creator));
    let mut changed = valid.clone();
    changed.key_package = "ab".repeat(32);
    changed.signature = hex::encode(alice.sign(changed.canonical().as_bytes()).as_ref());
    assert!(!verify_renewal(&changed, &manifest.creator));
    let duplicate = DmAuthorizationState {
        authorizations: vec![valid.clone(), valid],
        ..Default::default()
    };
    assert!(!verify_authorization_state(&duplicate, &manifest));
}

#[test]
fn revocation_dominates_renewal_and_does_not_invalidate_historical_signatures() {
    let (manifest, alice, _) = fixture();
    let root = key(1);
    let proof = DmRevocation {
        global_identity_id: manifest.creator.global_identity_id.clone(),
        device_public_key: manifest.creator.device_public_key.clone(),
        signature: hex::encode(
            root.sign(
                kova_protocol::device_revocation(
                    &manifest.creator.global_identity_id,
                    &manifest.creator.device_id,
                )
                .as_bytes(),
            )
            .as_ref(),
        ),
    };
    assert!(verify_revocation(&proof, &manifest));
    let mut state = DmAuthorizationState {
        authorizations: vec![renew(&manifest.creator, &root, &alice, 200, 86_600)],
        revocations: vec![proof.clone()],
    };
    assert!(verify_authorization_state(&state, &manifest));
    assert!(!conversation_authorized(&manifest, &state, 201));
    assert!(verify_conversation_historical(&manifest));
    state.revocations[0].signature = "00".repeat(64);
    assert!(!verify_authorization_state(&state, &manifest));
}

#[test]
fn concurrent_proofs_have_deterministic_order_and_future_proof_cannot_authorize() {
    let (manifest, alice, _) = fixture();
    let first = renew(&manifest.creator, &key(1), &alice, 200, 500);
    let second = renew(&manifest.creator, &key(1), &alice, 200, 600);
    let third = renew(&manifest.creator, &key(1), &alice, 201, 400);
    assert!(authorization_order(&first) < authorization_order(&second));
    assert!(authorization_order(&second) < authorization_order(&third));
    let state = DmAuthorizationState {
        authorizations: vec![third],
        revocations: vec![],
    };
    assert!(!conversation_authorized(&manifest, &state, 200));
    assert!(conversation_authorized(&manifest, &state, 201));
}
