//! Versioned, device-signed public envelopes for direct messages.
//! The payload remains opaque to servers; MLS owns its cryptographic format.
use serde::{Deserialize, Serialize};

/// Public control state, separate from the immutable MLS conversation manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DmAuthorizationState {
    pub authorizations: Vec<DeviceBundle>,
    pub revocations: Vec<DmRevocation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DmRevocation {
    pub global_identity_id: String,
    pub device_public_key: String,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceBundle {
    pub global_identity_id: String,
    pub device_id: String,
    pub root_public_key: String,
    pub device_public_key: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub authorization_signature: String,
    /// Opaque MLS KeyPackage, lowercase hexadecimal.
    pub key_package: String,
    pub signature: String,
}

impl DeviceBundle {
    pub fn canonical(&self) -> String {
        serde_json::to_string(&(
            "kova-dm-device-v1",
            &self.global_identity_id,
            &self.device_id,
            &self.root_public_key,
            &self.device_public_key,
            self.issued_at,
            self.expires_at,
            &self.authorization_signature,
            &self.key_package,
        ))
        .expect("serializing a fixed tuple cannot fail")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationManifest {
    pub conversation_id: String,
    pub origin: String,
    pub creator: DeviceBundle,
    pub recipient: DeviceBundle,
    /// Opaque MLS Welcome, lowercase hexadecimal.
    pub welcome: String,
    pub signature: String,
}

impl ConversationManifest {
    pub fn canonical(&self) -> String {
        serde_json::to_string(&(
            "kova-dm-conversation-v1",
            &self.conversation_id,
            &self.origin,
            &self.creator,
            &self.recipient,
            &self.welcome,
        ))
        .expect("serializing a fixed tuple cannot fail")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DmEvent {
    pub conversation_id: String,
    pub event_id: String,
    pub author_identity: String,
    pub author_device: String,
    pub parents: Vec<String>,
    /// Opaque MLS message, lowercase hexadecimal.
    pub ciphertext: String,
    pub signature: String,
}

impl DmEvent {
    pub fn canonical(&self) -> String {
        serde_json::to_string(&(
            "kova-dm-event-v1",
            &self.conversation_id,
            &self.event_id,
            &self.author_identity,
            &self.author_device,
            &self.parents,
            &self.ciphertext,
        ))
        .expect("serializing a fixed tuple cannot fail")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDmEvent {
    pub cursor: i64,
    pub event: DmEvent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DmEventPage {
    pub items: Vec<StoredDmEvent>,
    pub next_cursor: i64,
}

pub fn valid_dm_event(event: &DmEvent) -> bool {
    valid_uuid(&event.conversation_id)
        && valid_uuid(&event.event_id)
        && valid_identity(&event.author_identity)
        && valid_fixed_hex(&event.author_device, 32)
        && valid_bounded_hex(&event.ciphertext, 1..=16 * 1024)
        && event.parents.len() <= 16
        && event.parents.iter().all(|parent| valid_uuid(parent))
        && event.parents.windows(2).all(|pair| pair[0] < pair[1])
}

pub fn valid_device_bundle(bundle: &DeviceBundle) -> bool {
    valid_identity(&bundle.global_identity_id)
        && valid_fixed_hex(&bundle.root_public_key, 32)
        && valid_fixed_hex(&bundle.device_public_key, 32)
        && valid_fixed_hex(&bundle.device_id, 32)
        && valid_fixed_hex(&bundle.authorization_signature, 64)
        && valid_fixed_hex(&bundle.signature, 64)
        && valid_bounded_hex(&bundle.key_package, 1..=32 * 1024)
        && bundle.issued_at > 0
        && bundle.expires_at > bundle.issued_at
}

pub fn valid_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
            }
        })
}

pub fn valid_identity(value: &str) -> bool {
    value
        .strip_prefix("kova1-")
        .is_some_and(|key| valid_fixed_hex(key, 32))
}

fn valid_fixed_hex(value: &str, size: usize) -> bool {
    valid_bounded_hex(value, size..=size)
}

fn valid_bounded_hex(value: &str, range: std::ops::RangeInclusive<usize>) -> bool {
    value.len() % 2 == 0
        && range.contains(&(value.len() / 2))
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
