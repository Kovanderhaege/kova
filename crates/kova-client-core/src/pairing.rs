//! File-based pairing over a user-authenticated channel, no secret key transfer.
use super::*;
use ring::rand::SecureRandom;

#[derive(Serialize, Deserialize)]
pub struct PendingDevice {
    secret: Vec<u8>,
    pub request: Value,
}
impl Drop for PendingDevice {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}
fn request_payload(request: &Value) -> Result<String> {
    let public = request["device_public_key"].as_str().ok_or("Clé absente")?;
    let nonce = request["nonce"].as_str().ok_or("Nonce absent")?;
    let expiry = request["expires_at"].as_i64().ok_or("Échéance absente")?;
    if request["version"] != 1
        || kova_identity::public_key(public).is_none()
        || kova_identity::public_key(nonce).is_none()
        || !expiry
            .checked_sub(Utc::now().timestamp())
            .is_some_and(|remaining| (1..=600).contains(&remaining))
    {
        return Err("Demande invalide ou expirée.".into());
    }
    Ok(format!("kova-link-request-v1\n{public}\n{nonce}\n{expiry}"))
}
pub fn fingerprint(request: &Value) -> Result<String> {
    Ok(hex::encode(Sha256::digest(
        request_payload(request)?.as_bytes(),
    )))
}
impl PendingDevice {
    pub fn create() -> Result<Self> {
        Self::from_secret(Identity::generate()?.device_pkcs8.clone())
    }
    pub fn renew(identity: &Identity) -> Result<Self> {
        Self::from_secret(identity.device_pkcs8.clone())
    }
    fn from_secret(secret: Vec<u8>) -> Result<Self> {
        let device = Ed25519KeyPair::from_pkcs8(&secret).map_err(|_| "Clé invalide")?;
        let mut nonce = [0u8; 32];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| "Aléa indisponible")?;
        let mut request = json!({"version":1,"device_public_key":hex::encode(device.public_key().as_ref()),"nonce":hex::encode(nonce),"expires_at":Utc::now().timestamp()+600});
        request["signature"] = json!(hex::encode(
            device.sign(request_payload(&request)?.as_bytes()).as_ref()
        ));
        Ok(Self { secret, request })
    }
    pub fn complete(&self, response: &Value, expected_root: &str) -> Result<Identity> {
        let root = kova_identity::public_key(expected_root).ok_or("Empreinte racine invalide")?;
        if response["root_public_key"] != expected_root
            || response["request_hash"] != fingerprint(&self.request)?
        {
            return Err("Réponse non liée à la demande/racine attendue.".into());
        }
        let cert = &response["certificate"];
        let public = self.request["device_public_key"]
            .as_str()
            .ok_or("Clé absente")?;
        if cert["device_public_key"] != public || cert["root_public_key"] != expected_root {
            return Err("Appareil différent.".into());
        }
        let expiry = cert["device_authorization"]["expires_at"]
            .as_i64()
            .ok_or("Échéance absente")?;
        let issued = cert["device_authorization"]["issued_at"]
            .as_i64()
            .ok_or("Date d'émission absente")?;
        if !kova_protocol::authorization_is_fresh(issued, expiry, Utc::now().timestamp()) {
            return Err("Autorisation expirée/invalide.".into());
        }
        let device_id = hex::encode(Sha256::digest(hex::decode(public)?));
        let payload = kova_protocol::device_authorization_v2(
            &kova_identity::identity_id(&root),
            &device_id,
            public,
            issued,
            expiry,
        );
        let binding = format!(
            "kova-link-response-v1\n{}\n{}",
            fingerprint(&self.request)?,
            payload
        );
        if !kova_identity::verify_signature(
            &root,
            &payload,
            cert["device_authorization"]["signature"]
                .as_str()
                .unwrap_or(""),
        ) || !kova_identity::verify_signature(
            &root,
            &binding,
            response["signature"].as_str().unwrap_or(""),
        ) {
            return Err("Signature d'association invalide.".into());
        }
        Ok(Identity {
            root_pkcs8: Vec::new(),
            device_pkcs8: self.secret.clone(),
            linked_certificate: Some(cert.clone()),
        })
    }
}
pub fn approve(owner: &Identity, request: &Value, confirmed_fingerprint: &str) -> Result<Value> {
    let hash = fingerprint(request)?;
    if hash != confirmed_fingerprint {
        return Err("Empreinte non confirmée.".into());
    }
    let public = request["device_public_key"].as_str().ok_or("Clé absente")?;
    let key = kova_identity::public_key(public).ok_or("Clé invalide")?;
    if !kova_identity::verify_signature(
        &key,
        &request_payload(request)?,
        request["signature"].as_str().unwrap_or(""),
    ) {
        return Err("Preuve de possession invalide.".into());
    }
    let cert = owner.authorize_key(public)?;
    let payload = kova_protocol::device_authorization_v2(
        &owner.global_id()?,
        &hex::encode(Sha256::digest(key)),
        public,
        cert["device_authorization"]["issued_at"]
            .as_i64()
            .ok_or("Date d'émission absente")?,
        cert["device_authorization"]["expires_at"]
            .as_i64()
            .ok_or("Échéance absente")?,
    );
    let binding = format!("kova-link-response-v1\n{hash}\n{payload}");
    Ok(
        json!({"root_public_key":owner.root_public_key()?,"request_hash":hash,"certificate":cert,"signature":hex::encode(owner.root_key()?.sign(binding.as_bytes()).as_ref())}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairing_never_transfers_root_and_rejects_substitution() {
        let owner = Identity::generate().unwrap();
        let pending = PendingDevice::create().unwrap();
        let response = approve(
            &owner,
            &pending.request,
            &fingerprint(&pending.request).unwrap(),
        )
        .unwrap();
        let linked = pending
            .complete(&response, &owner.root_public_key().unwrap())
            .unwrap();
        assert!(!linked.is_root());
        assert!(linked.root_pkcs8.is_empty());
        assert!(
            linked
                .revocation(&owner.root_public_key().unwrap())
                .is_err()
        );
        assert!(recovery::backup(&linked, "synthetic long password").is_err());
        assert_eq!(linked.global_id().unwrap(), owner.global_id().unwrap());
        assert!(approve(&owner, &pending.request, "wrong").is_err());
        assert!(
            PendingDevice::create()
                .unwrap()
                .complete(&response, &owner.root_public_key().unwrap())
                .is_err()
        );
        assert!(
            pending
                .complete(
                    &response,
                    &Identity::generate().unwrap().root_public_key().unwrap()
                )
                .is_err()
        );
        let mut altered = response.clone();
        altered["signature"] = json!("00".repeat(64));
        assert!(
            pending
                .complete(&altered, &owner.root_public_key().unwrap())
                .is_err()
        );
        let mut expired = linked;
        expired.linked_certificate.as_mut().unwrap()["device_authorization"]["expires_at"] =
            json!(Utc::now().timestamp() - 1);
        assert!(expired.certificate().is_err());
        let mut expired_request = pending.request.clone();
        expired_request["expires_at"] = json!(Utc::now().timestamp() - 1);
        assert!(fingerprint(&expired_request).is_err());
    }
}
