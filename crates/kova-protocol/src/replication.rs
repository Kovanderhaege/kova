//! Bilateral authorization of replicas. Approvals never change signed policy bytes.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicaDescriptor {
    pub origin: String,
    pub public_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicaApproval {
    pub identity: String,
    pub device: String,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicaPolicy {
    pub conversation_id: String,
    pub version: u64,
    pub previous_hash: Option<String>,
    pub replicas: Vec<ReplicaDescriptor>,
    pub approvals: Vec<ReplicaApproval>,
}

impl ReplicaPolicy {
    pub fn canonical(&self) -> String {
        serde_json::to_string(&(
            "kova-dm-replica-policy-v1",
            &self.conversation_id,
            self.version,
            &self.previous_hash,
            &self.replicas,
        ))
        .expect("serializing a fixed tuple cannot fail")
    }
}
