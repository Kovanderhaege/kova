use super::*;
/// Only public instance addresses and drafts; secrets never enter SQLite.
pub struct Cache(rusqlite::Connection);
impl Cache {
    pub fn open(path: &Path) -> Result<Self> {
        let db = rusqlite::Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS instances (origin TEXT PRIMARY KEY); CREATE TABLE IF NOT EXISTS revocations (event_id TEXT PRIMARY KEY, proof TEXT NOT NULL); CREATE TABLE IF NOT EXISTS drafts (id TEXT PRIMARY KEY, body TEXT NOT NULL);")?;
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

#[cfg(test)]
mod journal_tests {
    use super::*;
    #[test]
    fn signed_union_rejects_tampering_and_never_forgets_revocations() {
        let cache = Cache::open(Path::new(":memory:")).unwrap();
        let owner = Identity::generate().unwrap();
        let device = Identity::generate().unwrap();
        let public = device.certificate().unwrap()["device_public_key"]
            .as_str()
            .unwrap()
            .to_owned();
        let proof = owner.revocation(&public).unwrap();
        let payload = kova_protocol::device_revocation(
            &owner.global_id().unwrap(),
            &device.device_id().unwrap(),
        );
        let mut journal = json!({"items":[{"event_id":hex::encode(Sha256::digest(payload.as_bytes())),"payload":payload,"signature":proof["signature"],"device_public_key":public}]});
        let root = owner.root_public_key().unwrap();
        cache.merge_revocations(&root, &journal).unwrap();
        cache.merge_revocations(&root, &journal).unwrap();
        cache
            .merge_revocations(&root, &json!({"items":[]}))
            .unwrap();
        let count: i64 = cache
            .0
            .query_row("SELECT count(*) FROM revocations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        assert!(
            cache
                .merge_revocations(&device.root_public_key().unwrap(), &journal)
                .is_err()
        );
        journal["items"][0]["signature"] = json!("00".repeat(64));
        assert!(cache.merge_revocations(&root, &journal).is_err());
        assert!(
            cache
                .merge_revocations(&root, &json!({"items":null}))
                .is_err()
        );
    }
}

impl Cache {
    /// Only accept canonical events signed by this profile's pinned root.
    pub fn merge_revocations(&self, root_public_key: &str, journal: &Value) -> Result<()> {
        let root = kova_identity::public_key(root_public_key).ok_or("Racine invalide")?;
        let identity = kova_identity::identity_id(&root);
        let items = journal["items"]
            .as_array()
            .filter(|items| items.len() <= 1024)
            .ok_or("Journal invalide ou incomplet")?;
        let mut proofs = Vec::new();
        for event in items {
            let public = event["device_public_key"].as_str().ok_or("Clé absente")?;
            let key = kova_identity::public_key(public).ok_or("Clé invalide")?;
            let payload =
                kova_protocol::device_revocation(&identity, &hex::encode(Sha256::digest(key)));
            let signature = event["signature"].as_str().ok_or("Signature absente")?;
            if event["payload"] != payload
                || event["event_id"] != hex::encode(Sha256::digest(payload.as_bytes()))
                || !kova_identity::verify_signature(&root, &payload, signature)
            {
                return Err("Journal altéré ou signé par une autre identité".into());
            }
            proofs.push(json!({"global_identity_id":identity,"device_public_key":public,"signature":signature}));
        }
        for proof in proofs {
            self.queue_revocation(&proof)?;
        }
        Ok(())
    }
    pub fn instances(&self) -> Result<Vec<String>> {
        Ok(self
            .0
            .prepare("SELECT origin FROM instances ORDER BY origin")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }
    pub fn queue_revocation(&self, proof: &Value) -> Result<()> {
        let key =
            kova_identity::public_key(proof["device_public_key"].as_str().ok_or("Clé absente")?)
                .ok_or("Clé invalide")?;
        let id = proof["global_identity_id"]
            .as_str()
            .ok_or("Identité absente")?;
        let payload = kova_protocol::device_revocation(id, &hex::encode(Sha256::digest(key)));
        self.0.execute(
            "INSERT OR IGNORE INTO revocations VALUES(?1,?2)",
            [
                hex::encode(Sha256::digest(payload.as_bytes())),
                serde_json::to_string(proof)?,
            ],
        )?;
        Ok(())
    }
    pub fn flush_revocations(&self) -> Result<Vec<String>> {
        let proofs = self
            .0
            .prepare("SELECT proof FROM revocations ORDER BY event_id")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut failures = Vec::new();
        for origin in self.instances()? {
            for proof in &proofs {
                if Instance::new(&origin)?
                    .publish_revocation(&serde_json::from_str(proof)?)
                    .is_err()
                {
                    failures.push(origin.clone());
                    break;
                }
            }
        }
        Ok(failures)
    }
}
