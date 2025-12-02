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
    async fn speak(&self, text: &str) -> anyhow::Result<Vec<Vec<u8>>> {
        info!("TODO: Implement Text-to-Speech and Opus encoding for text: '{}'", text);
        // Mock response: valid Opus frame header or silence?
        // We will return a small dummy buffer.
        Ok(vec![vec![0; 10]])
    }
}
