use crate::traits::TtsTrait;
use async_trait::async_trait;
use tracing::info;

pub struct LocalTts;

impl LocalTts {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TtsTrait for LocalTts {
    async fn speak(&self, text: &str) -> anyhow::Result<Vec<u8>> {
        info!("TODO: Implement Text-to-Speech and Opus encoding for text: '{}'", text);
        // Mock response: valid Opus frame header or silence?
        // A minimal Opus frame is complex to hand-craft without the lib.
        // We will return a small dummy buffer. The client might complain if it tries to decode this garbage,
        // but it satisfies the server loop.

        // A dummy 1-byte payload is technically invalid but enough to send "something".
        // Real Opus packets usually start with TOC byte.
        Ok(vec![0; 10])
    }
}
