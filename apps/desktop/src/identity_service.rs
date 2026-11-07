//! Blocking identity operations, executed away from the GPUI event loop.
use kova_client_core::{Cache, Identity, Instance, Result, pairing, recovery, vault::Vault};
use serde_json::Value;
use std::path::Path;
use zeroize::Zeroizing;

pub enum Command {
    Inspect,
    Create,
    Backup {
        path: String,
        password: Zeroizing<String>,
    },
    Restore {
        path: String,
        password: Zeroizing<String>,
    },
    Request {
        path: String,
    },
    Approve {
        path: String,
        fingerprint: String,
        output: String,
    },
    Complete {
        path: String,
        root: String,
    },
    Connect {
        origin: String,
        invite: String,
        handle: String,
        display: String,
    },
    Devices {
        origin: String,
    },
    Logout {
        origin: String,
    },
    Revoke {
        public_key: String,
    },
    Retry,
}

fn read_public(path: &str) -> Result<Value> {
    Ok(serde_json::from_slice(&recovery::read_bounded(
        Path::new(path),
    )?)?)
}
fn describe(identity: &Identity) -> Result<String> {
    Ok(format!(
        "Identité : {}\nClé publique racine : {}\nAppareil : {}\n{}",
        identity.global_id()?,
        identity.root_public_key()?,
        identity.device_id()?,
        if identity.is_root() {
            "Appareil racine : sauvegardez votre identité avec une phrase secrète conservée séparément. Sans sauvegarde, sa perte est définitive."
        } else {
            "Appareil associé : autorisation valable 24 h au maximum. Renouveler avec l'appareil racine avant expiration."
        }
    ))
}
fn propagate(cache: &Cache) -> Result<String> {
    if cache.instances()?.is_empty() {
        return Ok("Journal conservé localement. Aucune instance connue : connectez ce profil aux instances concernées pour le transmettre.".into());
    }
    let failures = cache.flush_revocations()?;
    Ok(if failures.is_empty() {
        "Journal transmis aux instances connues. Les instances non connues peuvent accepter une autorisation jusqu'à son expiration (24 h maximum).".into()
    } else {
        format!(
            "Révocation conservée. Transmission à réessayer : {}. Une instance inaccessible peut accepter l'appareil jusqu'à expiration de son autorisation (24 h maximum).",
            failures.join(", ")
        )
    })
}
pub fn execute(profile: &str, command: Command) -> Result<String> {
    let vault = Vault::new(profile)?;
    match command {
        Command::Inspect => {
            if vault.has_identity()? {
                describe(&vault.identity()?)
            } else {
                Ok("Profil vide. Créez, restaurez ou associez une identité.".into())
            }
        }
        Command::Create => {
            let identity = Identity::generate()?;
            vault.install(&identity)?;
            describe(&identity)
        }
        Command::Backup { path, password } => {
            recovery::write_new(
                Path::new(&path),
                &recovery::backup(&vault.identity()?, &password)?,
            )?;
            Ok("Sauvegarde chiffrée créée. Conservez la phrase secrète séparément ; elle ne peut pas être récupérée.".into())
        }
        Command::Restore { path, password } => {
            let identity =
                recovery::restore(&recovery::read_bounded(Path::new(&path))?, &password)?;
            vault.install(&identity)?;
            describe(&identity)
        }
        Command::Request { path } => {
            let pending = if vault.has_identity()? {
                let identity = vault.identity()?;
                if identity.is_root() {
                    return Err(
                        "Utiliser un nouveau profil pour associer un autre appareil.".into(),
                    );
                }
                pairing::PendingDevice::renew(&identity)?
            } else {
                pairing::PendingDevice::create()?
            };
            // Persist the secret before publishing its public request.
            vault.save_pending(&pending)?;
            recovery::write_new(
                Path::new(&path),
                &serde_json::to_vec_pretty(&pending.request)?,
            )?;
            Ok(format!(
                "Demande valable 10 minutes. Comparez cette empreinte complète sur l'appareil racine via un canal de confiance :\n{}",
                pairing::fingerprint(&pending.request)?
            ))
        }
        Command::Approve {
            path,
            fingerprint,
            output,
        } => {
            let response =
                pairing::approve(&vault.identity()?, &read_public(&path)?, &fingerprint)?;
            recovery::write_new(Path::new(&output), &serde_json::to_vec_pretty(&response)?)?;
            Ok("Autorisation exportée. Transmettez le fichier de réponse au nouvel appareil ; aucune clé privée n'est transférée.".into())
        }
        Command::Complete { path, root } => {
            vault.complete_pairing(&read_public(&path)?, &root)?;
            describe(&vault.identity()?)
        }
        Command::Connect {
            origin,
            invite,
            handle,
            display,
        } => {
            let instance = Instance::new(&origin)?;
            let cache = Cache::open(&vault.directory()?.join("cache.sqlite"))?;
            let join = if invite.is_empty() {
                None
            } else {
                Some((invite.as_str(), handle.as_str(), display.as_str()))
            };
            let session = instance.login(&vault.identity()?, join)?;
            vault.save_session(&session)?;
            cache.remember_instance(&origin)?;
            cache.merge_revocations(
                &vault.identity()?.root_public_key()?,
                &instance.revocations(&session)?,
            )?;
            Ok(format!(
                "Connecté à {origin}\n{}\n{}",
                serde_json::to_string_pretty(&session.profile)?,
                propagate(&cache)?
            ))
        }
        Command::Devices { origin } => Ok(serde_json::to_string_pretty(
            &Instance::new(&origin)?.devices(&vault.session(&origin)?)?,
        )?),
        Command::Logout { origin } => {
            Instance::new(&origin)?.logout(&vault.session(&origin)?)?;
            vault.forget_session(&origin)?;
            Ok("Session déconnectée.".into())
        }
        Command::Revoke { public_key } => {
            let cache = Cache::open(&vault.directory()?.join("cache.sqlite"))?;
            cache.queue_revocation(&vault.identity()?.revocation(&public_key)?)?;
            propagate(&cache)
        }
        Command::Retry => propagate(&Cache::open(&vault.directory()?.join("cache.sqlite"))?),
    }
}
