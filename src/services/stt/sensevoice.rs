use crate::traits::SttTrait;
use async_trait::async_trait;
use tracing::{info, error};
use sensevoice_rs::{SenseVoiceSmall, silero_vad::VadConfig};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct SenseVoiceStt {
    model: Arc<Mutex<Option<SenseVoiceSmall>>>,
}

impl SenseVoiceStt {
    pub fn new() -> Self {
        info!("Initializing SenseVoice STT...");

        let model = match SenseVoiceSmall::init(VadConfig::default()) {
            Ok(m) => Some(m),
            Err(e) => {
                error!("Failed to initialize SenseVoice model: {}", e);
                None
            }
        };

        Self {
            model: Arc::new(Mutex::new(model)),
        }
    }
}

#[async_trait]
impl SttTrait for SenseVoiceStt {
    async fn recognize(&self, audio_pcm: &[u8]) -> anyhow::Result<String> {
        // audio_pcm is raw bytes of i16 little endian PCM (already decoded from Opus)

        let start = Instant::now();

        // Safety: ensure length is even.
        if audio_pcm.len() % 2 != 0 {
             return Err(anyhow::anyhow!("Invalid PCM byte length"));
        }

        // Convert [u8] -> [i16]
        let pcm_i16: Vec<i16> = audio_pcm.chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        // Lock model
        let mut model_guard = self.model.lock().map_err(|_| anyhow::anyhow!("Poisoned lock"))?;

        if let Some(sv) = model_guard.as_mut() {
             // infer_vec expects Vec<i16>
             // Use 16000 as sample rate (SenseVoice standard)
             let result = sv.infer_vec(pcm_i16, 16000)
                 .map_err(|e| anyhow::anyhow!("Inference failed: {}", e))?;

             // Result is Vec<VoiceText>
             let text = result.into_iter()
                 .map(|vt| vt.content)
                 .collect::<Vec<String>>()
                 .join(" ");

             info!("STT complete in {:?}. Text: {}", start.elapsed(), text);
             Ok(text)
        } else {
             Err(anyhow::anyhow!("SenseVoice model not initialized"))
        }
    }
}
