use crate::traits::DbTrait;
use async_trait::async_trait;

// TODO: User has the option to switch to SQL.
// In Settings.toml, set db.type = "sql"
// This module currently is a stub. To implement:
// 1. Add `sqlx` or `rusqlite` to Cargo.toml.
// 2. Define the schema (e.g., CREATE TABLE devices (id TEXT PRIMARY KEY, ...))
// 3. Implement the DbTrait methods using SQL queries.

pub struct SqlDb;

impl SqlDb {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DbTrait for SqlDb {
    async fn is_activated(&self, _device_id: &str) -> anyhow::Result<bool> {
        // TODO: Implement SQL query
        // SELECT count(*) FROM activated_devices WHERE device_id = ?
        unimplemented!("SQL DB not yet implemented. Please implement connection and query logic.");
    }

    async fn activate_device(&self, _device_id: &str) -> anyhow::Result<()> {
        // TODO: Implement SQL insert
        // INSERT INTO activated_devices (device_id) VALUES (?)
        unimplemented!("SQL DB not yet implemented.");
    }

    async fn add_challenge(&self, _device_id: &str, _challenge: &str, _ttl_secs: u64) -> anyhow::Result<()> {
        // TODO: Implement SQL insert
        // INSERT INTO challenges (device_id, challenge, expiry) VALUES (?, ?, ?)
        unimplemented!("SQL DB not yet implemented.");
    }

    async fn get_challenge(&self, _device_id: &str) -> anyhow::Result<Option<String>> {
        // TODO: Implement SQL query
        // SELECT challenge FROM challenges WHERE device_id = ? AND expiry > NOW()
        unimplemented!("SQL DB not yet implemented.");
    }
}
