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
    let session = vault.session(origin)?;

    match command {
        Command::Publish => {
            store.publish(&instance, &session, &identity)?;
            Ok(format!(
                "Appareil publié pour {} · {}. Communiquez ces empreintes par un canal sûr.",
                identity.global_id()?,
                identity.device_id()?
            ))
        }
        Command::List => {
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
            store.sync(&instance, &session, &id)?;
            store.render(Some(&id))
        }
        Command::Send { id, body } => {
            if let Err(error) = store.send(&instance, &session, &identity, &id, &body) {
                return Err(format!(
                    "Envoi non confirmé. Si un message figure en attente dans l'historique local, utilisez Réessayer. Détail : {error}"
                ).into());
            }
            store.render(Some(&id))
        }
        Command::Retry { id } => {
            store.retry(&instance, &session, &id)?;
            store.render(Some(&id))
        }
        Command::Offline { id } => store.render(Some(&id)),
    }
}
