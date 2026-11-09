//! Public verification only; current device revocations and policy chain are checked by callers.
use crate::{dm::verify_conversation_historical, public_key, verify_signature};
use kova_protocol::{
    dm::ConversationManifest,
    replication::{ReplicaApproval, ReplicaPolicy},
};
use sha2::{Digest, Sha256};

pub fn policy_hash(policy: &ReplicaPolicy) -> String {
    hex::encode(Sha256::digest(policy.canonical().as_bytes()))
}

pub fn valid_policy(policy: &ReplicaPolicy, manifest: &ConversationManifest) -> bool {
    policy.conversation_id == manifest.conversation_id
        && verify_conversation_historical(manifest)
        && policy.version > 0
        && policy.version <= i64::MAX as u64
        && if policy.version == 1 {
            policy.previous_hash.is_none()
                && policy.replicas.iter().any(|r| r.origin == manifest.origin)
        } else {
            policy
                .previous_hash
                .as_deref()
                .is_some_and(|hash| public_key(hash).is_some())
        }
        && (1..=8).contains(&policy.replicas.len())
        && policy
            .replicas
            .windows(2)
            .all(|pair| pair[0].origin < pair[1].origin)
        && policy.replicas.iter().all(|r| {
            !r.origin.is_empty()
                && r.origin.len() <= 2048
                && !r.origin.chars().any(char::is_control)
                && public_key(&r.public_key).is_some()
        })
        && policy.approvals.len() <= 2
}

pub fn verify_approval(
    policy: &ReplicaPolicy,
    approval: &ReplicaApproval,
    manifest: &ConversationManifest,
) -> bool {
    if !valid_policy(policy, manifest) {
        return false;
    }
    [&manifest.creator, &manifest.recipient]
        .iter()
        .any(|participant| {
            participant.global_identity_id == approval.identity
                && participant.device_id == approval.device
                && public_key(&participant.device_public_key).is_some_and(|key| {
                    verify_signature(&key, &policy.canonical(), &approval.signature)
                })
        })
}

pub fn verify_policy(policy: &ReplicaPolicy, manifest: &ConversationManifest) -> bool {
    valid_policy(policy, manifest)
        && policy.approvals.len() == 2
        && policy.approvals[0].identity != policy.approvals[1].identity
        && policy
            .approvals
            .iter()
            .all(|approval| verify_approval(policy, approval, manifest))
}
