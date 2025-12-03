use crate::traits::TtsTrait;
use async_trait::async_trait;
use tracing::info;
use anyhow::Context;
use msedge_tts::tts::client::connect_async;
use msedge_tts::tts::SpeechConfig;
use crate::services::audio::opus_codec::OpusService;
use crate::services::audio::resampler::resample_24k_to_16k;

pub struct EdgeTts {
    voice: String,
    rate: String,
    pitch: String,
    volume: String,
}

impl EdgeTts {
    pub fn new(voice: String, rate: String, pitch: String, volume: String) -> Self {
        Self {
            voice,
            rate,
            pitch,
            volume,
        }
    }
}

#[async_trait]
impl TtsTrait for EdgeTts {
    async fn speak(&self, text: &str) -> anyhow::Result<Vec<Vec<u8>>> {
        info!("Generating Edge TTS for: '{}' using voice '{}'", text, self.voice);

        // Connect to Edge TTS
        let mut client = connect_async().await
            .context("Failed to connect to Edge TTS service")?;

        let pitch = self.pitch.trim_matches(|c: char| !c.is_numeric() && c != '-').parse::<i32>().unwrap_or(0);
        let rate = self.rate.trim_matches(|c: char| !c.is_numeric() && c != '-').parse::<i32>().unwrap_or(0);
        let volume = self.volume.trim_matches(|c: char| !c.is_numeric() && c != '-').parse::<i32>().unwrap_or(0);

        let config = SpeechConfig {
             voice_name: self.voice.clone(),
             pitch,
             rate,
             volume,
             audio_format: "audio-24khz-48kbitrate-mono-mp3".to_string(),
        };

        // Synthesize
        let audio_metadata = client.synthesize(text, &config).await
            .context("Failed to synthesize speech via Edge TTS")?;

        let audio_data = audio_metadata.audio_bytes;

        info!("Received {} bytes of MP3 audio from Edge TTS", audio_data.len());

        let mut decoder = minimp3::Decoder::new(&audio_data[..]);
        let mut pcm_i16 = Vec::new();
        let mut sample_rate = 0;

        loop {
            match decoder.next_frame() {
                Ok(frame) => {
                     if sample_rate == 0 {
                         sample_rate = frame.sample_rate;
                         info!("Edge TTS MP3 details: sample_rate={}, channels={}, layer={}, bitrate={}",
                               frame.sample_rate, frame.channels, frame.layer, frame.bitrate);
                     }

                     if frame.channels == 1 {
                         pcm_i16.extend_from_slice(&frame.data);
                     } else {
                         // Stereo to Mono: simple downsample (take left channel)
                         for chunk in frame.data.chunks(frame.channels) {
                             if let Some(&sample) = chunk.first() {
                                 pcm_i16.push(sample);
                             }
                         }
                     }
                },
                Err(minimp3::Error::Eof) => break,
                Err(e) => return Err(anyhow::anyhow!("MP3 decode error: {:?}", e)),
            }
        }

        // Resample based on actual sample rate
        let resampled_pcm = if sample_rate == 16000 {
            info!("Sample rate is 16kHz, skipping resampling.");
            pcm_i16
        } else if sample_rate == 24000 {
            info!("Sample rate is 24kHz, resampling to 16kHz.");
            resample_24k_to_16k(&pcm_i16)
        } else {
            info!("Sample rate is {}, attempting 24k->16k resampling (may be incorrect).", sample_rate);
            resample_24k_to_16k(&pcm_i16)
        };

        // Encode to Opus
        let mut encoder = OpusService::new_encoder()?;
        let mut frames = Vec::new();
        let frame_size = 960; // 60ms at 16kHz

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
