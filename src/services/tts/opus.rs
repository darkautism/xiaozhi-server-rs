use crate::traits::TtsTrait;
use async_trait::async_trait;
use tracing::{info, error};
use crate::services::audio::opus_codec::OpusService;

pub struct OpusTts;

impl OpusTts {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TtsTrait for OpusTts {
    async fn speak(&self, text: &str, _emotion: Option<&str>) -> anyhow::Result<Vec<Vec<u8>>> {
        info!("Generating TTS for: '{}' (Mocking PCM -> Opus)", text);

        // 1. Generate Dummy PCM (Sine wave beep)
        // 16kHz, 1 second beep
        let sample_rate = 16000;
        let duration_secs = 2;
        let frequency = 440.0;
        let samples_count = sample_rate * duration_secs;

        let mut pcm = Vec::with_capacity(samples_count);
        for t in 0..samples_count {
            let sample = (t as f32 * frequency * 2.0 * std::f32::consts::PI / sample_rate as f32).sin();
            let sample_i16 = (sample * 10000.0) as i16;
            pcm.push(sample_i16);
        }

        // 2. Encode to Opus
        let mut encoder = OpusService::new_encoder()?;
        let mut frames = Vec::new();

        // Opus usually expects frames of 2.5, 5, 10, 20, 40, or 60 ms.
        // At 16k, 60ms = 960 samples.
        let frame_size = 960;

        for chunk in pcm.chunks(frame_size) {
            if chunk.len() < frame_size {
                 let mut padded = chunk.to_vec();
                 padded.resize(frame_size, 0);
                 let encoded = encoder.encode_vec(&padded, frame_size * 2)?;
                 frames.push(encoded);
            } else {
                 let encoded = encoder.encode_vec(chunk, frame_size * 2)?;
                 frames.push(encoded);
            }
        }

        let total_bytes: usize = frames.iter().map(|f| f.len()).sum();
        info!("Generated {} bytes of Opus audio.", total_bytes);
        Ok(frames)
    }
}
