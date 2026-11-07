//! Community commands run on the existing background executor, with the selected vault session.
use kova_client_core::{Instance, Result, vault::Vault};

pub enum Command {
    List,
    Create {
        name: String,
    },
    Channels {
        community: String,
    },
    CreateChannel {
        community: String,
        name: String,
    },
    Members {
        community: String,
    },
    Invite {
        community: String,
    },
    SetRole {
        community: String,
        member: String,
        role: &'static str,
    },
    Exclude {
        community: String,
        member: String,
    },
}

pub fn execute(vault: &Vault, origin: &str, command: Command) -> Result<String> {
    let instance = Instance::new(origin)?;
    let session = vault.session(origin)?;
    let result = match command {
        Command::List => instance.communities(&session)?,
        Command::Create { name } => instance.create_community(&session, &name)?,
        Command::Channels { community } => instance.channels(&session, &community)?,
        Command::CreateChannel { community, name } => {
            instance.create_channel(&session, &community, &name)?
        }
        Command::Members { community } => instance.members(&session, &community)?,
        Command::Invite { community } => {
            let invite = instance.create_invite(&session, &community, 86400)?;
            return Ok(format!(
                "Invitation à usage unique, valable 24 h. Transmettez-la uniquement au destinataire. Elle ne sera plus affichée après navigation.\n{}",
                invite["invite"]
                    .as_str()
                    .ok_or("Réponse d'invitation invalide")?
            ));
        }
        Command::SetRole {
            community,
            member,
            role,
        } => {
            instance.set_member_role(&session, &community, &member, role)?;
            return Ok("Rôle mis à jour.".into());
        }
        Command::Exclude { community, member } => {
            instance.exclude_member(&session, &community, &member)?;
            return Ok("Membre exclu de cette communauté. Les invitations ne permettent plus à cette identité de la rejoindre.".into());
        }
    };
    Ok(serde_json::to_string_pretty(&result)?)
}
