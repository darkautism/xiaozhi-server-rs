use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, Duration};
use crate::config::ServerConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub db: Arc<RwLock<InMemoryDb>>,
}

pub struct InMemoryDb {
    pub activated_devices: HashSet<String>, // DeviceId
    pub pending_challenges: HashMap<String, (String, SystemTime)>, // DeviceId -> (Challenge, Expiry)
}

impl InMemoryDb {
    pub fn new() -> Self {
        Self {
            activated_devices: HashSet::new(),
            pending_challenges: HashMap::new(),
        }
    }

    pub fn is_activated(&self, device_id: &str) -> bool {
        self.activated_devices.contains(device_id)
    }

    pub fn add_challenge(&mut self, device_id: String, challenge: String, ttl: Duration) {
        self.pending_challenges.insert(device_id, (challenge, SystemTime::now() + ttl));
    }

    pub fn get_challenge(&self, device_id: &str) -> Option<String> {
        if let Some((challenge, expiry)) = self.pending_challenges.get(device_id) {
            if SystemTime::now() < *expiry {
                return Some(challenge.clone());
            }
        }
        None
    }

    pub fn activate_device(&mut self, device_id: String) {
        self.activated_devices.insert(device_id.clone());
        self.pending_challenges.remove(&self.device_id_key(&device_id));
    }

    // Helper to allow finding by ref
    fn device_id_key(&self, _id: &str) -> String {
        // Just return the id itself, mainly to match types
        _id.to_string()
    }
}
