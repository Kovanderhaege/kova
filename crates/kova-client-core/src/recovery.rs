//! Portable age-encrypted root backup. Device secrets and sessions are excluded.
use super::*;
use std::io::{Read, Write};
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize)]
struct RootBackup {
    version: u8,
    root_pkcs8: String,
}
impl Drop for RootBackup {
    fn drop(&mut self) {
        self.root_pkcs8.zeroize();
    }
}

pub fn backup(identity: &Identity, passphrase: &str) -> Result<Vec<u8>> {
    if passphrase.chars().count() < 12 {
        return Err("Phrase secrète : au moins 12 caractères.".into());
    }
    identity.root_key()?;
    let secret = Zeroizing::new(serde_json::to_vec(&RootBackup {
        version: 1,
        root_pkcs8: hex::encode(&identity.root_pkcs8),
    })?);
    Ok(age::encrypt(
        &age::scrypt::Recipient::new(age::secrecy::SecretString::from(passphrase.to_owned())),
        &secret,
    )?)
}

pub fn restore(ciphertext: &[u8], passphrase: &str) -> Result<Identity> {
    if ciphertext.len() > 65536 {
        return Err("Sauvegarde trop volumineuse.".into());
    }
    let secret = Zeroizing::new(age::decrypt(
        &age::scrypt::Identity::new(age::secrecy::SecretString::from(passphrase.to_owned())),
        ciphertext,
    )?);
    let data: RootBackup = serde_json::from_slice(&secret)?;
    if data.version != 1 {
        return Err("Version de sauvegarde inconnue.".into());
    }
    let decoded = hex::decode(&data.root_pkcs8);
    let mut identity = Identity::generate()?;
    identity.root_pkcs8.zeroize();
    identity.root_pkcs8 = decoded?;
    identity.root_key()?;
    Ok(identity)
}

pub fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    std::fs::File::open(path)?
        .take(65537)
        .read_to_end(&mut data)?;
    if data.len() > 65536 {
        return Err("Fichier trop volumineux.".into());
    }
    Ok(data)
}
pub fn write_new(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(data)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_recovery_preserves_root_but_rotates_device() {
        let identity = Identity::generate().unwrap();
        let bytes = backup(&identity, "a long recovery passphrase").unwrap();
        assert!(!bytes.windows(16).any(|w| w == &identity.root_pkcs8[..16]));
        let restored = restore(&bytes, "a long recovery passphrase").unwrap();
        assert_eq!(identity.global_id().unwrap(), restored.global_id().unwrap());
        assert_ne!(identity.device_id().unwrap(), restored.device_id().unwrap());
        assert!(restore(&bytes, "wrong password").is_err());
        let mut corrupt = bytes;
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        assert!(restore(&corrupt, "a long recovery passphrase").is_err());
    }
}
