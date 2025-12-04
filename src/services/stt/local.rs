use crate::traits::{SttTrait, SttEvent};
use async_trait::async_trait;
use tracing::info;
use futures_util::stream::BoxStream;

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
        Ok("Hello Gemini".to_string())
    }

    async fn stream_speech(
        &self,
        _input_stream: BoxStream<'static, Vec<i16>>
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<SttEvent>>> {
        Err(anyhow::anyhow!("Streaming STT not implemented for LocalStt"))
    }
}
