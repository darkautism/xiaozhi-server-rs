use anyhow::{Context, Result};
use opus::{Application, Channels, Decoder, Encoder};

// OpusService handles encoding and decoding.
// Since opus Encoders/Decoders are not thread-safe by default, we wrap them if needed,
// but usually we create one per session or per stream.
// Here we provide a helper to easily create them or manage them.

pub struct OpusService;

impl OpusService {
    pub fn decode(audio: &[u8]) -> Result<Vec<i16>> {
        // Create a new decoder for each chunk? No, that breaks state (packet loss concealment etc).
        // But for a simple "decode this full buffer" helper, it implies the buffer is a complete session?
        // Actually, the caller (WebSocket loop) should maintain the Decoder state.
        // This struct might just be a factory or utility wrapper.

        // However, if we receive a single large buffer of concatenated frames (not standard Opus),
        // we can't easily decode it without framing info.
        // The protocol says "Client sends binary frames (Opus data)".
        // Usually 60ms frames.

        // For this implementation, let's assume the input `audio` is EXACTLY ONE Opus packet
        // because that's how WebSocket messages usually arrive (one frame per message).

        let mut decoder = Decoder::new(16000, Channels::Mono)?;
        let mut output = vec![0i16; 5760]; // Max frame size for 120ms at 48k, here at 16k 60ms is 960 samples.
                                           // Let's alloc enough.

        let len = decoder.decode(audio, &mut output, false)?;
        output.truncate(len);
        Ok(output)
    }

    // Helper to create a stateful decoder
    pub fn new_decoder() -> Result<Decoder> {
        Decoder::new(16000, Channels::Mono).context("Failed to create Opus decoder")
    }

    pub fn new_encoder() -> Result<Encoder> {
        Encoder::new(16000, Channels::Mono, Application::Voip)
            .context("Failed to create Opus encoder")
    }
}
