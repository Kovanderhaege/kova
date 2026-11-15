//! Direct-message commands. Crypto, storage, and HTTP run on GPUI's background executor.
use kova_client_core::{Identity, Instance, Result, dm::DmStore, vault::Vault};

pub enum Command {
    Publish,
    List,
    Create {
        peer_identity: String,
        peer_device: String,
    },
    Accept {
        id: String,
        peer_identity: String,
        peer_device: String,
    },
    Sync {
        id: String,
    },
    AutoSync {
        id: String,
    },
    Send {
        id: String,
        body: String,
    },
    Retry {
        id: String,
    },
    Offline {
        id: String,
    },
    ProposeReplica {
        id: String,
        peer_origin: String,
    },
    RemoveReplica {
        id: String,
        peer_origin: String,
    },
    ReviewReplica {
        id: String,
    },
    ApproveReplica {
        id: String,
    },
    SyncReplica {
        id: String,
        peer_origin: String,
    },
}

fn open_store(vault: &Vault, identity: &Identity) -> Result<DmStore> {
    let directory = vault.directory()?;
    let secret = vault.dm_secret()?;
    DmStore::open(&directory.join("dm.sqlite"), &secret, identity)
}

pub fn execute(vault: &Vault, origin: &str, command: Command) -> Result<String> {
    let identity = vault.identity()?;
    let mut store = open_store(vault, &identity)?;
    if let Command::Offline { id } = command {
        return Ok(format!(
            "Identité : {}\nAppareil : {}\n{}",
            identity.global_id()?,
            identity.device_id()?,
            store.render(if id.is_empty() { None } else { Some(&id) })?
        ));
    }
    let instance = Instance::new(origin)?;
    match command {
        Command::Publish => {
            let session = vault.session(origin)?;
            store.publish(&instance, &session, &identity)?;
            Ok(format!(
                "Appareil publié pour {} · {}. Communiquez ces empreintes par un canal sûr.",
                identity.global_id()?,
                identity.device_id()?
            ))
        }
        Command::List => {
            let session = vault.session(origin)?;
            let conversations = instance.dm_conversations(&session)?;
            let listing = conversations
                .iter()
                .map(|conversation| {
                    format!(
                        "{}\n{} / {}\n{} / {}\n",
                        conversation.conversation_id,
                        conversation.creator.global_identity_id,
                        conversation.creator.device_id,
                        conversation.recipient.global_identity_id,
                        conversation.recipient.device_id
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(format!(
                "Identité locale : {}\nAppareil local : {}\nConversations proposées par l'instance (vérifier avant acceptation) :\n{}\n\n{}",
                identity.global_id()?,
                identity.device_id()?,
                listing,
                store.render(None)?
            ))
        }
        Command::Create {
            peer_identity,
            peer_device,
        } => {
            let session = vault.session(origin)?;
            let id = store.create(&instance, &session, &identity, &peer_identity, &peer_device)?;
            Ok(format!(
                "Conversation {id} créée et en attente d'acceptation. Vérifiez et transmettez son identifiant au correspondant."
            ))
        }
        Command::Accept {
            id,
            peer_identity,
            peer_device,
        } => {
            let session = vault.session(origin)?;
            store.accept(
                &instance,
                &session,
                &identity,
                &id,
                &peer_identity,
                &peer_device,
            )?;
            Ok(format!(
                "Conversation {id} acceptée après vérification de l'identité et de l'appareil."
            ))
        }
        Command::Sync { id } => {
            let selected =
                store.sync_available(origin, &id, |candidate| vault.session(candidate))?;
            Ok(format!(
                "Synchronisation automatique via {selected}.\n{}",
                store.render(Some(&id))?
            ))
        }
        Command::AutoSync { id } => {
            match store.retry_available(origin, &id, |candidate| vault.session(candidate)) {
                Ok(selected) => Ok(format!(
                    "Synchronisation de fond via {selected}.\n{}",
                    store.render(Some(&id))?
                )),
                Err(error) => Err(error),
            }
        }
        Command::Send { id, body } => {
            store.prepare(&identity, &id, &body)?;
            let selected = store
                .retry_available(origin, &id, |candidate| vault.session(candidate))
                .map_err(|error| format!(
                    "Envoi non confirmé. Si un message figure en attente dans l'historique local, utilisez Réessayer. Détail : {error}"
                ))?;
            Ok(format!(
                "Envoi transmis via {selected}.\n{}",
                store.render(Some(&id))?
            ))
        }
        Command::Retry { id } => {
            let selected =
                store.retry_available(origin, &id, |candidate| vault.session(candidate))?;
            Ok(format!(
                "Envoi en attente transmis via {selected}.\n{}",
                store.render(Some(&id))?
            ))
        }
        Command::Offline { id } => store.render(Some(&id)),
        Command::ProposeReplica { id, peer_origin } => {
            let session = vault.session(origin)?;
            let policy =
                store.propose_replica(&instance, &session, &identity, &id, &peer_origin)?;
            Ok(render_policy(
                &policy,
                "Proposition préparée localement. Vérifiez-la puis approuvez pour la transmettre.",
            ))
        }
        Command::RemoveReplica { id, peer_origin } => {
            let session = vault.session(origin)?;
            let policy =
                store.propose_replica_removal(&instance, &session, &identity, &id, &peer_origin)?;
            Ok(render_policy(
                &policy,
                "Retrait proposé. Les copies déjà conservées par l'instance ne peuvent pas être effacées.",
            ))
        }
        Command::ReviewReplica { id } => {
            let session = vault.session(origin)?;
            let policy = store.replica_proposal(&instance, &session, &id)?;
            Ok(render_policy(
                &policy,
                "Vérifiez les origines et empreintes ci-dessous. Utilisez Approuver dans une action distincte.",
            ))
        }
        Command::ApproveReplica { id } => {
            let session = vault.session(origin)?;
            let policy = store.approve_replica_proposal(&instance, &session, &identity, &id)?;
            Ok(render_policy(
                &policy,
                "Approbation enregistrée. La politique devient active après l'approbation de l'autre participant.",
            ))
        }
        Command::SyncReplica { id, peer_origin } => {
            let session = vault.session(origin)?;
            let (received, sent) = store.sync_replicas(&instance, &session, &id, &peer_origin)?;
            Ok(format!(
                "Réplication terminée avec {peer_origin} : {received} événement(s) reçu(s), {sent} envoyé(s).\n{}",
                store.render(Some(&id))?
            ))
        }
    }
}

fn render_policy(policy: &kova_protocol::replication::ReplicaPolicy, heading: &str) -> String {
    let replicas = policy
        .replicas
        .iter()
        .map(|replica| format!("{} · clé {}", replica.origin, replica.public_key))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{heading}\nConversation : {}\nVersion : {}\nEmpreinte : {}\nApprobations : {} / 2\nRéplicas :\n{}",
        policy.conversation_id,
        policy.version,
        kova_identity::replication::policy_hash(policy),
        policy.approvals.len(),
        replicas
    )
}
