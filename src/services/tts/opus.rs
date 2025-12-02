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
    async fn speak(&self, text: &str) -> anyhow::Result<Vec<u8>> {
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
        let mut opus_bytes = Vec::new();

        // Opus usually expects frames of 2.5, 5, 10, 20, 40, or 60 ms.
        // At 16k, 60ms = 960 samples.
        let frame_size = 960;

        for chunk in pcm.chunks(frame_size) {
            if chunk.len() < frame_size {
                // Pad with silence if needed, or just skip partial
                 let mut padded = chunk.to_vec();
                 padded.resize(frame_size, 0);
                 let encoded = encoder.encode_vec(&padded, frame_size * 2)?; // Alloc enough space? encode_vec handles it?
                 // opus crate `encode_vec` takes input and max_output_size?
                 // Actually `encode_vec` signature: `pub fn encode_vec(&mut self, input: &[i16], max_data_bytes: usize) -> Result<Vec<u8>>`
                 opus_bytes.extend_from_slice(&encoded);
            } else {
                 let encoded = encoder.encode_vec(chunk, frame_size * 2)?;
                 opus_bytes.extend_from_slice(&encoded);
            }
        }

        info!("Generated {} bytes of Opus audio.", opus_bytes.len());
        Ok(opus_bytes)
    }
}
