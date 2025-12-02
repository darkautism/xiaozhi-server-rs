use crate::traits::TtsTrait;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tracing::{info, error};
use anyhow::Context;
use crate::services::audio::opus_codec::OpusService;

pub struct GeminiTts {
    api_key: String,
    client: Client,
    model: String, // e.g. "gemini-2.5-flash-preview-tts"
    voice_name: String, // e.g. "Kore"
}

impl GeminiTts {
    pub fn new(api_key: String, model: String, voice_name: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
            model,
            voice_name,
        }
    }

    // Simple resampler using linear interpolation (24kHz -> 16kHz)
    fn resample_24k_to_16k(input: &[i16]) -> Vec<i16> {
        let input_len = input.len();
        // Ratio is 16/24 = 2/3.
        let output_len = (input_len * 2) / 3;
        let mut output = Vec::with_capacity(output_len);

        for i in 0..output_len {
            // Calculate position in input
            // out_index * 24 / 16 = out_index * 1.5
            let pos = i as f32 * 1.5;
            let index = pos.floor() as usize;
            let frac = pos - index as f32;

            if index + 1 < input_len {
                let s0 = input[index] as f32;
                let s1 = input[index + 1] as f32;
                let val = s0 + (s1 - s0) * frac;
                output.push(val as i16);
            } else if index < input_len {
                output.push(input[index]);
            }
        }
        output
    }
}

#[async_trait]
impl TtsTrait for GeminiTts {
    async fn speak(&self, text: &str) -> anyhow::Result<Vec<Vec<u8>>> {
        info!("Generating Gemini TTS for: '{}' using voice '{}'", text, self.voice_name);

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let body = json!({
            "contents": [{
                "parts": [{
                    "text": text
                }]
            }],
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "speechConfig": {
                    "voiceConfig": {
                        "prebuiltVoiceConfig": {
                            "voiceName": self.voice_name
                        }
                    }
                }
            }
        });

        let resp = self.client.post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Gemini TTS")?;

        if !resp.status().is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            error!("Gemini TTS API error: {}", error_text);
            return Err(anyhow::anyhow!("Gemini TTS API error: {}", error_text));
        }

        let json_resp: serde_json::Value = resp.json().await.context("Failed to parse Gemini TTS response")?;

        // Extract base64 audio data
        let encoded_audio = json_resp["candidates"][0]["content"]["parts"][0]["inlineData"]["data"]
            .as_str()
            .context("Invalid response format from Gemini TTS (missing inlineData.data)")?;

        use base64::{Engine as _, engine::general_purpose};
        let audio_data = general_purpose::STANDARD
            .decode(encoded_audio)
            .context("Failed to decode base64 audio data")?;

        info!("Received {} bytes of raw audio from Gemini TTS", audio_data.len());

        // Convert u8 bytes to i16 PCM (assuming Little Endian)
        let mut pcm_i16: Vec<i16> = Vec::with_capacity(audio_data.len() / 2);
        for chunk in audio_data.chunks(2) {
            if chunk.len() == 2 {
                pcm_i16.push(i16::from_le_bytes([chunk[0], chunk[1]]));
            }
        }

        // Resample 24k -> 16k
        let resampled_pcm = Self::resample_24k_to_16k(&pcm_i16);

        // Encode to Opus
        // Opus Encoder expects frames. Frame size for 16kHz:
        // 2.5ms, 5ms, 10ms, 20ms, 40ms, 60ms.
        // 60ms at 16k = 960 samples.
        // We will chunk the PCM data into 960-sample chunks and encode each.

        let mut encoder = OpusService::new_encoder()?;
        let mut frames = Vec::new();
        let frame_size = 960; // 60ms

        for chunk in resampled_pcm.chunks(frame_size) {
            let encoded = if chunk.len() < frame_size {
                let mut padded = chunk.to_vec();
                padded.resize(frame_size, 0);
                encoder.encode_vec(&padded, frame_size * 2)?
            } else {
                encoder.encode_vec(chunk, frame_size * 2)?
            };
            frames.push(encoded);
        }

        let total_bytes: usize = frames.iter().map(|f| f.len()).sum();
        info!("Encoded {} bytes of Opus audio into {} frames.", total_bytes, frames.len());
        Ok(frames)
    }
}
