use crate::traits::DbTrait;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::time::{SystemTime, Duration};

pub struct InMemoryDb {
    activated_devices: RwLock<HashSet<String>>,
    pending_challenges: RwLock<HashMap<String, (String, SystemTime)>>, // DeviceId -> (Challenge, Expiry)
}

impl InMemoryDb {
    pub fn new() -> Self {
        Self {
            activated_devices: RwLock::new(HashSet::new()),
            pending_challenges: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl DbTrait for InMemoryDb {
    async fn is_activated(&self, device_id: &str) -> anyhow::Result<bool> {
        let db = self.activated_devices.read().unwrap();
        Ok(db.contains(device_id))
    }

    async fn activate_device(&self, device_id: &str) -> anyhow::Result<()> {
        let mut db = self.activated_devices.write().unwrap();
        db.insert(device_id.to_string());
        // Also remove any pending challenge
        let mut challenges = self.pending_challenges.write().unwrap();
        challenges.remove(device_id);
        Ok(())
    }

    async fn add_challenge(&self, device_id: &str, challenge: &str, ttl_secs: u64) -> anyhow::Result<()> {
        let mut challenges = self.pending_challenges.write().unwrap();
        challenges.insert(
            device_id.to_string(),
            (challenge.to_string(), SystemTime::now() + Duration::from_secs(ttl_secs))
        );
        Ok(())
    }

    async fn get_challenge(&self, device_id: &str) -> anyhow::Result<Option<String>> {
        let challenges = self.pending_challenges.read().unwrap();
        if let Some((challenge, expiry)) = challenges.get(device_id) {
            if SystemTime::now() < *expiry {
                return Ok(Some(challenge.clone()));
            }
        }
        Ok(None)
    }
}
