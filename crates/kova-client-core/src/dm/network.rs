use super::*;
use kova_protocol::dm::DmEventPage;
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
}

impl DmStore {
    fn check_session(&self, instance: &Instance, session: &Session) -> Result<()> {
        instance.authorize(session)?;
        if session.profile["global_identity_id"] != self.state.identity {
            return Err("Session d'une autre identité".into());
        }
        Ok(())
    }

    fn check_origin(&self, instance: &Instance, id: &str) -> Result<()> {
        if self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?
            .manifest
            .origin
            != instance.origin
        {
            return Err("Cette instance n'est pas autorisée pour la conversation.".into());
        }
        Ok(())
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
        self.check_origin(instance, id)?;
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
        self.check_origin(instance, id)?;
        loop {
            let cursor = self
                .state
                .conversations
                .get(id)
                .ok_or("Conversation inconnue")?
                .cursor;
            let page: DmEventPage = serde_json::from_value(instance.community_request(
                session,
                Method::GET,
                &format!("/dm/conversations/{id}/events?after={cursor}&limit=100"),
                None,
            )?)?;
            let count = page.items.len();
            self.merge(id, page)?;
            if count < 100 {
                break;
            }
        }
        Ok(())
    }

    pub(super) fn merge(&mut self, id: &str, page: DmEventPage) -> Result<()> {
        if page.items.len() > 100 {
            return Err("Page MP excessive".into());
        }
        let mut state = self.state.clone();
        let mut crypto = CryptoClient::restore_state(&state.crypto)?;
        let conversation = state
            .conversations
            .get_mut(id)
            .ok_or("Conversation inconnue")?;
        for item in page.items {
            if item.cursor != conversation.cursor + 1
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
                let decrypted = crypto.decrypt(id, &hex::decode(&event.ciphertext)?)?;
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
            }
            conversation.pending.retain(|p| p != &event.event_id);
            conversation.cursor = item.cursor;
        }
        if conversation.cursor != page.next_cursor {
            return Err("Curseur MP invalide".into());
        }
        state.crypto = crypto.export_state()?;
        self.save(state)
    }
}
