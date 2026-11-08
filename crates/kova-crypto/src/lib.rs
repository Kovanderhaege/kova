//! Client-side MLS cryptography for one-to-one Kova conversations.
//!
//! Kova identity credentials bind an MLS signing key to a global identity and
//! device. The application authenticates that binding with its existing signed
//! device authorizations before trusting `Member` values.

use openmls::prelude::OpenMlsProvider;
use openmls::{
    group::{MlsGroup, StagedWelcome},
    prelude::{
        BasicCredential, Ciphersuite, Credential, CredentialWithKey, KeyPackage, KeyPackageIn,
        MlsGroupCreateConfig, MlsGroupJoinConfig, MlsMessageIn, ProcessedMessageContent,
        ProtocolVersion, Sender,
    },
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_libcrux_crypto::Provider;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;
use tls_codec::{Deserialize as TlsDeserialize, Serialize as TlsSerialize};
use zeroize::Zeroize;

const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;
const STATE_VERSION: u8 = 1;

pub type Result<T> = std::result::Result<T, CryptoError>;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid Kova identity or device credential")]
    InvalidCredential,
    #[error("invalid or unsupported MLS message")]
    InvalidMessage,
    #[error("conversation is unavailable")]
    UnknownConversation,
    #[error("MLS operation failed")]
    Mls,
    #[error("cryptographic state is invalid")]
    InvalidState,
    #[error("serialization failed")]
    Serialization,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Member {
    pub identity: String,
    pub device: String,
    /// The MLS signature key which Kova device authentication must bind to.
    pub signature_key: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decrypted {
    pub identity: String,
    pub device: String,
    pub body: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct PersistedState {
    version: u8,
    identity: String,
    device: String,
    signature_key: Vec<u8>,
    conversations: Vec<String>,
    /// Serialized OpenMLS provider key/value storage. This contains secret key
    /// material and must only be persisted inside authenticated encryption.
    storage: Vec<(Vec<u8>, Vec<u8>)>,
}

/// A client instance owns one MLS credential and any number of local groups.
pub struct CryptoClient {
    identity: String,
    device: String,
    provider: Provider,
    signer: SignatureKeyPair,
    signature_key: Vec<u8>,
    groups: BTreeMap<String, MlsGroup>,
}

impl CryptoClient {
    pub fn new(identity: &str, device: &str) -> Result<Self> {
        if identity.is_empty()
            || device.is_empty()
            || identity.len() > 256
            || device.len() > 256
            || identity.contains('\n')
            || device.contains('\n')
        {
            return Err(CryptoError::InvalidCredential);
        }

        let provider = Provider::new().map_err(|_| CryptoError::Mls)?;
        let signer = SignatureKeyPair::new(CIPHERSUITE.signature_algorithm())
            .map_err(|_| CryptoError::Mls)?;
        let signature_key = signer.public().to_vec();
        signer
            .store(provider.storage())
            .map_err(|_| CryptoError::Mls)?;
        Ok(Self {
            identity: identity.to_owned(),
            device: device.to_owned(),
            provider,
            signer,
            signature_key,
            groups: BTreeMap::new(),
        })
    }

    fn credential(&self) -> CredentialWithKey {
        let identity = format!("{}\n{}", self.identity, self.device).into_bytes();
        let credential = BasicCredential::new(identity);
        CredentialWithKey {
            credential: credential.into(),
            signature_key: self.signer.public().into(),
        }
    }

    pub fn key_package(&self) -> Result<Vec<u8>> {
        let package = KeyPackage::builder()
            .build(CIPHERSUITE, &self.provider, &self.signer, self.credential())
            .map_err(|_| CryptoError::Mls)?;
        package
            .key_package()
            .tls_serialize_detached()
            .map_err(|_| CryptoError::Serialization)
    }

    /// Decode and validate a peer's KeyPackage before Kova authenticates its
    /// credential and signature-key binding through the device authorization.
    pub fn package_member(package_bytes: &[u8]) -> Result<Member> {
        let provider = Provider::new().map_err(|_| CryptoError::Mls)?;
        let package_in = KeyPackageIn::tls_deserialize_exact(package_bytes)
            .map_err(|_| CryptoError::InvalidMessage)?;
        let package = package_in
            .validate(provider.crypto(), ProtocolVersion::Mls10)
            .map_err(|_| CryptoError::InvalidMessage)?;
        member_from_credential(
            package.leaf_node().credential(),
            package.leaf_node().signature_key().as_slice(),
        )
    }

    /// Create the first conversation group and add its second participant.
    /// Return the MLS Welcome that must be relayed to that participant.
    pub fn create_conversation(
        &mut self,
        conversation_id: &str,
        peer_package: &[u8],
    ) -> Result<Vec<u8>> {
        validate_conversation_id(conversation_id)?;
        if self.groups.contains_key(conversation_id) {
            return Err(CryptoError::InvalidState);
        }
        let package_in = KeyPackageIn::tls_deserialize_exact(peer_package)
            .map_err(|_| CryptoError::InvalidMessage)?;
        let package = package_in
            .validate(self.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(|_| CryptoError::InvalidMessage)?;
        let peer = member_from_credential(
            package.leaf_node().credential(),
            package.leaf_node().signature_key().as_slice(),
        )?;
        if peer.identity == self.identity && peer.device == self.device {
            return Err(CryptoError::InvalidCredential);
        }

        let mut group = MlsGroup::new_with_group_id(
            &self.provider,
            &self.signer,
            &MlsGroupCreateConfig::builder()
                .ciphersuite(CIPHERSUITE)
                .use_ratchet_tree_extension(true)
                .build(),
            openmls::prelude::GroupId::from_slice(conversation_id.as_bytes()),
            self.credential(),
        )
        .map_err(|_| CryptoError::Mls)?;
        let (_commit, welcome, _group_info) = group
            .add_members(&self.provider, &self.signer, std::slice::from_ref(&package))
            .map_err(|_| CryptoError::Mls)?;
        group
            .merge_pending_commit(&self.provider)
            .map_err(|_| CryptoError::Mls)?;
        let welcome = welcome
            .tls_serialize_detached()
            .map_err(|_| CryptoError::Serialization)?;
        self.groups.insert(conversation_id.to_owned(), group);
        Ok(welcome)
    }

    pub fn join_conversation(&mut self, conversation_id: &str, welcome_bytes: &[u8]) -> Result<()> {
        validate_conversation_id(conversation_id)?;
        if self.groups.contains_key(conversation_id) {
            return Err(CryptoError::InvalidState);
        }
        let message = MlsMessageIn::tls_deserialize_exact(welcome_bytes)
            .map_err(|_| CryptoError::InvalidMessage)?;
        let welcome = match message.extract() {
            openmls::framing::MlsMessageBodyIn::Welcome(welcome) => welcome,
            _ => return Err(CryptoError::InvalidMessage),
        };
        let staged = StagedWelcome::new_from_welcome(
            &self.provider,
            &MlsGroupJoinConfig::default(),
            welcome,
            None,
        )
        .map_err(|_| CryptoError::Mls)?;
        if staged.group_context().group_id().as_slice() != conversation_id.as_bytes() {
            return Err(CryptoError::InvalidMessage);
        }
        let group = staged
            .into_group(&self.provider)
            .map_err(|_| CryptoError::Mls)?;
        if group.members().count() != 2 || !self.owns_group_member(&group)? {
            return Err(CryptoError::InvalidCredential);
        }
        self.groups.insert(conversation_id.to_owned(), group);
        Ok(())
    }

    pub fn encrypt(&mut self, conversation_id: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
        let group = self
            .groups
            .get_mut(conversation_id)
            .ok_or(CryptoError::UnknownConversation)?;
        let message = group
            .create_message(&self.provider, &self.signer, plaintext)
            .map_err(|_| CryptoError::Mls)?;
        message
            .tls_serialize_detached()
            .map_err(|_| CryptoError::Serialization)
    }

    /// Decrypt an application message. Callers must discard this client after
    /// any error because OpenMLS may consume ratchet state while rejecting it.
    pub fn decrypt(&mut self, conversation_id: &str, ciphertext: &[u8]) -> Result<Decrypted> {
        if !self.groups.contains_key(conversation_id) {
            return Err(CryptoError::UnknownConversation);
        }
        let message = MlsMessageIn::tls_deserialize_exact(ciphertext)
            .map_err(|_| CryptoError::InvalidMessage)?
            .try_into_protocol_message()
            .map_err(|_| CryptoError::InvalidMessage)?;
        if message.group_id().as_slice() != conversation_id.as_bytes() {
            return Err(CryptoError::InvalidMessage);
        }
        // OpenMLS 0.8.1 runs a debug_assert on AEAD failure before returning
        // its documented error. Contain that debug-only panic at the boundary.
        let process_result = {
            let group = self
                .groups
                .get_mut(conversation_id)
                .ok_or(CryptoError::UnknownConversation)?;
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                group.process_message(&self.provider, message)
            }))
        };
        let processed = match process_result {
            Ok(Ok(processed)) => processed,
            Ok(Err(_)) => return Err(CryptoError::InvalidMessage),
            Err(_) => {
                self.groups.remove(conversation_id);
                return Err(CryptoError::InvalidMessage);
            }
        };
        let member = match processed.sender() {
            Sender::Member(index) => {
                let group = self
                    .groups
                    .get(conversation_id)
                    .ok_or(CryptoError::InvalidMessage)?;
                let sender = group.member_at(*index).ok_or(CryptoError::InvalidMessage)?;
                member_from_credential(&sender.credential, sender.signature_key.as_slice())?
            }
            _ => return Err(CryptoError::InvalidMessage),
        };
        let body = match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(content) => content.into_bytes(),
            _ => return Err(CryptoError::InvalidMessage),
        };
        Ok(Decrypted {
            identity: member.identity,
            device: member.device,
            body,
        })
    }

    pub fn members(&self, conversation_id: &str) -> Result<Vec<Member>> {
        let group = self
            .groups
            .get(conversation_id)
            .ok_or(CryptoError::UnknownConversation)?;
        group
            .members()
            .map(|member| {
                member_from_credential(&member.credential, member.signature_key.as_slice())
            })
            .collect()
    }

    fn owns_group_member(&self, group: &MlsGroup) -> Result<bool> {
        Ok(group.members().any(|member| {
            member_from_credential(&member.credential, member.signature_key.as_slice()).is_ok_and(
                |entry| {
                    entry.identity == self.identity
                        && entry.device == self.device
                        && entry.signature_key == self.signer.public()
                },
            )
        }))
    }

    /// Serialize all MLS provider data. The caller MUST encrypt and
    /// authenticate the returned bytes before writing them to persistent storage.
    pub fn export_state(&self) -> Result<Vec<u8>> {
        let storage = self.provider.storage();
        let values = storage
            .values
            .read()
            .map_err(|_| CryptoError::InvalidState)?;
        let state = PersistedState {
            version: STATE_VERSION,
            identity: self.identity.clone(),
            device: self.device.clone(),
            signature_key: self.signature_key.clone(),
            conversations: self.groups.keys().cloned().collect(),
            storage: values.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        };
        serde_json::to_vec(&state).map_err(|_| CryptoError::Serialization)
    }

    /// Restore a state blob previously produced by `export_state`.
    /// Decrypt and authenticate it in the caller before passing it here.
    pub fn restore_state(state_bytes: &[u8]) -> Result<Self> {
        let mut state: PersistedState =
            serde_json::from_slice(state_bytes).map_err(|_| CryptoError::InvalidState)?;
        if state.version != STATE_VERSION {
            return Err(CryptoError::InvalidState);
        }
        let provider = Provider::new().map_err(|_| CryptoError::InvalidState)?;
        {
            let mut values = provider
                .storage()
                .values
                .write()
                .map_err(|_| CryptoError::InvalidState)?;
            for (key, value) in state.storage.drain(..) {
                values.insert(key, value);
            }
        }
        let signer = SignatureKeyPair::read(
            provider.storage(),
            &state.signature_key,
            CIPHERSUITE.signature_algorithm(),
        )
        .ok_or(CryptoError::InvalidState)?;
        let mut client = Self {
            identity: state.identity.clone(),
            device: state.device.clone(),
            provider,
            signer,
            signature_key: state.signature_key.clone(),
            groups: BTreeMap::new(),
        };
        let mut conversation_ids = std::collections::BTreeSet::new();
        for id in state.conversations.drain(..) {
            validate_conversation_id(&id)?;
            if !conversation_ids.insert(id.clone()) {
                return Err(CryptoError::InvalidState);
            }
            let group_id = openmls::prelude::GroupId::from_slice(id.as_bytes());
            let group = MlsGroup::load(client.provider.storage(), &group_id)
                .map_err(|_| CryptoError::InvalidState)?
                .ok_or(CryptoError::InvalidState)?;
            if group.group_id().as_slice() != id.as_bytes() || !client.owns_group_member(&group)? {
                return Err(CryptoError::InvalidState);
            }
            client.groups.insert(id, group);
        }
        Ok(client)
    }
}

fn validate_conversation_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 128 || id.contains('\n') {
        return Err(CryptoError::InvalidCredential);
    }
    Ok(())
}

fn member_from_credential(credential: &Credential, signature_key: &[u8]) -> Result<Member> {
    let basic = BasicCredential::try_from(credential.clone())
        .map_err(|_| CryptoError::InvalidCredential)?;
    let id = std::str::from_utf8(basic.identity()).map_err(|_| CryptoError::InvalidCredential)?;
    let (identity, device) = id.split_once('\n').ok_or(CryptoError::InvalidCredential)?;
    if identity.is_empty() || device.is_empty() || device.contains('\n') {
        return Err(CryptoError::InvalidCredential);
    }
    Ok(Member {
        identity: identity.to_owned(),
        device: device.to_owned(),
        signature_key: signature_key.to_vec(),
    })
}

impl Drop for PersistedState {
    fn drop(&mut self) {
        self.identity.zeroize();
        self.device.zeroize();
        self.signature_key.zeroize();
        for id in &mut self.conversations {
            id.zeroize();
        }
        for (_, value) in &mut self.storage {
            value.zeroize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_clients_exchange_messages_across_restart_and_reject_replay() {
        let mut alice = CryptoClient::new("kova1-alice", "device-alice").unwrap();
        let mut bob = CryptoClient::new("kova1-bob", "device-bob").unwrap();
        let bob_package = bob.key_package().unwrap();

        assert_eq!(
            CryptoClient::package_member(&bob_package).unwrap(),
            Member {
                identity: "kova1-bob".into(),
                device: "device-bob".into(),
                signature_key: bob.signature_key.clone(),
            }
        );

        let welcome = alice
            .create_conversation("conversation-1", &bob_package)
            .unwrap();
        bob.join_conversation("conversation-1", &welcome).unwrap();
        assert_eq!(alice.members("conversation-1").unwrap().len(), 2);
        assert_eq!(bob.members("conversation-1").unwrap().len(), 2);

        let first = alice.encrypt("conversation-1", b"bonjour").unwrap();
        let bob_state = bob.export_state().unwrap();
        let mut bob = CryptoClient::restore_state(&bob_state).unwrap();
        let received = bob.decrypt("conversation-1", &first).unwrap();
        assert_eq!(received.identity, "kova1-alice");
        assert_eq!(received.device, "device-alice");
        assert_eq!(received.body, b"bonjour");
        assert!(bob.decrypt("conversation-1", &first).is_err());

        let second = alice
            .encrypt("conversation-1", "après reprise".as_bytes())
            .unwrap();
        assert_eq!(
            bob.decrypt("conversation-1", &second).unwrap().body,
            "après reprise".as_bytes()
        );
    }

    #[test]
    fn tampering_and_unjoined_clients_are_rejected() {
        let mut alice = CryptoClient::new("kova1-alice", "device-a").unwrap();
        let mut bob = CryptoClient::new("kova1-bob", "device-b").unwrap();
        let welcome = alice
            .create_conversation("conversation-2", &bob.key_package().unwrap())
            .unwrap();
        bob.join_conversation("conversation-2", &welcome).unwrap();
        let mut ciphertext = alice.encrypt("conversation-2", b"secret").unwrap();
        let last_byte = ciphertext.last_mut().unwrap();
        *last_byte ^= 1;
        assert!(bob.decrypt("conversation-2", &ciphertext).is_err());

        let mut charlie = CryptoClient::new("kova1-charlie", "device-c").unwrap();
        assert!(charlie.decrypt("conversation-2", &ciphertext).is_err());
    }
}

impl Drop for CryptoClient {
    fn drop(&mut self) {
        self.identity.zeroize();
        self.device.zeroize();
        self.signature_key.zeroize();
        if let Ok(mut values) = self.provider.storage().values.write() {
            for (_, value) in values.iter_mut() {
                value.zeroize();
            }
            values.clear();
        }
    }
}
