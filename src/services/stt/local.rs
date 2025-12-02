use crate::traits::SttTrait;
use async_trait::async_trait;
use tracing::info;

pub struct LocalStt;

impl LocalStt {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SttTrait for LocalStt {
    async fn recognize(&self, audio: &[u8]) -> anyhow::Result<String> {
        info!("TODO: Implement Opus decoding and Whisper STT. Received {} bytes.", audio.len());
        // Mock response
        // In a real loop, we would accumulate audio and eventually return text.
        // For this mock, we assume the caller handles accumulation and trigger,
        // or we just return a fixed string to prove the pipeline works.
        Ok("Hello Gemini".to_string())
    }
}
