use async_trait::async_trait;
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub enum SttEvent {
    Text(String),
    NoSpeech,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub type_: String, // "function"
    pub function: ToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String, // JSON string
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String, // "user" or "model" or "assistant" or "tool"
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum ChatResponse {
    Text(String),
    ToolCall(Vec<ToolCall>),
}

#[async_trait]
pub trait LlmTrait: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> anyhow::Result<ChatResponse>;
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
        input_stream: BoxStream<'static, Vec<i16>>,
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
    async fn add_challenge(
        &self,
        device_id: &str,
        challenge: &str,
        ttl_secs: u64,
    ) -> anyhow::Result<()>;
    async fn get_challenge(&self, device_id: &str) -> anyhow::Result<Option<String>>;

    async fn add_chat_history(
        &self,
        device_id: &str,
        role: &str,
        content: &str,
    ) -> anyhow::Result<()>;
    async fn get_chat_history(&self, device_id: &str, limit: usize)
        -> anyhow::Result<Vec<Message>>;
}
