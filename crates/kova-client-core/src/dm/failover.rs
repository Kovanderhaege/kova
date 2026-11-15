use super::*;
use reqwest::Method;

fn is_missing_session(error: &(dyn std::error::Error + 'static)) -> bool {
    error.downcast_ref::<crate::SessionUnavailable>().is_some()
}

fn retryable_unavailability(error: &(dyn std::error::Error + 'static)) -> bool {
    if let Some(response) = error.downcast_ref::<crate::transport::HttpResponseError>() {
        return response.status.is_server_error();
    }
    if let Some(request) = error.downcast_ref::<reqwest::Error>() {
        return request.is_connect()
            || request.is_timeout()
            || request
                .status()
                .is_some_and(|status| status.is_server_error());
    }
    false
}

impl DmStore {
    /// Synchronizes from an approved replica, preferring the requested origin.
    /// Sessions are supplied by the caller so this module never reaches into the OS vault.
    pub fn sync_available<F>(
        &mut self,
        preferred_origin: &str,
        id: &str,
        mut session_for: F,
    ) -> Result<String>
    where
        F: FnMut(&str) -> Result<Session>,
    {
        self.with_available_replica(
            preferred_origin,
            id,
            &mut session_for,
            |store, instance, session| store.sync(instance, session, id),
        )
    }

    /// Retries durable outgoing events on an approved replica after syncing it.
    pub fn retry_available<F>(
        &mut self,
        preferred_origin: &str,
        id: &str,
        mut session_for: F,
    ) -> Result<String>
    where
        F: FnMut(&str) -> Result<Session>,
    {
        self.with_available_replica(
            preferred_origin,
            id,
            &mut session_for,
            |store, instance, session| {
                let pending_creation = store
                    .state
                    .conversations
                    .get(id)
                    .is_some_and(|conversation| conversation.pending_creation);
                if !pending_creation {
                    store.refresh_authorizations(instance, session, id)?;
                    store.publish_local_authorization(instance, session, id)?;
                    let peers = store
                        .state
                        .conversations
                        .get(id)
                        .and_then(|conversation| conversation.active_policy.as_ref())
                        .map(|policy| {
                            policy
                                .replicas
                                .iter()
                                .map(|replica| replica.origin.clone())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    for peer in peers.into_iter().filter(|peer| peer != &instance.origin) {
                        match store.sync_replicas(instance, session, id, &peer) {
                            Ok(_) => {}
                            Err(error) if retryable_unavailability(error.as_ref()) => {}
                            Err(error) => return Err(error),
                        }
                    }
                    store.replay_pending_ancestors(instance, session, id)?;
                    store.sync(instance, session, id)?;
                }
                store.retry(instance, session, id)
            },
        )
    }

    fn replay_pending_ancestors(
        &mut self,
        instance: &Instance,
        session: &Session,
        id: &str,
    ) -> Result<()> {
        self.check_session(instance, session)?;
        self.check_replica_origin(instance, session, id)?;
        let conversation = self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?;
        let mut required = std::collections::BTreeSet::new();
        let mut pending = conversation.pending.clone();
        while let Some(event_id) = pending.pop() {
            if !required.insert(event_id.clone()) {
                continue;
            }
            let event = conversation
                .events
                .get(&event_id)
                .ok_or("Envoi MP absent du cache")?;
            pending.extend(event.parents.iter().cloned());
        }
        let mut remaining: Vec<_> = required
            .into_iter()
            .filter_map(|event_id| {
                conversation
                    .events
                    .get(&event_id)
                    .map(|event| (event_id, event))
            })
            .collect();
        let mut sent = std::collections::BTreeSet::new();
        while !remaining.is_empty() {
            let Some(index) = remaining.iter().position(|(_, event)| {
                event.parents.iter().all(|parent| {
                    sent.contains(parent) || !conversation.events.contains_key(parent)
                })
            }) else {
                return Err("Dépendances circulaires dans l'outbox locale".into());
            };
            let (event_id, event) = remaining.remove(index);
            if event.author_identity == self.state.identity
                && event.author_device == self.state.device
            {
                let ack: kova_protocol::dm::StoredDmEvent =
                    serde_json::from_value(instance.community_request(
                        session,
                        Method::POST,
                        &format!("/dm/conversations/{id}/events"),
                        Some(serde_json::to_value(event)?),
                    )?)?;
                if ack.event != *event {
                    return Err("Accusé de reprise MP contradictoire".into());
                }
            }
            sent.insert(event_id);
        }
        Ok(())
    }

    fn with_available_replica<F, O>(
        &mut self,
        preferred_origin: &str,
        id: &str,
        session_for: &mut F,
        mut operation: O,
    ) -> Result<String>
    where
        F: FnMut(&str) -> Result<Session>,
        O: FnMut(&mut Self, &Instance, &Session) -> Result<()>,
    {
        let conversation = self
            .state
            .conversations
            .get(id)
            .ok_or("Conversation inconnue")?;
        let mut allowed = if let Some(policy) = &conversation.active_policy {
            if !kova_identity::replication::verify_policy(policy, &conversation.manifest) {
                return Err("Politique de réplication conservée invalide".into());
            }
            policy
                .replicas
                .iter()
                .map(|replica| replica.origin.clone())
                .collect::<Vec<_>>()
        } else {
            vec![conversation.manifest.origin.clone()]
        };
        if !allowed.iter().any(|origin| origin == preferred_origin) {
            return Err("Origine préférée absente de la politique approuvée".into());
        }
        allowed.sort();
        allowed.dedup();
        let mut candidates = vec![preferred_origin.to_owned()];
        candidates.extend(
            allowed
                .into_iter()
                .filter(|origin| origin != preferred_origin),
        );
        let mut last_unavailable: Option<Box<dyn std::error::Error + Send + Sync>> = None;
        for origin in candidates {
            if let Some(policy) = self
                .state
                .conversations
                .get(id)
                .and_then(|conversation| conversation.active_policy.as_ref())
            {
                if !kova_identity::replication::verify_policy(
                    policy,
                    &self.state.conversations[id].manifest,
                ) {
                    return Err("Politique de réplication conservée invalide".into());
                }
                if !policy
                    .replicas
                    .iter()
                    .any(|replica| replica.origin == origin)
                {
                    continue;
                }
            } else if origin != self.state.conversations[id].manifest.origin {
                continue;
            }
            let session = match session_for(&origin) {
                Ok(session) => session,
                Err(error) if is_missing_session(error.as_ref()) => {
                    last_unavailable = Some(error);
                    continue;
                }
                Err(error) => return Err(error),
            };
            let instance = Instance::new(&origin)?;
            if session.origin != origin {
                return Err("Session d'une autre origine".into());
            }
            if session.profile["global_identity_id"] != self.state.identity {
                return Err("Session d'une autre identité".into());
            }
            if session.expires_at <= chrono::Utc::now().timestamp() {
                last_unavailable = Some("Session expirée pour ce réplica".into());
                continue;
            }
            self.check_session(&instance, &session)?;
            if let Some(policy) = self
                .state
                .conversations
                .get(id)
                .and_then(|item| item.active_policy.as_ref())
            {
                let pinned = policy
                    .replicas
                    .iter()
                    .find(|replica| replica.origin == origin)
                    .ok_or("Réplica absent de la politique approuvée")?;
                let observed = match Instance::replica_descriptor(&origin) {
                    Ok(observed) => observed,
                    Err(error) if retryable_unavailability(error.as_ref()) => {
                        last_unavailable = Some(error);
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if &observed != pinned {
                    return Err("Identité du réplica différente de l'empreinte approuvée".into());
                }
            }
            match operation(self, &instance, &session) {
                Ok(()) => return Ok(origin),
                Err(error) if retryable_unavailability(error.as_ref()) => {
                    last_unavailable = Some(error)
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_unavailable.unwrap_or_else(|| "Aucun réplica autorisé disponible".into()))
    }
}
