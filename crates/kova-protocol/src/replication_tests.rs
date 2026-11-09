use crate::replication::{ReplicaApproval, ReplicaDescriptor, ReplicaPolicy};

fn policy() -> ReplicaPolicy {
    ReplicaPolicy {
        conversation_id: "123e4567-e89b-42d3-a456-426614174000".into(),
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

#[test]
fn canonical_bytes_are_domain_separated_and_exclude_approvals() {
    let mut first = policy();
    let canonical = first.canonical();
    assert!(canonical.starts_with("[\"kova-dm-replica-policy-v1\","));
    first.approvals.push(ReplicaApproval {
        identity: "kova1-ignored".into(),
        device: "ignored".into(),
        signature: "ignored".into(),
    });
    assert_eq!(first.canonical(), canonical);
    first.replicas.swap(0, 1);
    assert_ne!(first.canonical(), canonical);
}

#[test]
fn strict_deserialization_rejects_unknown_policy_fields() {
    let value = serde_json::json!({
        "conversation_id": "123e4567-e89b-42d3-a456-426614174000",
        "version": 1,
        "previous_hash": null,
        "replicas": [],
        "approvals": [],
        "admin_override": true
    });
    assert!(serde_json::from_value::<ReplicaPolicy>(value).is_err());
}
