//! Client-only identity, origin-scoped sessions and non-secret SQLite state.
//! No server dependency and no E2EE keys or protocol at this milestone.
use chrono::{DateTime, Utc};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{error::Error, path::Path};
use url::{Host, Url};
use zeroize::Zeroize;

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Serialize, Deserialize)]
pub struct Identity {
    root_pkcs8: Vec<u8>,
    device_pkcs8: Vec<u8>,
}
impl Drop for Identity {
    fn drop(&mut self) {
        self.root_pkcs8.zeroize();
        self.device_pkcs8.zeroize();
    }
}
impl Identity {
    pub fn generate() -> Result<Self> {
        let rng = SystemRandom::new();
        Ok(Self {
            root_pkcs8: Ed25519KeyPair::generate_pkcs8(&rng)
                .map_err(|_| "root generation failed")?
                .as_ref()
                .to_vec(),
            device_pkcs8: Ed25519KeyPair::generate_pkcs8(&rng)
                .map_err(|_| "device generation failed")?
                .as_ref()
                .to_vec(),
        })
    }
    fn keys(&self) -> Result<(Ed25519KeyPair, Ed25519KeyPair)> {
        Ok((
            Ed25519KeyPair::from_pkcs8(&self.root_pkcs8).map_err(|_| "invalid root key")?,
            Ed25519KeyPair::from_pkcs8(&self.device_pkcs8).map_err(|_| "invalid device key")?,
        ))
    }
    pub fn root_public_key(&self) -> Result<String> {
        Ok(hex::encode(self.keys()?.0.public_key().as_ref()))
    }
    pub fn global_id(&self) -> Result<String> {
        Ok(kova_identity::identity_id(
            self.keys()?.0.public_key().as_ref(),
        ))
    }
    pub fn device_id(&self) -> Result<String> {
        Ok(hex::encode(Sha256::digest(
            self.keys()?.1.public_key().as_ref(),
        )))
    }

    pub fn proof(&self, origin: &str, challenge: &Value, invite: Option<&str>) -> Result<Value> {
        validate_origin(origin)?;
        let (root, device) = self.keys()?;
        let identity = self.global_id()?;
        let device_id = self.device_id()?;
        let payload = challenge["challenge"].as_str().ok_or("missing challenge")?;
        let fields: Vec<_> = payload.split('\n').collect();
        let invite_hash = invite
            .map(|token| {
                kova_identity::public_key(token)
                    .map(|raw| hex::encode(Sha256::digest(raw)))
                    .ok_or("invalid invite")
            })
            .transpose()?
            .unwrap_or_default();
        let expected = [
            "kova-auth-desktop-v1",
            origin,
            if invite.is_some() { "join" } else { "login" },
            &identity,
            &device_id,
            &invite_hash,
            challenge["challenge_id"].as_str().ok_or("missing id")?,
        ];
        if fields.len() != 9
            || fields[..7] != expected
            || challenge["session_mode"] != "bearer"
            || challenge["expires_at"].as_str() != Some(fields[8])
            || kova_identity::public_key(fields[7]).is_none()
        {
            return Err("challenge context mismatch".into());
        }
        let remaining =
            DateTime::parse_from_rfc3339(fields[8])?.timestamp() - Utc::now().timestamp();
        if !(1..=120).contains(&remaining) {
            return Err("challenge expired or excessive lifetime".into());
        }
        let expiry = Utc::now().timestamp() + 90 * 86400;
        let public = hex::encode(device.public_key().as_ref());
        let attestation =
            kova_protocol::device_authorization(&identity, &device_id, &public, expiry);
        Ok(
            json!({ "challenge_id":challenge["challenge_id"], "signature":hex::encode(device.sign(payload.as_bytes()).as_ref()),
            "root_public_key":hex::encode(root.public_key().as_ref()), "device_public_key":public,
            "device_authorization":{"expires_at":expiry,"signature":hex::encode(root.sign(attestation.as_bytes()).as_ref())}, "invite":invite }),
        )
    }
}

pub fn validate_origin(origin: &str) -> Result<Url> {
    let url = Url::parse(origin)?;
    let local = match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain("localhost")) => true,
        _ => false,
    };
    if url.origin().ascii_serialization() != origin
        || !(url.scheme() == "https" || (url.scheme() == "http" && local))
    {
        return Err("expected exact HTTPS origin (HTTP only on loopback)".into());
    }
    Ok(url)
}

#[derive(Serialize, Deserialize)]
pub struct Session {
    origin: String,
    token: String,
    expires_at: i64,
    pub profile: Value,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

pub struct Instance {
    origin: String,
    http: reqwest::blocking::Client,
}
impl Instance {
    pub fn new(origin: &str) -> Result<Self> {
        validate_origin(origin)?;
        Ok(Self {
            origin: origin.to_owned(),
            http: reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(10))
                .build()?,
        })
    }
    pub fn login(&self, identity: &Identity, join: Option<(&str, &str, &str)>) -> Result<Session> {
        let invite = join.map(|j| j.0);
        let challenge: Value = self.http.post(format!("{}/api/v1/auth/challenges", self.origin)).header("Origin", &self.origin)
            .json(&json!({"global_identity_id":identity.global_id()?,"device_id":identity.device_id()?,
                "operation":if join.is_some() {"join"} else {"login"},"invite":invite,"session_mode":"bearer"}))
            .send()?.error_for_status()?.json()?;
        let mut proof = identity.proof(&self.origin, &challenge, invite)?;
        if let Some((_, handle, display_name)) = join {
            proof["profile"] = json!({"handle":handle,"display_name":display_name});
        }
        let mut response: Value = self
            .http
            .post(format!("{}/api/v1/auth/verify", self.origin))
            .header("Origin", &self.origin)
            .json(&proof)
            .send()?
            .error_for_status()?
            .json()?;
        let token = response["access_token"]
            .take()
            .as_str()
            .ok_or("missing session token")?
            .to_owned();
        if kova_identity::public_key(&token).is_none() || response["token_type"] != "Bearer" {
            return Err("invalid token response".into());
        }
        let seconds = response["expires_in"]
            .as_i64()
            .filter(|n| (1..=3600).contains(n))
            .ok_or("invalid session duration")?;
        if response["profile"]["global_identity_id"] != identity.global_id()? {
            return Err("identity mismatch".into());
        }
        Ok(Session {
            origin: self.origin.clone(),
            token,
            expires_at: Utc::now().timestamp() + seconds,
            profile: response["profile"].take(),
        })
    }
    fn authorize(&self, session: &Session) -> Result<()> {
        if session.origin != self.origin || session.expires_at <= Utc::now().timestamp() {
            return Err("session expired or belongs to another origin".into());
        }
        Ok(())
    }
    pub fn me(&self, session: &Session) -> Result<Value> {
        self.authorize(session)?;
        Ok(self
            .http
            .get(format!("{}/api/v1/me", self.origin))
            .bearer_auth(&session.token)
            .send()?
            .error_for_status()?
            .json()?)
    }
    pub fn logout(&self, session: &Session) -> Result<()> {
        self.authorize(session)?;
        self.http
            .post(format!("{}/api/v1/auth/logout", self.origin))
            .header("Origin", &self.origin)
            .bearer_auth(&session.token)
            .send()?
            .error_for_status()?;
        Ok(())
    }
}

/// Only public instance addresses and drafts; secrets never enter SQLite.
pub struct Cache(rusqlite::Connection);
impl Cache {
    pub fn open(path: &Path) -> Result<Self> {
        let db = rusqlite::Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS instances (origin TEXT PRIMARY KEY); CREATE TABLE IF NOT EXISTS drafts (id TEXT PRIMARY KEY, body TEXT NOT NULL);")?;
        Ok(Self(db))
    }
    pub fn remember_instance(&self, origin: &str) -> Result<()> {
        validate_origin(origin)?;
        self.0
            .execute("INSERT OR IGNORE INTO instances VALUES (?1)", [origin])?;
        Ok(())
    }
    pub fn save_draft(&self, id: &str, body: &str) -> Result<()> {
        self.0.execute(
            "INSERT INTO drafts VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
            [id, body],
        )?;
        Ok(())
    }
    pub fn draft(&self, id: &str) -> Result<String> {
        use rusqlite::OptionalExtension;
        Ok(self
            .0
            .query_row("SELECT body FROM drafts WHERE id=?1", [id], |r| r.get(0))
            .optional()?
            .unwrap_or_default())
    }
}

#[cfg(windows)]
pub mod vault {
    use super::*;
    fn entry(name: &str) -> Result<keyring::Entry> {
        Ok(keyring::Entry::new("Kova", name)?)
    }
    pub fn save_identity(identity: &Identity) -> Result<()> {
        // Serialize concurrent init commands before checking Credential Manager.
        let directory = std::path::PathBuf::from(
            std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA unavailable")?,
        )
        .join("Kova");
        std::fs::create_dir_all(&directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(directory.join("identity.lock"))?;
        lock.lock()?;
        let e = entry("identity-v1")?;
        match e.get_secret() {
            Err(keyring::Error::NoEntry) => (),
            Ok(_) => return Err("identity already exists; refusing replacement".into()),
            Err(e) => return Err(e.into()),
        }
        let mut secret = serde_json::to_vec(identity)?;
        let result = e.set_secret(&secret);
        secret.zeroize();
        result?;
        Ok(())
    }
    pub fn identity() -> Result<Identity> {
        let mut secret = entry("identity-v1")?.get_secret()?;
        let value = serde_json::from_slice(&secret);
        secret.zeroize();
        Ok(value?)
    }
    fn session_key(origin: &str) -> Result<String> {
        validate_origin(origin)?;
        Ok(format!(
            "session-v1-{}",
            hex::encode(Sha256::digest(origin.as_bytes()))
        ))
    }
    pub fn save_session(s: &Session) -> Result<()> {
        let mut secret = serde_json::to_vec(s)?;
        let result = entry(&session_key(&s.origin)?)?.set_secret(&secret);
        secret.zeroize();
        result?;
        Ok(())
    }
    pub fn session(origin: &str) -> Result<Session> {
        let mut secret = entry(&session_key(origin)?)?.get_secret()?;
        let value = serde_json::from_slice(&secret);
        secret.zeroize();
        Ok(value?)
    }
    pub fn forget_session(origin: &str) -> Result<()> {
        entry(&session_key(origin)?)?.delete_credential()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proof_checks_context_and_has_verifiable_signatures() {
        let identity = Identity::generate().unwrap();
        let id = identity.global_id().unwrap();
        let device = identity.device_id().unwrap();
        let expiry = (Utc::now() + chrono::Duration::seconds(90)).to_rfc3339();
        let payload = kova_protocol::AuthChallenge {
            origin: "https://a.example",
            operation: "login",
            identity: &id,
            device_id: &device,
            invite_hash: "",
            id: "challenge",
            nonce: &"01".repeat(32),
            expires_at: &expiry,
        }
        .canonical_desktop();
        let ch = json!({"challenge":payload,"challenge_id":"challenge","expires_at":expiry,"session_mode":"bearer"});
        let proof = identity.proof("https://a.example", &ch, None).unwrap();
        let key = kova_identity::public_key(proof["device_public_key"].as_str().unwrap()).unwrap();
        assert!(kova_identity::verify_signature(
            &key,
            ch["challenge"].as_str().unwrap(),
            proof["signature"].as_str().unwrap()
        ));
        let root = kova_identity::public_key(proof["root_public_key"].as_str().unwrap()).unwrap();
        let auth = kova_protocol::device_authorization(
            &id,
            &device,
            proof["device_public_key"].as_str().unwrap(),
            proof["device_authorization"]["expires_at"]
                .as_i64()
                .unwrap(),
        );
        assert!(kova_identity::verify_signature(
            &root,
            &auth,
            proof["device_authorization"]["signature"].as_str().unwrap()
        ));
        assert!(identity.proof("https://b.example", &ch, None).is_err());
        let mut cookie = ch.clone();
        cookie["challenge"] = json!(payload.replacen("kova-auth-desktop-v1", "kova-auth-v1", 1));
        assert!(identity.proof("https://a.example", &cookie, None).is_err());
    }
    #[test]
    fn origins_and_sessions_are_isolated() {
        for origin in [
            "http://example.com",
            "https://a.example/",
            "https://a.example/path",
            "https://user:pass@a.example",
        ] {
            assert!(validate_origin(origin).is_err());
        }
        let a = Instance::new("http://127.0.0.1:8080").unwrap();
        let s = Session {
            origin: "https://b.example".into(),
            token: "00".repeat(32),
            expires_at: Utc::now().timestamp() + 100,
            profile: json!({}),
        };
        assert!(a.authorize(&s).is_err());
    }
    #[test]
    fn draft_survives_reopen() {
        let path = std::env::temp_dir().join(format!("kova-cache-{}.db", std::process::id()));
        {
            let db = Cache::open(&path).unwrap();
            db.save_draft("composer", "Bonjour 👋").unwrap();
        }
        {
            let db = Cache::open(&path).unwrap();
            assert_eq!(db.draft("composer").unwrap(), "Bonjour 👋");
        }
        std::fs::remove_file(path).unwrap();
    }
}
