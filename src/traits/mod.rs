use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use futures_util::stream::BoxStream;

#[derive(Debug, Clone)]
pub enum SttEvent {
    Text(String),
    NoSpeech,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String, // "user" or "model"
    pub content: String,
}

#[async_trait]
pub trait LlmTrait: Send + Sync {
    async fn chat(&self, messages: Vec<Message>) -> anyhow::Result<String>;
}

#[async_trait]
pub trait SttTrait: Send + Sync {
    // In a real implementation, this might take a stream or a complete buffer
    // For this protocol, we might feed it chunks.
    // Simplified for now: assume we might process a chunk or a whole buffer.
    async fn recognize(&self, audio: &[u8]) -> anyhow::Result<String>;

    // New streaming method
    async fn stream_speech(
        &self,
        input_stream: BoxStream<'static, Vec<i16>>
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<SttEvent>>>;
}

#[async_trait]
pub trait TtsTrait: Send + Sync {
    // Returns a list of Opus encoded frames (each frame is a Vec<u8>)
    // emotion: Optional emotion string extracted from text (e.g. "happy", "sad")
    async fn speak(&self, text: &str, emotion: Option<&str>) -> anyhow::Result<Vec<Vec<u8>>>;
}

#[async_trait]
pub trait DbTrait: Send + Sync {
    async fn is_activated(&self, device_id: &str) -> anyhow::Result<bool>;
    async fn activate_device(&self, device_id: &str) -> anyhow::Result<()>;
    async fn add_challenge(&self, device_id: &str, challenge: &str, ttl_secs: u64) -> anyhow::Result<()>;
    async fn get_challenge(&self, device_id: &str) -> anyhow::Result<Option<String>>;

    async fn add_chat_history(&self, device_id: &str, role: &str, content: &str) -> anyhow::Result<()>;
    async fn get_chat_history(&self, device_id: &str, limit: usize) -> anyhow::Result<Vec<Message>>;
}
