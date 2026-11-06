//! Public identity verification shared by server and future Rust clients.
use ring::signature::{ED25519, UnparsedPublicKey};
use sha2::{Digest, Sha256};

pub fn public_key(value: &str) -> Option<[u8; 32]> {
    let mut key = [0; 32];
    hex::decode_to_slice(value, &mut key).ok()?;
    (hex::encode(key) == value).then_some(key)
}

pub fn identity_id(key: &[u8]) -> String {
    format!("kova1-{}", hex::encode(Sha256::digest(key)))
}

pub fn verify_signature(key: &[u8], message: &str, signature: &str) -> bool {
    let mut bytes = [0; 64];
    if hex::decode_to_slice(signature, &mut bytes).is_err() {
        return false;
    }
    UnparsedPublicKey::new(&ED25519, key)
        .verify(message.as_bytes(), &bytes)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc8032_vector_and_invalid_signature() {
        let key =
            public_key("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a").unwrap();
        let signature = concat!(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555f",
            "b8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        );
        assert!(verify_signature(&key, "", signature));
        assert!(!verify_signature(&key, "changed", signature));
        assert!(!verify_signature(&key, "", "00"));
        assert!(public_key(&hex::encode(key).to_uppercase()).is_none());
        assert_eq!(
            identity_id(&key),
            "kova1-21fe31dfa154a261626bf854046fd2271b7bed4b6abe45aa58877ef47f9721b9"
        );
    }
}
