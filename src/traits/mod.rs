use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait LlmTrait: Send + Sync {
    async fn chat(&self, text: &str) -> anyhow::Result<String>;
}

#[async_trait]
pub trait SttTrait: Send + Sync {
    // In a real implementation, this might take a stream or a complete buffer
    // For this protocol, we might feed it chunks.
    // Simplified for now: assume we might process a chunk or a whole buffer.
    async fn recognize(&self, audio: &[u8]) -> anyhow::Result<String>;
}

#[async_trait]
pub trait TtsTrait: Send + Sync {
    // Returns Opus encoded bytes
    async fn speak(&self, text: &str) -> anyhow::Result<Vec<u8>>;
}

#[async_trait]
pub trait DbTrait: Send + Sync {
    async fn is_activated(&self, device_id: &str) -> anyhow::Result<bool>;
    async fn activate_device(&self, device_id: &str) -> anyhow::Result<()>;
    async fn add_challenge(&self, device_id: &str, challenge: &str, ttl_secs: u64) -> anyhow::Result<()>;
    async fn get_challenge(&self, device_id: &str) -> anyhow::Result<Option<String>>;
}
