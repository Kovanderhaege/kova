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
    #[serde(default)]
    linked_certificate: Option<Value>,
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
            linked_certificate: None,
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
    fn root_key(&self) -> Result<Ed25519KeyPair> {
        Ed25519KeyPair::from_pkcs8(&self.root_pkcs8)
            .map_err(|_| "Cet appareil ne possède pas la racine.".into())
    }
    fn device_key(&self) -> Result<Ed25519KeyPair> {
        Ed25519KeyPair::from_pkcs8(&self.device_pkcs8).map_err(|_| "Clé appareil invalide.".into())
    }
    pub fn is_root(&self) -> bool {
        !self.root_pkcs8.is_empty()
    }
    pub fn root_public_key(&self) -> Result<String> {
        if let Some(cert) = &self.linked_certificate {
            return Ok(cert["root_public_key"]
                .as_str()
                .ok_or("Certificat invalide")?
                .to_owned());
        }
        Ok(hex::encode(self.root_key()?.public_key().as_ref()))
    }
    pub fn global_id(&self) -> Result<String> {
        Ok(kova_identity::identity_id(
            &kova_identity::public_key(&self.root_public_key()?).ok_or("Racine invalide")?,
        ))
    }
    pub fn device_id(&self) -> Result<String> {
        Ok(hex::encode(Sha256::digest(
            self.device_key()?.public_key().as_ref(),
        )))
    }
    pub fn certificate(&self) -> Result<Value> {
        if let Some(cert) = &self.linked_certificate {
            if !kova_protocol::authorization_is_fresh(
                cert["device_authorization"]["issued_at"]
                    .as_i64()
                    .unwrap_or(0),
                cert["device_authorization"]["expires_at"]
                    .as_i64()
                    .unwrap_or(0),
                Utc::now().timestamp(),
            ) {
                return Err("Autorisation expirée : renouveler depuis l'appareil racine.".into());
            }
            return Ok(cert.clone());
        }
        self.authorize_key(&hex::encode(self.device_key()?.public_key().as_ref()))
    }
    fn authorize_key(&self, public: &str) -> Result<Value> {
        let key = kova_identity::public_key(public).ok_or("Clé appareil invalide")?;
        let device_id = hex::encode(Sha256::digest(key));
        let issued = Utc::now().timestamp();
        let expiry = issued + 86400;
        let payload = kova_protocol::device_authorization_v2(
            &self.global_id()?,
            &device_id,
            public,
            issued,
            expiry,
        );
        Ok(
            json!({"root_public_key":self.root_public_key()?,"device_public_key":public,
            "device_authorization":{"issued_at":issued,"expires_at":expiry,"signature":hex::encode(self.root_key()?.sign(payload.as_bytes()).as_ref())}}),
        )
    }
    pub fn revocation(&self, device_public_key: &str) -> Result<Value> {
        let key = kova_identity::public_key(device_public_key).ok_or("Clé publique invalide")?;
        let device_id = hex::encode(Sha256::digest(key));
        let payload = kova_protocol::device_revocation(&self.global_id()?, &device_id);
        Ok(
            json!({"global_identity_id":self.global_id()?,"device_public_key":device_public_key,
            "signature":hex::encode(self.root_key()?.sign(payload.as_bytes()).as_ref())}),
        )
    }

    pub fn proof(&self, origin: &str, challenge: &Value, invite: Option<&str>) -> Result<Value> {
        validate_origin(origin)?;
        let device = self.device_key()?;
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
        let mut proof = self.certificate()?;
        proof["challenge_id"] = challenge["challenge_id"].clone();
        proof["signature"] = json!(hex::encode(device.sign(payload.as_bytes()).as_ref()));
        proof["invite"] = json!(invite);
        Ok(proof)
    }
}

mod cache;
pub mod pairing;
pub mod recovery;
mod transport;
#[cfg(windows)]
pub mod vault;
pub use cache::Cache;
pub use transport::{Instance, Session, validate_origin};

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
        let auth = kova_protocol::device_authorization_v2(
            &id,
            &device,
            proof["device_public_key"].as_str().unwrap(),
            proof["device_authorization"]["issued_at"].as_i64().unwrap(),
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
