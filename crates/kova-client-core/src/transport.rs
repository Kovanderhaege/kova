use super::*;
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
    pub(crate) origin: String,
    pub(crate) token: String,
    pub(crate) expires_at: i64,
    pub profile: Value,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

pub struct Instance {
    pub(crate) origin: String,
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
    pub(crate) fn authorize(&self, session: &Session) -> Result<()> {
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

impl Instance {
    pub fn revocations(&self, session: &Session) -> Result<Value> {
        self.authorize(session)?;
        Ok(self
            .http
            .get(format!("{}/api/v1/identity/revocations", self.origin))
            .bearer_auth(&session.token)
            .send()?
            .error_for_status()?
            .json()?)
    }
    pub fn devices(&self, session: &Session) -> Result<Value> {
        self.authorize(session)?;
        Ok(self
            .http
            .get(format!("{}/api/v1/identity/devices", self.origin))
            .bearer_auth(&session.token)
            .send()?
            .error_for_status()?
            .json()?)
    }
    pub fn publish_revocation(&self, proof: &Value) -> Result<()> {
        self.http
            .post(format!("{}/api/v1/identity/revocations", self.origin))
            .header("Origin", &self.origin)
            .json(proof)
            .send()?
            .error_for_status()?;
        Ok(())
    }
}
