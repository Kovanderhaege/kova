//! Device-bound private conversations. Network requests contain only signed public
//! credentials and MLS ciphertext. All durable client state is an age-encrypted blob.
mod network;
mod storage;
#[cfg(test)]
mod tests;

use crate::{Identity, Instance, Result, Session};
use kova_crypto::CryptoClient;
use kova_protocol::dm::{ConversationManifest, DeviceBundle, DmEvent};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use storage::DmStore;

#[derive(Clone, Serialize, Deserialize)]
struct Conversation {
    manifest: ConversationManifest,
    cursor: i64,
    #[serde(default)]
    source_cursors: BTreeMap<String, i64>,
    #[serde(default)]
    active_policy: Option<kova_protocol::replication::ReplicaPolicy>,
    pending_creation: bool,
    events: BTreeMap<String, DmEvent>,
    messages: BTreeMap<String, String>,
    pending: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct State {
    version: u8,
    identity: String,
    device: String,
    crypto: Vec<u8>,
    bundle: Option<DeviceBundle>,
    conversations: BTreeMap<String, Conversation>,
    #[serde(default)]
    replica_proposals: BTreeMap<String, kova_protocol::replication::ReplicaPolicy>,
}

impl Drop for State {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.crypto.zeroize();
    }
}

impl Drop for Conversation {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        for body in self.messages.values_mut() {
            body.zeroize();
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Plaintext {
    conversation_id: String,
    event_id: String,
    parents: Vec<String>,
    body: String,
}

fn signed_bundle(identity: &Identity, package: Vec<u8>) -> Result<DeviceBundle> {
    let certificate = identity.certificate()?;
    let mut bundle = DeviceBundle {
        global_identity_id: identity.global_id()?,
        device_id: identity.device_id()?,
        root_public_key: identity.root_public_key()?,
        device_public_key: certificate["device_public_key"]
            .as_str()
            .ok_or("Clé absente")?
            .into(),
        issued_at: certificate["device_authorization"]["issued_at"]
            .as_i64()
            .ok_or("Date absente")?,
        expires_at: certificate["device_authorization"]["expires_at"]
            .as_i64()
            .ok_or("Date absente")?,
        authorization_signature: certificate["device_authorization"]["signature"]
            .as_str()
            .ok_or("Signature absente")?
            .into(),
        key_package: hex::encode(package),
        signature: String::new(),
    };
    bundle.signature = sign(identity, &bundle.canonical())?;
    Ok(bundle)
}

fn sign(identity: &Identity, canonical: &str) -> Result<String> {
    Ok(hex::encode(
        identity.device_key()?.sign(canonical.as_bytes()).as_ref(),
    ))
}

fn verify_package(bundle: &DeviceBundle) -> Result<()> {
    let member = CryptoClient::package_member(&hex::decode(&bundle.key_package)?)?;
    if member.identity != bundle.global_identity_id || member.device != bundle.device_id {
        return Err("Le KeyPackage ne correspond pas à l'appareil signé.".into());
    }
    Ok(())
}

fn verify_members(crypto: &CryptoClient, manifest: &ConversationManifest) -> Result<()> {
    let members = crypto.members(&manifest.conversation_id)?;
    let expected = [&manifest.creator, &manifest.recipient]
        .into_iter()
        .map(|bundle| -> Result<_> {
            Ok(CryptoClient::package_member(&hex::decode(
                &bundle.key_package,
            )?)?)
        })
        .collect::<Result<Vec<_>>>()?;
    if members.len() != 2 || expected.iter().any(|member| !members.contains(member)) {
        return Err("Participants ou clés MLS inattendus : conversation refusée.".into());
    }
    Ok(())
}

fn heads(conversation: &Conversation) -> Vec<String> {
    let parents: std::collections::BTreeSet<_> = conversation
        .events
        .values()
        .flat_map(|event| event.parents.iter())
        .collect();
    conversation
        .events
        .keys()
        .filter(|id| !parents.contains(id))
        .cloned()
        .collect()
}

impl DmStore {
    fn check_identity(&self, identity: &Identity) -> Result<()> {
        if identity.global_id()? != self.state.identity
            || identity.device_id()? != self.state.device
        {
            return Err("Ce coffre MP appartient à un autre appareil.".into());
        }
        Ok(())
    }

    pub fn render(&self, id: Option<&str>) -> Result<String> {
        let Some(id) = id.filter(|id| !id.is_empty()) else {
            let mut text = String::from("Conversations locales — identifiants globaux\n");
            for (id, conversation) in &self.state.conversations {
                text.push_str(&format!(
                    "\n{id}\n{} ↔ {}\n{} · {} envoi(s) en attente\n{}",
                    conversation.manifest.creator.global_identity_id,
                    conversation.manifest.recipient.global_identity_id,
                    conversation.manifest.origin,
                    conversation.pending.len(),
                    if let Some(policy) = &conversation.active_policy {
                        policy
                            .replicas
                            .iter()
                            .map(|replica| {
                                format!(
                                    "{} · curseur {}",
                                    replica.origin,
                                    conversation
                                        .source_cursors
                                        .get(&replica.origin)
                                        .copied()
                                        .unwrap_or(0)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        format!(
                            "{} · curseur {}",
                            conversation.manifest.origin,
                            conversation
                                .source_cursors
                                .get(&conversation.manifest.origin)
                                .copied()
                                .unwrap_or(0)
                        )
                    }
                ));
            }
            return Ok(text);
        };
        let conversation = self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?;
        let mut text = format!(
            "MP {id}\n{}\nDeux appareils vérifiés. Historique local chiffré.\n",
            conversation.manifest.origin
        );
        if conversation.pending_creation {
            text.push_str("Création en attente : Réessayer.\n");
        }
        // Topological order with UUID tie-breaking, independent of server clocks.
        let mut remaining = conversation.events.clone();
        let mut seen = std::collections::BTreeSet::new();
        while !remaining.is_empty() {
            let next = remaining
                .iter()
                .find(|(_, event)| event.parents.iter().all(|p| seen.contains(p)))
                .map(|(id, _)| id.clone());
            let Some(next) = next else {
                return Err("Dépendances MP manquantes".into());
            };
            let event = remaining.remove(&next).ok_or("Événement absent")?;
            let status = if conversation.pending.contains(&next) {
                "en attente"
            } else {
                "reçu par le serveur"
            };
            text.push_str(&format!(
                "\n{} · {status}\n{}\n",
                event.author_identity,
                conversation.messages.get(&next).ok_or("Message absent")?
            ));
            seen.insert(next);
        }
        Ok(text)
    }

    /// Prepares ciphertext and ratchet state durably before any attempt to send.
    pub fn prepare(&mut self, identity: &Identity, id: &str, body: &str) -> Result<String> {
        self.check_identity(identity)?;
        if !kova_protocol::valid_message_body(body) {
            return Err("Message vide ou trop long".into());
        }
        let mut state = self.state.clone();
        let conversation = state
            .conversations
            .get_mut(id)
            .ok_or("Conversation inconnue")?;
        if !conversation.pending.is_empty() {
            return Err("Réessayer l'envoi en attente avant un nouvel envoi.".into());
        }
        if !kova_identity::dm::verify_conversation(
            &conversation.manifest,
            chrono::Utc::now().timestamp(),
        ) {
            return Err("Autorisation d'appareil expirée : ouvrir une nouvelle conversation avec des preuves renouvelées.".into());
        }
        let parents = heads(conversation);
        if parents.len() > 16 {
            return Err("Trop de branches causales : synchroniser.".into());
        }
        let event_id = uuid::Uuid::new_v4().to_string();
        let plain = Plaintext {
            conversation_id: id.into(),
            event_id: event_id.clone(),
            parents: parents.clone(),
            body: body.into(),
        };
        let mut crypto = CryptoClient::restore_state(&state.crypto)?;
        let bytes = zeroize::Zeroizing::new(serde_json::to_vec(&plain)?);
        let mut event = DmEvent {
            conversation_id: id.into(),
            event_id: event_id.clone(),
            author_identity: state.identity.clone(),
            author_device: state.device.clone(),
            parents,
            ciphertext: hex::encode(crypto.encrypt(id, &bytes)?),
            signature: String::new(),
        };
        event.signature = sign(identity, &event.canonical())?;
        if !kova_protocol::dm::valid_dm_event(&event) {
            return Err("Message chiffré trop volumineux : raccourcir le texte.".into());
        }
        conversation.events.insert(event_id.clone(), event);
        conversation.messages.insert(event_id.clone(), body.into());
        conversation.pending.push(event_id.clone());
        state.crypto = crypto.export_state()?;
        self.save(state)?;
        Ok(event_id)
    }

    pub fn send(
        &mut self,
        instance: &Instance,
        session: &Session,
        identity: &Identity,
        id: &str,
        body: &str,
    ) -> Result<()> {
        self.prepare(identity, id, body)?;
        self.retry(instance, session, id)
    }
}
