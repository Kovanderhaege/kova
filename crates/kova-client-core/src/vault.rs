//! Windows Credential Manager scoped to an explicitly named local profile.
use super::*;
use serde::de::DeserializeOwned;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Vault {
    profile: String,
}
impl Vault {
    pub fn new(profile: &str) -> Result<Self> {
        if profile.is_empty()
            || profile.len() > 32
            || !profile
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("Profil : 1–32 lettres/chiffres/tirets.".into());
        }
        Ok(Self {
            profile: profile.into(),
        })
    }
    pub fn directory(&self) -> Result<std::path::PathBuf> {
        let path = std::path::PathBuf::from(
            std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA absent")?,
        )
        .join("Kova")
        .join("profiles")
        .join(&self.profile);
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }
    fn entry(&self, name: &str) -> Result<keyring::Entry> {
        let service = if self.profile == "default" {
            "Kova".to_owned()
        } else {
            format!("Kova.{}", self.profile)
        };
        Ok(keyring::Entry::new(&service, name)?)
    }
    fn write<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let secret = Zeroizing::new(serde_json::to_vec(value)?);
        self.entry(name)?.set_secret(&secret)?;
        Ok(())
    }
    fn read<T: DeserializeOwned>(&self, name: &str) -> Result<T> {
        let secret = Zeroizing::new(self.entry(name)?.get_secret()?);
        Ok(serde_json::from_slice(&secret)?)
    }
    fn lock(&self) -> Result<std::fs::File> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.directory()?.join("identity.lock"))?;
        file.lock()?;
        Ok(file)
    }
    pub fn has_identity(&self) -> Result<bool> {
        match self.entry("identity-v1")?.get_secret() {
            Ok(mut secret) => {
                secret.zeroize();
                Ok(true)
            }
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
    pub fn identity(&self) -> Result<Identity> {
        self.read("identity-v1")
    }
    pub fn install(&self, identity: &Identity) -> Result<()> {
        let _lock = self.lock()?;
        if self.has_identity()? {
            return Err("Une identité existe déjà dans ce profil. Choisir un nouveau profil pour restaurer.".into());
        }
        self.write("identity-v1", identity)
    }
    pub fn save_pending(&self, pending: &pairing::PendingDevice) -> Result<()> {
        self.write("pairing-v1", pending)
    }
    pub fn pending(&self) -> Result<pairing::PendingDevice> {
        self.read("pairing-v1")
    }
    pub fn complete_pairing(&self, response: &Value, expected_root: &str) -> Result<()> {
        let _lock = self.lock()?;
        let linked = self.pending()?.complete(response, expected_root)?;
        if self.has_identity()? {
            let current = self.identity()?;
            if current.is_root()
                || current.global_id()? != linked.global_id()?
                || current.device_id()? != linked.device_id()?
            {
                return Err("L'association remplacerait une autre identité/appareil.".into());
            }
        }
        self.write("identity-v1", &linked)?;
        self.entry("pairing-v1")?.delete_credential()?;
        Ok(())
    }
    fn session_key(origin: &str) -> Result<String> {
        validate_origin(origin)?;
        Ok(format!(
            "session-v1-{}",
            hex::encode(Sha256::digest(origin.as_bytes()))
        ))
    }
    pub fn save_session(&self, session: &Session) -> Result<()> {
        self.write(&Self::session_key(&session.origin)?, session)
    }
    pub fn session(&self, origin: &str) -> Result<Session> {
        self.read(&Self::session_key(origin)?)
    }
    pub fn forget_session(&self, origin: &str) -> Result<()> {
        match self.entry(&Self::session_key(origin)?)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}
// Compatibility entry points for the original diagnostic CLI.
pub fn save_identity(identity: &Identity) -> Result<()> {
    Vault::new("default")?.install(identity)
}
pub fn identity() -> Result<Identity> {
    Vault::new("default")?.identity()
}
pub fn save_session(session: &Session) -> Result<()> {
    Vault::new("default")?.save_session(session)
}
pub fn session(origin: &str) -> Result<Session> {
    Vault::new("default")?.session(origin)
}
pub fn forget_session(origin: &str) -> Result<()> {
    Vault::new("default")?.forget_session(origin)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_vault_pairs_without_overwriting_root_and_scopes_sessions() {
        let name = format!("test-{}-{}", std::process::id(), Utc::now().timestamp());
        let vault = Vault::new(&name).unwrap();
        struct Cleanup(Vault);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for name in ["identity-v1", "pairing-v1"] {
                    let _ = self
                        .0
                        .entry(name)
                        .and_then(|entry| Ok(entry.delete_credential()?));
                }
                let _ = self.0.forget_session("http://localhost:8001");
            }
        }
        let _cleanup = Cleanup(vault.clone());
        assert!(!vault.has_identity().unwrap());
        let owner = Identity::generate().unwrap();
        let pending = pairing::PendingDevice::create().unwrap();
        vault.save_pending(&pending).unwrap();
        let response = pairing::approve(
            &owner,
            &pending.request,
            &pairing::fingerprint(&pending.request).unwrap(),
        )
        .unwrap();
        vault
            .complete_pairing(&response, &owner.root_public_key().unwrap())
            .unwrap();
        let linked = vault.identity().unwrap();
        assert_eq!(linked.global_id().unwrap(), owner.global_id().unwrap());
        assert!(!linked.is_root());
        assert!(vault.pending().is_err());
        assert!(vault.install(&owner).is_err());
        let renewal = pairing::PendingDevice::renew(&linked).unwrap();
        vault.save_pending(&renewal).unwrap();
        let response = pairing::approve(
            &owner,
            &renewal.request,
            &pairing::fingerprint(&renewal.request).unwrap(),
        )
        .unwrap();
        vault
            .complete_pairing(&response, &owner.root_public_key().unwrap())
            .unwrap();
        assert_eq!(
            vault.identity().unwrap().device_id().unwrap(),
            linked.device_id().unwrap()
        );
        let session = Session {
            origin: "http://localhost:8001".into(),
            token: "synthetic test token".into(),
            expires_at: Utc::now().timestamp() + 60,
            profile: json!({}),
        };
        vault.save_session(&session).unwrap();
        assert_eq!(vault.session(&session.origin).unwrap().token, session.token);
        assert!(vault.session("http://localhost:8002").is_err());
        vault.forget_session(&session.origin).unwrap();
        assert!(vault.session(&session.origin).is_err());
    }

    #[test]
    fn windows_vault_restores_root_without_replacing_existing_identity() {
        let name = format!("root-{}-{}", std::process::id(), Utc::now().timestamp());
        let vault = Vault::new(&name).unwrap();
        struct Cleanup(Vault);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = self.0.entry("identity-v1").unwrap().delete_credential();
            }
        }
        let _cleanup = Cleanup(vault.clone());
        let original = Identity::generate().unwrap();
        let backup = recovery::backup(&original, "synthetic recovery passphrase").unwrap();
        let restored = recovery::restore(&backup, "synthetic recovery passphrase").unwrap();
        vault.install(&restored).unwrap();
        let reopened = Vault::new(&name).unwrap().identity().unwrap();
        assert!(reopened.is_root());
        assert_eq!(reopened.global_id().unwrap(), original.global_id().unwrap());
        assert_ne!(reopened.device_id().unwrap(), original.device_id().unwrap());
        assert!(vault.install(&Identity::generate().unwrap()).is_err());
        assert_eq!(
            vault.identity().unwrap().global_id().unwrap(),
            original.global_id().unwrap()
        );
    }
}
