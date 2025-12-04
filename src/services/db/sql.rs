use crate::traits::{DbTrait, Message};
use async_trait::async_trait;
use sqlx::sqlite::SqlitePool;
use sqlx::{Pool, Row, Sqlite};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct SqlDb {
    pool: Pool<Sqlite>,
}

impl SqlDb {
    pub async fn new(url: &str) -> anyhow::Result<Self> {
        let pool = SqlitePool::connect(url).await?;
        let db = Self { pool };
        db.init().await?;
        Ok(db)
    }

    async fn init(&self) -> anyhow::Result<()> {
        // Create tables if not exist
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS activated_devices (
                device_id TEXT PRIMARY KEY
            );
            CREATE TABLE IF NOT EXISTS challenges (
                device_id TEXT PRIMARY KEY,
                challenge TEXT NOT NULL,
                expiry INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS chat_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_chat_history_device_id ON chat_history(device_id);
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl DbTrait for SqlDb {
    async fn is_activated(&self, device_id: &str) -> anyhow::Result<bool> {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM activated_devices WHERE device_id = ?")
                .bind(device_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(count > 0)
    }

    async fn activate_device(&self, device_id: &str) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("INSERT OR IGNORE INTO activated_devices (device_id) VALUES (?)")
            .bind(device_id)
            .execute(&mut *tx)
            .await?;

        // Remove pending challenge
        sqlx::query("DELETE FROM challenges WHERE device_id = ?")
            .bind(device_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn add_challenge(
        &self,
        device_id: &str,
        challenge: &str,
        ttl_secs: u64,
    ) -> anyhow::Result<()> {
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + ttl_secs;

        sqlx::query(
            "INSERT OR REPLACE INTO challenges (device_id, challenge, expiry) VALUES (?, ?, ?)",
        )
        .bind(device_id)
        .bind(challenge)
        .bind(expiry as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_challenge(&self, device_id: &str) -> anyhow::Result<Option<String>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let result: Option<(String, i64)> =
            sqlx::query_as("SELECT challenge, expiry FROM challenges WHERE device_id = ?")
                .bind(device_id)
                .fetch_optional(&self.pool)
                .await?;

        if let Some((challenge, expiry)) = result {
            if now < expiry {
                return Ok(Some(challenge));
            } else {
                let _ = sqlx::query("DELETE FROM challenges WHERE device_id = ?")
                    .bind(device_id)
                    .execute(&self.pool)
                    .await;
            }
        }
        Ok(None)
    }

    async fn add_chat_history(
        &self,
        device_id: &str,
        role: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        sqlx::query(
            "INSERT INTO chat_history (device_id, role, content, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(device_id)
        .bind(role)
        .bind(content)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_chat_history(
        &self,
        device_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<Message>> {
        // Fetch recent messages. Since we need them in chronological order,
        // we sort by created_at DESC, limit, then reverse.
        // Or using subquery.
        let rows = sqlx::query(
            r#"
            SELECT role, content FROM (
                SELECT role, content, created_at
                FROM chat_history
                WHERE device_id = ?
                ORDER BY created_at DESC, id DESC
                LIMIT ?
            ) ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(device_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(Message {
                role: row.get("role"),
                content: row.get("content"),
            });
        }
        Ok(messages)
    }
}
