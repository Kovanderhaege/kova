use super::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
use zeroize::Zeroizing;

pub struct DmStore {
    database: Connection,
    key: age::x25519::Identity,
    // Prevent two processes from advancing the same MLS sending ratchet.
    _lock: File,
    pub(super) state: State,
}

impl DmStore {
    pub fn open(path: &Path, secret: &str, identity: &Identity) -> Result<Self> {
        let key: age::x25519::Identity =
            secret.parse().map_err(|_| "Clé de stockage MP invalide")?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.with_extension("lock"))?;
        lock.lock()?;
        let database = Connection::open(path)?;
        database.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS dm_state (id INTEGER PRIMARY KEY CHECK(id=1), ciphertext BLOB NOT NULL);")?;
        let encrypted: Option<Vec<u8>> = database
            .query_row("SELECT ciphertext FROM dm_state WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        let state = if let Some(encrypted) = encrypted {
            let plaintext = Zeroizing::new(age::decrypt(&key, &encrypted)?);
            let state: State = serde_json::from_slice(&plaintext)?;
            if state.version != 1
                || state.identity != identity.global_id()?
                || state.device != identity.device_id()?
            {
                return Err("Coffre MP incompatible ou appartenant à un autre appareil".into());
            }
            CryptoClient::restore_state(&state.crypto)?;
            state
        } else {
            State {
                version: 1,
                identity: identity.global_id()?,
                device: identity.device_id()?,
                crypto: CryptoClient::new(&identity.global_id()?, &identity.device_id()?)?
                    .export_state()?,
                bundle: None,
                conversations: BTreeMap::new(),
            }
        };
        let mut store = Self {
            database,
            key,
            _lock: lock,
            state,
        };
        store.save(store.state.clone())?;
        Ok(store)
    }

    pub(super) fn save(&mut self, state: State) -> Result<()> {
        let plaintext = Zeroizing::new(serde_json::to_vec(&state)?);
        let ciphertext = age::encrypt(&self.key.to_public(), &plaintext)?;
        self.database.execute("INSERT INTO dm_state(id,ciphertext) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET ciphertext=excluded.ciphertext", params![ciphertext])?;
        self.state = state;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use age::secrecy::ExposeSecret;
    #[test]
    fn encrypted_state_restarts_and_rejects_other_keys_and_devices() {
        let path = std::env::temp_dir().join(format!("kova-dm-{}.sqlite", uuid::Uuid::new_v4()));
        let identity = Identity::generate().unwrap();
        let key = age::x25519::Identity::generate().to_string();
        {
            let mut store = DmStore::open(&path, key.expose_secret(), &identity).unwrap();
            let mut state = store.state.clone();
            // Exercise encryption with recognizable secret material.
            state.bundle = None;
            let crypto = CryptoClient::restore_state(&state.crypto).unwrap();
            crypto.key_package().unwrap();
            state.crypto = crypto.export_state().unwrap();
            store.save(state).unwrap();
            assert!(store.render(None).unwrap().contains("Conversations"));
            assert!(
                !std::fs::read(&path)
                    .unwrap()
                    .windows(identity.global_id().unwrap().len())
                    .any(|w| w == identity.global_id().unwrap().as_bytes())
            );
        }
        assert!(DmStore::open(&path, key.expose_secret(), &identity).is_ok());
        let wrong = age::x25519::Identity::generate().to_string();
        assert!(DmStore::open(&path, wrong.expose_secret(), &identity).is_err());
        assert!(DmStore::open(&path, key.expose_secret(), &Identity::generate().unwrap()).is_err());
        let db = Connection::open(&path).unwrap();
        db.execute("UPDATE dm_state SET ciphertext=zeroblob(500)", [])
            .unwrap();
        drop(db);
        assert!(DmStore::open(&path, key.expose_secret(), &identity).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
