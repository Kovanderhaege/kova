use crate::{
    identity_id,
    replication::{policy_hash, valid_policy, verify_approval, verify_policy},
};
use kova_protocol::{
    device_authorization_v2,
    dm::{ConversationManifest, DeviceBundle},
    replication::{ReplicaApproval, ReplicaDescriptor, ReplicaPolicy},
};
use ring::signature::{Ed25519KeyPair, KeyPair};

fn key(seed: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).expect("fixed seed")
}

fn signed_bundle(root: &Ed25519KeyPair, device: &Ed25519KeyPair) -> DeviceBundle {
    let root_public_key = hex::encode(root.public_key().as_ref());
    let device_public_key = hex::encode(device.public_key().as_ref());
    let global_identity_id = identity_id(root.public_key().as_ref());
    let device_id = hex::encode(
        ring::digest::digest(&ring::digest::SHA256, device.public_key().as_ref()).as_ref(),
    );
    let issued_at = 100;
    let expires_at = 86_500;
    let authorization = device_authorization_v2(
        &global_identity_id,
        &device_id,
        &device_public_key,
        issued_at,
        expires_at,
    );
    let mut bundle = DeviceBundle {
        global_identity_id,
        device_id,
        root_public_key,
        device_public_key,
        issued_at,
        expires_at,
        authorization_signature: hex::encode(root.sign(authorization.as_bytes()).as_ref()),
        key_package: "aa".repeat(32),
        signature: String::new(),
    };
    bundle.signature = hex::encode(device.sign(bundle.canonical().as_bytes()).as_ref());
    bundle
}

fn fixture() -> (ConversationManifest, Ed25519KeyPair, Ed25519KeyPair) {
    let root_a = key(1);
    let device_a = key(2);
    let root_b = key(3);
    let device_b = key(4);
    let creator = signed_bundle(&root_a, &device_a);
    let recipient = signed_bundle(&root_b, &device_b);
    let mut manifest = ConversationManifest {
        conversation_id: "123e4567-e89b-42d3-a456-426614174000".into(),
        origin: "https://a.example".into(),
        creator,
        recipient,
        welcome: "aa".into(),
        signature: String::new(),
    };
    manifest.signature = hex::encode(device_a.sign(manifest.canonical().as_bytes()).as_ref());
    (manifest, device_a, device_b)
}

fn candidate(manifest: &ConversationManifest) -> ReplicaPolicy {
    ReplicaPolicy {
        conversation_id: manifest.conversation_id.clone(),
        version: 1,
        previous_hash: None,
        replicas: vec![
            ReplicaDescriptor {
                origin: "https://a.example".into(),
                public_key: "00".repeat(32),
            },
            ReplicaDescriptor {
                origin: "https://b.example".into(),
                public_key: "11".repeat(32),
            },
        ],
        approvals: Vec::new(),
    }
}

fn approval(
    device: &Ed25519KeyPair,
    policy: &ReplicaPolicy,
    identity: &str,
    device_id: &str,
) -> ReplicaApproval {
    ReplicaApproval {
        identity: identity.into(),
        device: device_id.into(),
        signature: hex::encode(device.sign(policy.canonical().as_bytes()).as_ref()),
    }
}

#[test]
fn both_participants_must_sign_the_same_canonical_policy() {
    let (manifest, device_a, device_b) = fixture();
    let mut policy = candidate(&manifest);
    assert!(valid_policy(&policy, &manifest));
    assert!(!verify_policy(&policy, &manifest));
    let a = approval(
        &device_a,
        &policy,
        &manifest.creator.global_identity_id,
        &manifest.creator.device_id,
    );
    assert!(verify_approval(&policy, &a, &manifest));
    policy.approvals.push(a);
    let b = approval(
        &device_b,
        &policy,
        &manifest.recipient.global_identity_id,
        &manifest.recipient.device_id,
    );
    assert!(verify_approval(&policy, &b, &manifest));
    policy.approvals.push(b);
    assert!(verify_policy(&policy, &manifest));
    assert_eq!(policy_hash(&policy), policy_hash(&candidate(&manifest)));
}

#[test]
fn rejects_missing_duplicate_wrong_and_tampered_approvals() {
    let (manifest, device_a, device_b) = fixture();
    let mut policy = candidate(&manifest);
    let a = approval(
        &device_a,
        &policy,
        &manifest.creator.global_identity_id,
        &manifest.creator.device_id,
    );
    let b = approval(
        &device_b,
        &policy,
        &manifest.recipient.global_identity_id,
        &manifest.recipient.device_id,
    );
    assert!(!verify_approval(
        &policy,
        &approval(
            &device_b,
            &policy,
            &manifest.creator.global_identity_id,
            &manifest.creator.device_id
        ),
        &manifest
    ));
    policy.approvals = vec![a.clone(), a];
    assert!(!verify_policy(&policy, &manifest));
    let mut changed = candidate(&manifest);
    changed.replicas[1].origin = "https://evil.example".into();
    changed.approvals = vec![
        approval(
            &device_a,
            &changed,
            &manifest.creator.global_identity_id,
            &manifest.creator.device_id,
        ),
        b.clone(),
    ];
    assert!(!verify_policy(&changed, &manifest));
    changed = candidate(&manifest);
    changed.approvals = vec![
        approval(
            &device_a,
            &changed,
            &manifest.creator.global_identity_id,
            &manifest.creator.device_id,
        ),
        b,
    ];
    changed.version = 2;
    changed.previous_hash = Some("22".repeat(32));
    assert!(!verify_policy(&changed, &manifest));
}

#[test]
fn validates_conversation_binding_versions_keys_order_and_limits() {
    let (manifest, _, _) = fixture();
    let base = candidate(&manifest);
    assert!(valid_policy(&base, &manifest));
    let mut changed = base.clone();
    changed.conversation_id = "123e4567-e89b-42d3-a456-426614174001".into();
    assert!(!valid_policy(&changed, &manifest));
    changed = base.clone();
    changed.version = 0;
    assert!(!valid_policy(&changed, &manifest));
    changed = base.clone();
    changed.version = 2;
    changed.previous_hash = None;
    assert!(!valid_policy(&changed, &manifest));
    changed.previous_hash = Some("not-a-hash".into());
    assert!(!valid_policy(&changed, &manifest));
    changed = base.clone();
    changed.replicas[0].public_key = "GG".repeat(32);
    assert!(!valid_policy(&changed, &manifest));
    changed = base.clone();
    changed.replicas.swap(0, 1);
    assert!(!valid_policy(&changed, &manifest));
    changed = base.clone();
    changed.replicas[0].origin = "https://a.example\nX: injected".into();
    assert!(!valid_policy(&changed, &manifest));
    changed = base;
    changed.replicas.clear();
    assert!(!valid_policy(&changed, &manifest));
}
