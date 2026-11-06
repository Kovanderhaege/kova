//! Canonical authentication payloads. UTF-8, LF separators, no trailing LF.
//! These functions accept already validated fields; never pass untrusted line breaks.
pub fn device_authorization(
    identity: &str,
    device_id: &str,
    public_key: &str,
    expires_at: i64,
) -> String {
    format!("kova-device-v1\n{identity}\n{device_id}\n{public_key}\n{expires_at}")
}

pub fn device_revocation(identity: &str, device_id: &str) -> String {
    format!("kova-revoke-v1\n{identity}\n{device_id}")
}

pub struct AuthChallenge<'a> {
    pub origin: &'a str,
    pub operation: &'a str,
    pub identity: &'a str,
    pub device_id: &'a str,
    pub invite_hash: &'a str,
    pub id: &'a str,
    pub nonce: &'a str,
    pub expires_at: &'a str,
}

impl AuthChallenge<'_> {
    /// Distinct signed context prevents cookie/desktop transport substitution.
    pub fn canonical_desktop(&self) -> String {
        self.canonical()
            .replacen("kova-auth-v1", "kova-auth-desktop-v1", 1)
    }

    pub fn canonical(&self) -> String {
        format!(
            "kova-auth-v1\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.origin,
            self.operation,
            self.identity,
            self.device_id,
            self.invite_hash,
            self.id,
            self.nonce,
            self.expires_at
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_python_v1_wire_format() {
        let challenge = AuthChallenge {
            origin: "https://a.example",
            operation: "login",
            identity: "kova1-root",
            device_id: "device",
            invite_hash: "",
            id: "id",
            nonce: "nonce",
            expires_at: "2026-11-06T12:00:00.000000Z",
        };
        assert_eq!(
            challenge.canonical(),
            "kova-auth-v1\nhttps://a.example\nlogin\nkova1-root\ndevice\n\nid\nnonce\n2026-11-06T12:00:00.000000Z"
        );
        assert_eq!(
            device_authorization("identity", "device", "key", 123),
            "kova-device-v1\nidentity\ndevice\nkey\n123"
        );
        assert_eq!(
            device_revocation("identity", "device"),
            "kova-revoke-v1\nidentity\ndevice"
        );
    }
}
