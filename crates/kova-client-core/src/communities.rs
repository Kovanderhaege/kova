//! Community operations use the same origin-bound session as identity operations.
use crate::{Instance, Result, Session};
use reqwest::Method;
use serde_json::{Value, json};

fn validate_id(id: &str) -> Result<()> {
    // IDs are path segments, never arbitrary paths or query parameters.
    if id.len() != 36
        || !id.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return Err("Identifiant de communauté ou de membre invalide.".into());
    }
    Ok(())
}

fn community_path(id: &str) -> Result<String> {
    validate_id(id)?;
    Ok(format!("/communities/{id}"))
}

impl Instance {
    pub fn communities(&self, session: &Session) -> Result<Value> {
        self.community_request(session, Method::GET, "/communities", None)
    }
    pub fn create_community(&self, session: &Session, name: &str) -> Result<Value> {
        self.community_request(
            session,
            Method::POST,
            "/communities",
            Some(json!({"name":name})),
        )
    }
    pub fn channels(&self, session: &Session, community: &str) -> Result<Value> {
        self.community_request(
            session,
            Method::GET,
            &format!("{}/channels", community_path(community)?),
            None,
        )
    }
    pub fn create_channel(&self, session: &Session, community: &str, name: &str) -> Result<Value> {
        self.community_request(
            session,
            Method::POST,
            &format!("{}/channels", community_path(community)?),
            Some(json!({"name":name})),
        )
    }
    pub fn members(&self, session: &Session, community: &str) -> Result<Value> {
        self.community_request(
            session,
            Method::GET,
            &format!("{}/members", community_path(community)?),
            None,
        )
    }
    pub fn create_invite(
        &self,
        session: &Session,
        community: &str,
        expires_in: i32,
    ) -> Result<Value> {
        self.community_request(
            session,
            Method::POST,
            &format!("{}/invites", community_path(community)?),
            Some(json!({"expires_in":expires_in})),
        )
    }
    pub fn redeem_invite(&self, session: &Session, invite: &str) -> Result<Value> {
        self.community_request(
            session,
            Method::POST,
            "/invites/redeem",
            Some(json!({"invite":invite})),
        )
    }
    pub fn set_member_role(
        &self,
        session: &Session,
        community: &str,
        user: &str,
        role: &str,
    ) -> Result<()> {
        validate_id(user)?;
        if !matches!(role, "member" | "moderator") {
            return Err("Rôle invalide : member ou moderator attendu.".into());
        }
        self.community_request(
            session,
            Method::PATCH,
            &format!("{}/members/{user}", community_path(community)?),
            Some(json!({"role":role})),
        )?;
        Ok(())
    }
    pub fn exclude_member(&self, session: &Session, community: &str, user: &str) -> Result<()> {
        validate_id(user)?;
        self.community_request(
            session,
            Method::DELETE,
            &format!("{}/members/{user}", community_path(community)?),
            None,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn community_calls_reject_expired_or_foreign_sessions_before_transport() {
        let instance = Instance::new("http://127.0.0.1:1").unwrap();
        let mut session = Session {
            origin: "http://127.0.0.1:2".into(),
            token: "test-secret".into(),
            expires_at: chrono::Utc::now().timestamp() + 60,
            profile: Value::Null,
        };
        assert!(
            instance
                .communities(&session)
                .unwrap_err()
                .to_string()
                .contains("another origin")
        );
        session.origin = "http://127.0.0.1:1".into();
        session.expires_at = 0;
        assert!(
            instance
                .create_community(&session, "Atelier")
                .unwrap_err()
                .to_string()
                .contains("expired")
        );
    }

    #[test]
    fn path_identifiers_cannot_inject_routes_or_queries() {
        for id in [
            "../identity/devices",
            "id?admin=true",
            "id/channels",
            "",
            "00000000-0000-0000-0000-00000000000z",
        ] {
            assert!(validate_id(id).is_err());
        }
        assert!(validate_id("00000000-0000-0000-0000-000000000001").is_ok());
    }
}
