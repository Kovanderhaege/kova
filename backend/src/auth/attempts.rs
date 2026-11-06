use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};

// Fenêtre fixe partagée, remise à zéro en O(1) par tentative entre deux fenêtres.
pub(super) struct Attempts {
    since: Instant,
    identities: HashMap<String, u8>,
    addresses: HashMap<IpAddr, u8>,
}

impl Attempts {
    pub fn new() -> Self {
        Self {
            since: Instant::now(),
            identities: HashMap::new(),
            addresses: HashMap::new(),
        }
    }

    pub fn allow(&mut self, ip: IpAddr, identity: Option<&str>) -> bool {
        if self.since.elapsed() >= Duration::from_secs(60) {
            *self = Self::new();
        }
        if self.addresses.len() >= 8192 && !self.addresses.contains_key(&ip) {
            return false;
        }
        let address = self.addresses.entry(ip).or_default();
        if *address >= 30 {
            return false;
        }
        *address += 1;
        let Some(identity) = identity else {
            return true;
        };
        if self.identities.len() >= 8192 && !self.identities.contains_key(identity) {
            return false;
        }
        let account = self.identities.entry(identity.to_owned()).or_default();
        if *account >= 10 {
            return false;
        }
        *account += 1;
        true
    }
}
