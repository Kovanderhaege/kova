//! Typed data adapter for the navigable Windows workspace.
use kova_client_core::{Cache, Instance, Result, chat::ChatScope, dm::DmStore, vault::Vault};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub author: String,
    pub date: String,
    pub body: String,
    pub pending: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub conversation_sources: std::collections::BTreeMap<String, String>,
    pub communities: Vec<Choice>,
    pub channels: Vec<Choice>,
    pub members: Vec<Choice>,
    pub conversations: Vec<Choice>,
    pub messages: Vec<Message>,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Channel(String),
    Dm(String),
}

fn items(value: &Value) -> &[Value] {
    value["items"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn choice(value: &Value, id_key: &str, label_keys: &[&str]) -> Option<Choice> {
    let id = value[id_key].as_str()?.to_owned();
    let label = label_keys
        .iter()
        .find_map(|key| value[*key].as_str())
        .filter(|label| !label.is_empty())
        .unwrap_or(&id)
        .to_owned();
    Some(Choice { id, label })
}

fn read_local_store(vault: &Vault, identity: &kova_client_core::Identity) -> Result<DmStore> {
    let directory = vault.directory()?;
    let secret = vault.dm_secret()?;
    DmStore::open(&directory.join("dm.sqlite"), &secret, identity)
}

fn append_status(status: &mut String, message: impl AsRef<str>) {
    if !status.is_empty() {
        status.push_str(" · ");
    }
    status.push_str(message.as_ref());
}

/// Loads current-origin lists while keeping cached message history available offline.
/// Proposed remote conversations are listed for review but never accepted here.
pub fn load(
    profile: &str,
    origin: &str,
    community: Option<&str>,
    target: Option<&Target>,
    limit: usize,
) -> Result<Snapshot> {
    let vault = Vault::new(profile)?;
    let identity = vault.identity()?;
    let identity_id = identity.global_id()?;
    let cache = Cache::open(&vault.directory()?.join("cache.sqlite"))?;
    let mut snapshot = Snapshot::default();
    let instance = Instance::new(origin)?;

    match vault.session(origin) {
        Ok(session) => {
            match instance.communities(&session) {
                Ok(value) => snapshot.communities.extend(
                    items(&value)
                        .iter()
                        .filter_map(|entry| choice(entry, "id", &["name"])),
                ),
                Err(error) => append_status(
                    &mut snapshot.status,
                    format!("Communautés indisponibles : {error}"),
                ),
            }
            if let Some(community_id) = community {
                match instance.channels(&session, community_id) {
                    Ok(value) => snapshot.channels.extend(
                        items(&value)
                            .iter()
                            .filter_map(|entry| choice(entry, "id", &["name"])),
                    ),
                    Err(error) => append_status(
                        &mut snapshot.status,
                        format!("Salons indisponibles : {error}"),
                    ),
                }
                match instance.members(&session, community_id) {
                    Ok(value) => {
                        snapshot
                            .members
                            .extend(items(&value).iter().filter_map(|entry| {
                                let mut member = choice(entry, "id", &["display_name", "handle"])?;
                                if let Some(role) = entry["role"].as_str() {
                                    member.label.push_str(" · ");
                                    member.label.push_str(role);
                                }
                                Some(member)
                            }))
                    }
                    Err(error) => append_status(
                        &mut snapshot.status,
                        format!("Membres indisponibles : {error}"),
                    ),
                }
            }
        }
        Err(error) => append_status(
            &mut snapshot.status,
            format!("Hors ligne sur cette instance : {error}"),
        ),
    }

    let store = read_local_store(&vault, &identity)?;
    let local_conversations = store.local_conversations();
    for conversation in &local_conversations {
        snapshot.conversations.push(Choice {
            id: conversation.id.clone(),
            label: if conversation.pending > 0 {
                format!(
                    "MP avec {} · {} · {} en attente",
                    short_identity(&conversation.peer_identity),
                    conversation.origin,
                    conversation.pending
                )
            } else {
                format!(
                    "MP avec {} · {}",
                    short_identity(&conversation.peer_identity),
                    conversation.origin
                )
            },
        });
    }
    for known_origin in cache
        .instances()?
        .into_iter()
        .chain([origin.to_owned()])
        .collect::<BTreeSet<_>>()
    {
        let session = match vault.session(&known_origin) {
            Ok(session) => session,
            Err(error) => {
                append_status(
                    &mut snapshot.status,
                    format!("Session {known_origin} indisponible : {error}"),
                );
                continue;
            }
        };
        let remote =
            match Instance::new(&known_origin).and_then(|peer| peer.dm_conversations(&session)) {
                Ok(remote) => remote,
                Err(error) => {
                    append_status(
                        &mut snapshot.status,
                        format!("MP {known_origin} indisponibles : {error}"),
                    );
                    continue;
                }
            };
        for manifest in remote {
            let counterpart = if manifest.creator.global_identity_id == identity_id {
                &manifest.recipient.global_identity_id
            } else {
                &manifest.creator.global_identity_id
            };
            if !snapshot
                .conversations
                .iter()
                .any(|item| item.id == manifest.conversation_id)
            {
                snapshot
                    .conversation_sources
                    .insert(manifest.conversation_id.clone(), known_origin.clone());
                snapshot.conversations.push(Choice {
                    id: manifest.conversation_id,
                    label: format!(
                        "MP avec {} · {} · à vérifier",
                        short_identity(counterpart),
                        known_origin
                    ),
                });
            }
        }
    }

    drop(store);
    if let Some(target) = target {
        if let Target::Dm(id) = target
            && snapshot.conversation_sources.contains_key(id)
        {
            append_status(
                &mut snapshot.status,
                "Proposition distante : vérifiez puis acceptez depuis Paramètres → MP.",
            );
        } else {
            match timeline(profile, origin, target, limit, &snapshot.members) {
                Ok(messages) => snapshot.messages = messages,
                Err(error) => append_status(&mut snapshot.status, error.to_string()),
            }
        }
    }

    if snapshot.status.is_empty() {
        snapshot.status = "Connecté".into();
    }
    Ok(snapshot)
}

/// Reads only local caches for the selected timeline; safe for live UI refreshes.
/// It performs no network request and never accepts a remote DM proposal.
pub fn timeline(
    profile: &str,
    origin: &str,
    target: &Target,
    limit: usize,
    members: &[Choice],
) -> Result<Vec<Message>> {
    let vault = Vault::new(profile)?;
    let identity = vault.identity()?;
    let identity_id = identity.global_id()?;
    match target {
        Target::Channel(channel) => {
            let scope = ChatScope::new(origin, &identity_id, channel)?;
            let cache = Cache::open(&vault.directory()?.join("cache.sqlite"))?;
            let mut messages = cache
                .cached_messages_page(&scope, limit)?
                .into_iter()
                .map(|message| {
                    let author = members
                        .iter()
                        .find(|member| member.id == message.author_id)
                        .map(|member| member.label.split(" · ").next().unwrap_or(&member.label))
                        .map(str::to_owned)
                        .unwrap_or_else(|| short_identity(&message.author_id).to_owned());
                    Message {
                        author,
                        date: message.created_at,
                        body: message.body,
                        pending: false,
                    }
                })
                .collect::<Vec<_>>();
            if let Some(pending) = cache.pending_message(&scope)? {
                messages.push(Message {
                    author: "Vous".into(),
                    date: "En attente".into(),
                    body: pending.body,
                    pending: true,
                });
            }
            Ok(messages)
        }
        Target::Dm(id) => {
            let store = read_local_store(&vault, &identity)?;
            if !store
                .local_conversations()
                .iter()
                .any(|conversation| conversation.id == *id)
            {
                return Err(
                    "Proposition distante : vérifiez puis acceptez depuis Paramètres → MP.".into(),
                );
            }
            Ok(store
                .local_messages(id, limit)?
                .into_iter()
                .map(|message| Message {
                    author: if message.author_identity == identity_id {
                        "Vous".into()
                    } else {
                        "Correspondant".into()
                    },
                    date: "Date non attestée".into(),
                    body: message.body,
                    pending: message.pending,
                })
                .collect())
        }
    }
}

fn short_identity(identity: &str) -> &str {
    identity.get(..18).unwrap_or(identity)
}

pub fn send(profile: &str, origin: &str, target: &Target, body: &str) -> Result<()> {
    match target {
        Target::Channel(channel) => crate::chat_service::execute(
            &Vault::new(profile)?,
            origin,
            channel,
            crate::chat_service::Command::Send(body.to_owned()),
        )
        .map(|_| ()),
        Target::Dm(id) => crate::dm_service::execute(
            &Vault::new(profile)?,
            origin,
            crate::dm_service::Command::Send {
                id: id.clone(),
                body: body.to_owned(),
            },
        )
        .map(|_| ()),
    }
}

pub fn sync(profile: &str, origin: &str, target: &Target) -> Result<()> {
    match target {
        Target::Channel(channel) => crate::chat_service::execute(
            &Vault::new(profile)?,
            origin,
            channel,
            crate::chat_service::Command::Sync,
        )
        .map(|_| ()),
        Target::Dm(id) => crate::dm_service::execute(
            &Vault::new(profile)?,
            origin,
            crate::dm_service::Command::Sync { id: id.clone() },
        )
        .map(|_| ()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn community_choice_prefers_name_and_falls_back_to_id() {
        let named = serde_json::json!({"id":"community-id","name":"Atelier"});
        assert_eq!(
            choice(&named, "id", &["name"]),
            Some(Choice {
                id: "community-id".into(),
                label: "Atelier".into(),
            })
        );
        let unnamed = serde_json::json!({"id":"member-id"});
        assert_eq!(
            choice(&unnamed, "id", &["display_name", "handle"]),
            Some(Choice {
                id: "member-id".into(),
                label: "member-id".into(),
            })
        );
    }

    #[test]
    fn list_adapter_requires_items_array() {
        assert!(items(&serde_json::json!({"items":[{"id":"x"}]})).len() == 1);
        assert!(items(&serde_json::json!({"communities":[]})).is_empty());
    }

    #[test]
    #[ignore = "requires the connected Windows desktop recipe fixture"]
    fn workspace_adapter_loads_recipe_community_and_encrypted_dm() {
        let state_path = std::path::PathBuf::from(
            std::env::var("KOVA_WORKSPACE_RECIPE").expect("KOVA_WORKSPACE_RECIPE"),
        );
        let state: Value = serde_json::from_slice(&std::fs::read(state_path).unwrap()).unwrap();
        let profile = state["profiles"]["bob"].as_str().unwrap();
        let origin = state["origins"]["a"].as_str().unwrap();
        let community = state["communities"]["a"].as_str().unwrap();
        let channel = state["channels"]["a"].as_str().unwrap();
        let conversation = state["conversation_id"].as_str().unwrap();

        let lists = load(profile, origin, Some(community), None, 50).unwrap();
        assert!(lists.communities.iter().any(|item| item.id == community));
        assert!(lists.channels.iter().any(|item| item.id == channel));
        assert!(!lists.members.is_empty());
        let messages = load(
            profile,
            origin,
            None,
            Some(&Target::Dm(conversation.to_owned())),
            50,
        )
        .unwrap();
        assert!(
            messages
                .conversations
                .iter()
                .any(|item| item.id == conversation)
        );
        assert!(messages.messages.iter().any(|item| {
            item.body == "desktop-recipe-initial-message" && item.date == "Date non attestée"
        }));
        let offline = timeline(
            profile,
            "http://127.0.0.1:1",
            &Target::Dm(conversation.into()),
            50,
            &[],
        )
        .unwrap();
        assert_eq!(offline, messages.messages);
    }
}
