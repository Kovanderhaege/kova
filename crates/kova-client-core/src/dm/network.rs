use super::*;
use kova_protocol::dm::DmEventPage;
use kova_protocol::replication::{ReplicaApproval, ReplicaDescriptor, ReplicaPolicy};
use reqwest::Method;

impl Instance {
    pub fn dm_conversations(&self, session: &Session) -> Result<Vec<ConversationManifest>> {
        Ok(serde_json::from_value(self.community_request(
            session,
            Method::GET,
            "/dm/conversations",
            None,
        )?)?)
    }

    pub fn dm_conversation(&self, session: &Session, id: &str) -> Result<ConversationManifest> {
        Ok(serde_json::from_value(self.community_request(
            session,
            Method::GET,
            &format!("/dm/conversations/{id}"),
            None,
        )?)?)
    }

    fn replica_request<T: serde::de::DeserializeOwned>(
        &self,
        session: &Session,
        method: Method,
        id: &str,
        suffix: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        Ok(serde_json::from_value(self.community_request(
            session,
            method,
            &format!("/dm/conversations/{id}/replicas{suffix}"),
            body,
        )?)?)
    }

    pub fn dm_replica_policy(&self, session: &Session, id: &str) -> Result<ReplicaPolicy> {
        self.replica_request(session, Method::GET, id, "", None)
    }

    pub fn dm_replica_proposal(&self, session: &Session, id: &str) -> Result<ReplicaPolicy> {
        self.replica_request(session, Method::GET, id, "/proposal", None)
    }

    pub fn dm_replica_history(&self, session: &Session, id: &str) -> Result<Vec<ReplicaPolicy>> {
        self.replica_request(session, Method::GET, id, "/history", None)
    }

    pub fn dm_approve_replica_policy(
        &self,
        session: &Session,
        id: &str,
        policy: &ReplicaPolicy,
        approval: &ReplicaApproval,
    ) -> Result<ReplicaPolicy> {
        self.replica_request(
            session,
            Method::POST,
            id,
            "/approve",
            Some(serde_json::json!({ "policy": policy, "approval": approval })),
        )
    }

    pub fn dm_sync_replica(&self, session: &Session, id: &str, origin: &str) -> Result<(u64, u64)> {
        let result: serde_json::Value = self.replica_request(
            session,
            Method::POST,
            id,
            "/sync",
            Some(serde_json::json!({ "origin": origin })),
        )?;
        Ok((
            result["received"]
                .as_u64()
                .ok_or("Sync MP sans compteur received")?,
            result["sent"]
                .as_u64()
                .ok_or("Sync MP sans compteur sent")?,
        ))
    }

    pub(super) fn replica_descriptor(origin: &str) -> Result<ReplicaDescriptor> {
        crate::transport::validate_origin(origin)?;
        let response: serde_json::Value = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(10))
            .build()?
            .get(format!("{origin}/api/v1/federation/v1/identity"))
            .send()?
            .error_for_status()?
            .json()?;
        let descriptor = ReplicaDescriptor {
            origin: response["origin"]
                .as_str()
                .ok_or("Origine du réplica absente")?
                .into(),
            public_key: response["public_key"]
                .as_str()
                .ok_or("Clé du réplica absente")?
                .into(),
        };
        if descriptor.origin != origin
            || kova_identity::public_key(&descriptor.public_key).is_none()
        {
            return Err("Identité de réplica invalide ou différente de l'origine demandée".into());
        }
        Ok(descriptor)
    }
}

impl DmStore {
    pub fn propose_replica(
        &mut self,
        instance: &Instance,
        session: &Session,
        identity: &Identity,
        id: &str,
        peer_origin: &str,
    ) -> Result<ReplicaPolicy> {
        self.check_identity(identity)?;
        self.check_session(instance, session)?;
        self.check_replica_origin(instance, session, id)?;
        if peer_origin == instance.origin {
            return Err("Le réplica doit être une autre instance".into());
        }
        let previous = match instance.dm_replica_policy(session, id) {
            Ok(current) => {
                if !kova_identity::replication::verify_policy(
                    &current,
                    &self.state.conversations[id].manifest,
                ) {
                    return Err("Politique active invalide".into());
                }
                let history = instance.dm_replica_history(session, id)?;
                if self.remember_policy_history(id, &history)? != current {
                    return Err("Politique active différente de la fin de sa chaîne".into());
                }
                Some(current)
            }
            Err(error) if error.to_string().starts_with("404 ") => None,
            Err(error) => return Err(error),
        };
        let mut replicas = match &previous {
            Some(current) => current.replicas.clone(),
            None => vec![Instance::replica_descriptor(&instance.origin)?],
        };
        let peer = Instance::replica_descriptor(peer_origin)?;
        if replicas.iter().any(|replica| replica.origin == peer.origin) {
            return Err("Cette instance figure déjà dans la politique".into());
        }
        replicas.push(peer);
        replicas.sort_by(|left, right| left.origin.cmp(&right.origin));
        if replicas.len() > 8 {
            return Err("Une politique accepte au plus huit réplicas".into());
        }
        let policy = ReplicaPolicy {
            conversation_id: id.into(),
            version: previous.as_ref().map_or(1, |value| value.version + 1),
            previous_hash: previous
                .as_ref()
                .map(kova_identity::replication::policy_hash),
            replicas,
            approvals: Vec::new(),
        };
        if !kova_identity::replication::valid_policy(
            &policy,
            &self.state.conversations[id].manifest,
        ) {
            return Err("La politique candidate ne correspond pas au manifeste".into());
        }
        let mut state = self.state.clone();
        state.replica_proposals.insert(id.into(), policy.clone());
        self.save(state)?;
        Ok(policy)
    }

    pub fn propose_replica_removal(
        &mut self,
        instance: &Instance,
        session: &Session,
        identity: &Identity,
        id: &str,
        peer_origin: &str,
    ) -> Result<ReplicaPolicy> {
        self.check_identity(identity)?;
        self.check_session(instance, session)?;
        self.check_replica_origin(instance, session, id)?;
        if peer_origin == instance.origin {
            return Err(
                "Sélectionnez un autre réplica conservé pour retirer cette instance.".into(),
            );
        }
        let previous = instance.dm_replica_policy(session, id)?;
        let manifest = self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?
            .manifest
            .clone();
        if !kova_identity::replication::verify_policy(&previous, &manifest) {
            return Err("Politique active invalide".into());
        }
        let history = instance.dm_replica_history(session, id)?;
        if self.remember_policy_history(id, &history)? != previous {
            return Err("Politique active différente de la fin de sa chaîne".into());
        }
        let replicas: Vec<_> = previous
            .replicas
            .iter()
            .filter(|replica| replica.origin != peer_origin)
            .cloned()
            .collect();
        if replicas.len() == previous.replicas.len() || replicas.is_empty() {
            return Err("Réplica inconnu ou suppression du dernier réplica interdite".into());
        }
        let policy = ReplicaPolicy {
            conversation_id: id.into(),
            version: previous.version + 1,
            previous_hash: Some(kova_identity::replication::policy_hash(&previous)),
            replicas,
            approvals: Vec::new(),
        };
        if !kova_identity::replication::valid_policy(&policy, &manifest) {
            return Err("La politique de retrait est invalide".into());
        }
        let mut state = self.state.clone();
        state.replica_proposals.insert(id.into(), policy.clone());
        self.save(state)?;
        Ok(policy)
    }

    pub fn approve_replica_proposal(
        &mut self,
        instance: &Instance,
        session: &Session,
        identity: &Identity,
        id: &str,
    ) -> Result<ReplicaPolicy> {
        self.check_identity(identity)?;
        self.check_session(instance, session)?;
        self.check_replica_origin(instance, session, id)?;
        let policy = self
            .state
            .replica_proposals
            .get(id)
            .ok_or("Consultez et enregistrez d'abord la proposition avant de l'approuver")?
            .clone();
        let manifest = &self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?
            .manifest;
        let identity_id = identity.global_id()?;
        if !self.valid_next_policy(id, &policy, manifest)?
            || policy
                .approvals
                .iter()
                .any(|approval| approval.identity == identity_id)
        {
            return Err("Proposition invalide ou déjà approuvée par cette identité".into());
        }
        let approval = ReplicaApproval {
            identity: identity.global_id()?,
            device: identity.device_id()?,
            signature: sign(identity, &policy.canonical())?,
        };
        let mut submitted = policy.clone();
        submitted.approvals.clear();
        let updated = instance.dm_approve_replica_policy(session, id, &submitted, &approval)?;
        if updated.canonical() != policy.canonical()
            || updated
                .approvals
                .iter()
                .any(|item| !kova_identity::replication::verify_approval(&updated, item, manifest))
            || !updated.approvals.iter().any(|item| {
                item.identity == approval.identity
                    && item.device == approval.device
                    && item.signature == approval.signature
            })
        {
            return Err(
                "Le serveur a renvoyé une politique ou des approbations contradictoires".into(),
            );
        }
        if updated.approvals.len() == 2
            && !kova_identity::replication::verify_policy(&updated, manifest)
        {
            return Err(
                "La seconde approbation n'a pas produit une politique bilatérale valide".into(),
            );
        }
        if updated.approvals.len() == 2 {
            let mut state = self.state.clone();
            state.replica_proposals.remove(id);
            state
                .conversations
                .get_mut(id)
                .ok_or("Conversation inconnue")?
                .active_policy = Some(updated.clone());
            self.save(state)?;
        } else {
            let mut state = self.state.clone();
            state.replica_proposals.insert(id.into(), updated.clone());
            self.save(state)?;
        }
        Ok(updated)
    }

    pub fn replica_proposal(
        &mut self,
        instance: &Instance,
        session: &Session,
        id: &str,
    ) -> Result<ReplicaPolicy> {
        self.check_session(instance, session)?;
        self.check_replica_origin(instance, session, id)?;
        let policy = if let Some(policy) = self.state.replica_proposals.get(id) {
            policy.clone()
        } else {
            instance.dm_replica_proposal(session, id)?
        };
        let manifest = &self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?
            .manifest;
        if !self.valid_next_policy(id, &policy, manifest)? {
            return Err("Proposition ou approbation invalide".into());
        }
        let mut state = self.state.clone();
        state.replica_proposals.insert(id.into(), policy.clone());
        self.save(state)?;
        Ok(policy)
    }

    pub fn sync_replicas(
        &mut self,
        instance: &Instance,
        session: &Session,
        id: &str,
        peer_origin: &str,
    ) -> Result<(u64, u64)> {
        self.check_session(instance, session)?;
        self.check_replica_origin(instance, session, id)?;
        let history = instance.dm_replica_history(session, id)?;
        let policy = self.remember_policy_history(id, &history)?;
        let active_peer = policy
            .replicas
            .iter()
            .any(|replica| replica.origin == peer_origin);
        let removed_peer = !active_peer
            && history
                .iter()
                .take(history.len().saturating_sub(1))
                .any(|previous| {
                    previous
                        .replicas
                        .iter()
                        .any(|replica| replica.origin == peer_origin)
                });
        if !active_peer && !removed_peer {
            return Err("Réplica absent d'une politique bilatéralement approuvée".into());
        }
        let (mut received, mut sent) = (0u64, 0u64);
        for _ in 0..64 {
            let (batch_received, batch_sent) =
                instance.dm_sync_replica(session, id, peer_origin)?;
            received = received.saturating_add(batch_received);
            sent = sent.saturating_add(batch_sent);
            if batch_received == 0 && batch_sent == 0 {
                self.sync(instance, session, id)?;
                return Ok((received, sent));
            }
        }
        Err("Synchronisation MP incomplète après 64 lots ; relancez-la pour reprendre.".into())
    }

    pub(super) fn check_session(&self, instance: &Instance, session: &Session) -> Result<()> {
        instance.authorize(session)?;
        if session.profile["global_identity_id"] != self.state.identity {
            return Err("Session d'une autre identité".into());
        }
        Ok(())
    }

    pub(super) fn check_replica_origin(
        &mut self,
        instance: &Instance,
        session: &Session,
        id: &str,
    ) -> Result<()> {
        let manifest_origin = self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?
            .manifest
            .origin
            .clone();
        let history = match instance.dm_replica_history(session, id) {
            Ok(history) => history,
            Err(error)
                if error.to_string().starts_with("404 ")
                    && manifest_origin == instance.origin
                    && self
                        .state
                        .conversations
                        .get(id)
                        .and_then(|conversation| conversation.active_policy.as_ref())
                        .is_none() =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let policy = self.remember_policy_history(id, &history)?;
        if !policy
            .replicas
            .iter()
            .any(|replica| replica.origin == instance.origin)
        {
            return Err("Cette instance n'est pas autorisée par la politique active".into());
        }
        Ok(())
    }

    fn remember_policy_history(
        &mut self,
        id: &str,
        policies: &[ReplicaPolicy],
    ) -> Result<ReplicaPolicy> {
        if policies.is_empty() {
            return Err("Historique de politique vide".into());
        }
        let conversation = self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?;
        if let Some(cached) = &conversation.active_policy {
            let hash = kova_identity::replication::policy_hash(cached);
            if !policies.iter().any(|candidate| {
                candidate.version == cached.version
                    && kova_identity::replication::policy_hash(candidate) == hash
            }) {
                return Err("Historique de politique divergent du dernier état conservé".into());
            }
        }
        let mut previous: Option<&ReplicaPolicy> = None;
        for (index, policy) in policies.iter().enumerate() {
            let valid_parent = match previous {
                None => policy.previous_hash.is_none(),
                Some(prior) => {
                    policy.previous_hash.as_deref()
                        == Some(kova_identity::replication::policy_hash(prior).as_str())
                }
            };
            if policy.version != index as u64 + 1
                || !valid_parent
                || !kova_identity::replication::verify_policy(policy, &conversation.manifest)
            {
                return Err("Chaîne de politiques incomplète, invalide ou divergente".into());
            }
            previous = Some(policy);
        }
        let policy = previous.ok_or("Historique de politique vide")?.clone();
        let mut state = self.state.clone();
        state
            .conversations
            .get_mut(id)
            .ok_or("Conversation inconnue")?
            .active_policy = Some(policy.clone());
        self.save(state)?;
        Ok(policy)
    }

    fn valid_next_policy(
        &self,
        id: &str,
        policy: &ReplicaPolicy,
        manifest: &ConversationManifest,
    ) -> Result<bool> {
        if !kova_identity::replication::valid_policy(policy, manifest)
            || policy.approvals.iter().any(|approval| {
                !kova_identity::replication::verify_approval(policy, approval, manifest)
            })
        {
            return Ok(false);
        }
        match self
            .state
            .conversations
            .get(id)
            .and_then(|conversation| conversation.active_policy.as_ref())
        {
            Some(previous) => Ok(policy.version == previous.version + 1
                && policy.previous_hash.as_deref()
                    == Some(kova_identity::replication::policy_hash(previous).as_str())),
            None => Ok(policy.version == 1 && policy.previous_hash.is_none()),
        }
    }

    pub fn publish(
        &mut self,
        instance: &Instance,
        session: &Session,
        identity: &Identity,
    ) -> Result<()> {
        self.check_identity(identity)?;
        self.check_session(instance, session)?;
        let mut state = self.state.clone();
        if state
            .bundle
            .as_ref()
            .is_none_or(|b| !kova_identity::dm::verify_device(b, chrono::Utc::now().timestamp()))
        {
            let crypto = CryptoClient::restore_state(&state.crypto)?;
            state.bundle = Some(signed_bundle(identity, crypto.key_package()?)?);
            state.crypto = crypto.export_state()?;
            self.save(state)?;
        }
        instance.community_request(
            session,
            Method::POST,
            "/dm/key-packages",
            Some(serde_json::to_value(&self.state.bundle)?),
        )?;
        Ok(())
    }

    pub fn create(
        &mut self,
        instance: &Instance,
        session: &Session,
        identity: &Identity,
        peer_identity: &str,
        peer_device: &str,
    ) -> Result<String> {
        self.check_identity(identity)?;
        self.check_session(instance, session)?;
        if !kova_protocol::dm::valid_identity(peer_identity)
            || kova_identity::public_key(peer_device).is_none()
            || peer_identity == self.state.identity
        {
            return Err("Identité et empreinte d'appareil du correspondant requises.".into());
        }
        let peer: DeviceBundle = serde_json::from_value(instance.community_request(
            session,
            Method::GET,
            &format!("/dm/key-packages/{peer_identity}/{peer_device}"),
            None,
        )?)?;
        if peer.global_identity_id != peer_identity
            || peer.device_id != peer_device
            || !kova_identity::dm::verify_device(&peer, chrono::Utc::now().timestamp())
        {
            return Err("Appareil distant non vérifié ou autorisation expirée".into());
        }
        verify_package(&peer)?;
        let mut state = self.state.clone();
        let mut crypto = CryptoClient::restore_state(&state.crypto)?;
        let own = signed_bundle(identity, crypto.key_package()?)?;
        let id = uuid::Uuid::new_v4().to_string();
        let welcome = crypto.create_conversation(&id, &hex::decode(&peer.key_package)?)?;
        let mut manifest = ConversationManifest {
            conversation_id: id.clone(),
            origin: instance.origin.clone(),
            creator: own,
            recipient: peer,
            welcome: hex::encode(welcome),
            signature: String::new(),
        };
        manifest.signature = sign(identity, &manifest.canonical())?;
        verify_members(&crypto, &manifest)?;
        state.conversations.insert(
            id.clone(),
            Conversation {
                manifest,
                cursor: 0,
                source_cursors: BTreeMap::new(),
                active_policy: None,
                pending_creation: true,
                events: BTreeMap::new(),
                messages: BTreeMap::new(),
                pending: Vec::new(),
            },
        );
        state.crypto = crypto.export_state()?;
        self.save(state)?;
        // A lost response leaves the exact manifest ready to retry, never a new group.
        self.retry(instance, session, &id)
            .map_err(|e| format!("Conversation {id} conservée localement. Réessayer : {e}"))?;
        Ok(id)
    }

    pub fn accept(
        &mut self,
        instance: &Instance,
        session: &Session,
        identity: &Identity,
        id: &str,
        expected_peer_identity: &str,
        expected_peer_device: &str,
    ) -> Result<()> {
        self.check_identity(identity)?;
        self.check_session(instance, session)?;
        if !kova_protocol::valid_uuid(id) {
            return Err("Identifiant de conversation invalide".into());
        }
        let manifest: ConversationManifest = serde_json::from_value(instance.community_request(
            session,
            Method::GET,
            &format!("/dm/conversations/{id}"),
            None,
        )?)?;
        if manifest.conversation_id != id
            || manifest.origin != instance.origin
            || manifest.creator.global_identity_id != expected_peer_identity
            || manifest.creator.device_id != expected_peer_device
            || manifest.recipient.global_identity_id != self.state.identity
            || manifest.recipient.device_id != self.state.device
            || !kova_identity::dm::verify_conversation(&manifest, chrono::Utc::now().timestamp())
        {
            return Err("Conversation, origine ou appareil non vérifié.".into());
        }
        if let Some(existing) = self.state.conversations.get(id) {
            if existing.manifest != manifest {
                return Err("Manifeste contradictoire".into());
            }
            return Ok(());
        }
        verify_package(&manifest.creator)?;
        verify_package(&manifest.recipient)?;
        let mut state = self.state.clone();
        let mut crypto = CryptoClient::restore_state(&state.crypto)?;
        crypto.join_conversation(id, &hex::decode(&manifest.welcome)?)?;
        verify_members(&crypto, &manifest)?;
        if state
            .bundle
            .as_ref()
            .is_some_and(|b| b.key_package == manifest.recipient.key_package)
        {
            state.bundle = None;
        }
        state.conversations.insert(
            id.into(),
            Conversation {
                manifest,
                cursor: 0,
                source_cursors: BTreeMap::new(),
                active_policy: None,
                pending_creation: false,
                events: BTreeMap::new(),
                messages: BTreeMap::new(),
                pending: Vec::new(),
            },
        );
        state.crypto = crypto.export_state()?;
        self.save(state)
    }

    pub fn retry(&mut self, instance: &Instance, session: &Session, id: &str) -> Result<()> {
        self.check_session(instance, session)?;
        self.check_replica_origin(instance, session, id)?;
        let conversation = self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?
            .clone();
        if conversation.pending_creation {
            let ack: ConversationManifest = serde_json::from_value(instance.community_request(
                session,
                Method::POST,
                "/dm/conversations",
                Some(serde_json::to_value(&conversation.manifest)?),
            )?)?;
            if ack != conversation.manifest {
                return Err("Accusé de création contradictoire".into());
            }
            let mut state = self.state.clone();
            state
                .conversations
                .get_mut(id)
                .ok_or("Conversation inconnue")?
                .pending_creation = false;
            self.save(state)?;
        }
        for event_id in &conversation.pending {
            let event = conversation.events.get(event_id).ok_or("Envoi absent")?;
            let ack: kova_protocol::dm::StoredDmEvent =
                serde_json::from_value(instance.community_request(
                    session,
                    Method::POST,
                    &format!("/dm/conversations/{id}/events"),
                    Some(serde_json::to_value(event)?),
                )?)?;
            if ack.event != *event {
                return Err("Accusé de message contradictoire".into());
            }
            let mut state = self.state.clone();
            state
                .conversations
                .get_mut(id)
                .ok_or("Conversation inconnue")?
                .pending
                .retain(|p| p != event_id);
            self.save(state)?;
        }
        Ok(())
    }

    pub fn sync(&mut self, instance: &Instance, session: &Session, id: &str) -> Result<()> {
        self.check_session(instance, session)?;
        self.check_replica_origin(instance, session, id)?;
        for _ in 0..64 {
            let cursor = self
                .state
                .conversations
                .get(id)
                .ok_or("Conversation inconnue")?
                .source_cursors
                .get(&instance.origin)
                .copied()
                .unwrap_or(0);
            let page: DmEventPage = serde_json::from_value(instance.community_request(
                session,
                Method::GET,
                &format!("/dm/conversations/{id}/events?after={cursor}&limit=100"),
                None,
            )?)?;
            let count = page.items.len();
            self.merge_from(id, &instance.origin, page)?;
            if count < 100 {
                return Ok(());
            }
        }
        Err("Synchronisation MP incomplète après 64 lots ; relancez-la pour reprendre.".into())
    }

    #[cfg(test)]
    pub(super) fn merge(&mut self, id: &str, page: DmEventPage) -> Result<()> {
        let source = self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?
            .manifest
            .origin
            .clone();
        self.merge_from(id, &source, page)
    }

    pub(super) fn merge_from(&mut self, id: &str, source: &str, page: DmEventPage) -> Result<()> {
        if page.items.len() > 100 {
            return Err("Page MP excessive".into());
        }
        let mut state = self.state.clone();
        let mut crypto = None;
        let conversation = state
            .conversations
            .get_mut(id)
            .ok_or("Conversation inconnue")?;
        let source_cursor = conversation
            .source_cursors
            .get(source)
            .copied()
            .unwrap_or(0);
        let expected_next = page.items.last().map_or(source_cursor, |item| item.cursor);
        for (index, item) in page.items.into_iter().enumerate() {
            if item.cursor != source_cursor + index as i64 + 1
                || !kova_identity::dm::verify_event(&item.event, &conversation.manifest)
            {
                return Err("Événement MP invalide ou historique incomplet".into());
            }
            let event = item.event;
            if event
                .parents
                .iter()
                .any(|p| !conversation.events.contains_key(p))
            {
                return Err("Dépendances MP manquantes".into());
            }
            if let Some(existing) = conversation.events.get(&event.event_id) {
                if existing != &event {
                    return Err("Événement MP contradictoire".into());
                }
            } else {
                if crypto.is_none() {
                    crypto = Some(CryptoClient::restore_state(&state.crypto)?);
                }
                let decrypted = crypto
                    .as_mut()
                    .ok_or("État MLS absent")?
                    .decrypt(id, &hex::decode(&event.ciphertext)?)?;
                if decrypted.identity != event.author_identity
                    || decrypted.device != event.author_device
                {
                    return Err("Auteur MLS différent du signataire".into());
                }
                let bytes = zeroize::Zeroizing::new(decrypted.body);
                let plain: Plaintext = serde_json::from_slice(&bytes)?;
                if plain.event_id != event.event_id
                    || plain.conversation_id != id
                    || plain.parents != event.parents
                    || !kova_protocol::valid_message_body(&plain.body)
                {
                    return Err("Contexte chiffré contradictoire".into());
                }
                conversation
                    .messages
                    .insert(event.event_id.clone(), plain.body);
                conversation
                    .events
                    .insert(event.event_id.clone(), event.clone());
                conversation.cursor += 1;
            }
            conversation.pending.retain(|p| p != &event.event_id);
        }
        if expected_next != page.next_cursor {
            return Err("Curseur MP invalide".into());
        }
        conversation
            .source_cursors
            .insert(source.into(), page.next_cursor);
        if let Some(crypto) = crypto {
            state.crypto = crypto.export_state()?;
        }
        self.save(state)
    }
}
