use super::*;

impl DmStore {
    pub(super) fn ensure_conversation_authorized(&self, id: &str) -> Result<()> {
        let conversation = self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?;
        if !conversation.authorization_state.revocations.is_empty() {
            return Err("Un appareil de cette conversation est révoqué définitivement. Les nouveaux envois sont bloqués ; l'historique local est conservé.".into());
        }
        if kova_identity::dm::conversation_authorized(
            &conversation.manifest,
            &conversation.authorization_state,
            chrono::Utc::now().timestamp(),
        ) {
            Ok(())
        } else {
            Err("Preuves MP expirées : chaque participant doit renouveler son appareil. Un appareil associé doit obtenir une nouvelle autorisation de son appareil racine, puis réessayer.".into())
        }
    }

    /// Renews the local device authorization while preserving the MLS group and its queue.
    /// Associated devices need their root device to issue a fresh certificate first.
    pub fn renew_authorization(
        &mut self,
        instance: &Instance,
        session: &Session,
        identity: &Identity,
        id: &str,
    ) -> Result<()> {
        self.check_identity(identity)?;
        self.check_session(instance, session)?;
        self.check_replica_origin(instance, session, id)?;
        let original = {
            let conversation = self
                .state
                .conversations
                .get(id)
                .ok_or("Conversation inconnue")?;
            if conversation.manifest.creator.global_identity_id == self.state.identity
                && conversation.manifest.creator.device_id == self.state.device
            {
                conversation.manifest.creator.clone()
            } else if conversation.manifest.recipient.global_identity_id == self.state.identity
                && conversation.manifest.recipient.device_id == self.state.device
            {
                conversation.manifest.recipient.clone()
            } else {
                return Err("Cet appareil ne participe pas à la conversation".into());
            }
        };
        let bundle = signed_bundle(identity, hex::decode(&original.key_package)?)?;
        if !kova_identity::dm::verify_renewal(&bundle, &original) {
            return Err(
                "La preuve renouvelée ne correspond pas à l'appareil et au groupe MLS existants."
                    .into(),
            );
        }

        // Keep the signed proof durable before network I/O so a restart can retry it.
        let mut state = self.state.clone();
        merge_authorizations(
            state
                .conversations
                .get_mut(id)
                .ok_or("Conversation inconnue")?,
            DmAuthorizationState {
                authorizations: vec![bundle.clone()],
                revocations: Vec::new(),
            },
        )?;
        self.save(state)?;

        let response = instance.renew_dm_authorization(session, id, &bundle)?;
        self.merge_authorization_response(id, response)
    }

    pub(super) fn refresh_authorizations(
        &mut self,
        instance: &Instance,
        session: &Session,
        id: &str,
    ) -> Result<()> {
        let response = instance.dm_authorizations(session, id)?;
        self.merge_authorization_response(id, response)
    }

    pub(super) fn publish_local_authorization(
        &mut self,
        instance: &Instance,
        session: &Session,
        id: &str,
    ) -> Result<()> {
        let own_bundle = self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?
            .authorization_state
            .authorizations
            .iter()
            .find(|bundle| {
                bundle.global_identity_id == self.state.identity
                    && bundle.device_id == self.state.device
            })
            .cloned();
        let Some(bundle) = own_bundle else {
            return Ok(());
        };
        if kova_identity::dm::verify_renewal(
            &bundle,
            if self.state.conversations[id].manifest.creator.device_id == self.state.device {
                &self.state.conversations[id].manifest.creator
            } else {
                &self.state.conversations[id].manifest.recipient
            },
        ) && kova_protocol::authorization_is_fresh(
            bundle.issued_at,
            bundle.expires_at,
            chrono::Utc::now().timestamp(),
        ) {
            let response = instance.renew_dm_authorization(session, id, &bundle)?;
            self.merge_authorization_response(id, response)?;
        }
        Ok(())
    }

    pub(super) fn merge_authorization_response(
        &mut self,
        id: &str,
        incoming: DmAuthorizationState,
    ) -> Result<()> {
        let mut state = self.state.clone();
        let revocations = incoming.revocations.clone();
        let conversation = state
            .conversations
            .get_mut(id)
            .ok_or("Conversation inconnue")?;
        merge_authorizations(conversation, incoming)?;
        // A root revocation concerns a device, across all of its conversations.
        for conversation in state.conversations.values_mut() {
            let relevant: Vec<_> = revocations
                .iter()
                .filter(|proof| kova_identity::dm::verify_revocation(proof, &conversation.manifest))
                .cloned()
                .collect();
            if !relevant.is_empty() {
                merge_authorizations(
                    conversation,
                    DmAuthorizationState {
                        authorizations: Vec::new(),
                        revocations: relevant,
                    },
                )?;
            }
        }
        self.save(state)
    }
}

pub(super) fn merge_authorizations(
    conversation: &mut Conversation,
    incoming: DmAuthorizationState,
) -> Result<()> {
    if !kova_identity::dm::verify_authorization_state(&incoming, &conversation.manifest)
        || incoming
            .authorizations
            .iter()
            .any(|bundle| bundle.issued_at > chrono::Utc::now().timestamp())
    {
        return Err("État des autorisations MP invalide".into());
    }
    for fresh in incoming.authorizations {
        let existing = conversation
            .authorization_state
            .authorizations
            .iter_mut()
            .find(|item| {
                item.global_identity_id == fresh.global_identity_id
                    && item.device_id == fresh.device_id
            });
        if let Some(existing) = existing {
            if kova_identity::dm::authorization_order(&fresh)
                > kova_identity::dm::authorization_order(existing)
            {
                *existing = fresh;
            }
        } else {
            conversation.authorization_state.authorizations.push(fresh);
        }
    }
    conversation
        .authorization_state
        .authorizations
        .sort_by(|left, right| {
            (&left.global_identity_id, &left.device_id)
                .cmp(&(&right.global_identity_id, &right.device_id))
        });
    for proof in incoming.revocations {
        if !conversation
            .authorization_state
            .revocations
            .iter()
            .any(|existing| {
                existing.global_identity_id == proof.global_identity_id
                    && existing.device_public_key == proof.device_public_key
            })
        {
            conversation.authorization_state.revocations.push(proof);
        }
    }
    conversation
        .authorization_state
        .revocations
        .sort_by(|left, right| {
            (
                &left.global_identity_id,
                &left.device_public_key,
                &left.signature,
            )
                .cmp(&(
                    &right.global_identity_id,
                    &right.device_public_key,
                    &right.signature,
                ))
        });
    if !kova_identity::dm::verify_authorization_state(
        &conversation.authorization_state,
        &conversation.manifest,
    ) {
        return Err("État fusionné des autorisations MP invalide".into());
    }
    Ok(())
}
