use crate::traits::{SttTrait, SttEvent};
use async_trait::async_trait;
use tracing::{info, error};
use sensevoice_rs::{SenseVoiceSmall, silero_vad::VadConfig, SenseVoiceLanguage};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use futures_util::stream::{BoxStream, StreamExt};
use crate::config::VadSettings;
use async_stream::stream;
use futures_util::Stream;

pub struct SenseVoiceStt {
    shared_model: Arc<Mutex<Option<SenseVoiceSmall>>>,
    vad_settings: VadSettings,
}

impl SenseVoiceStt {
    pub fn new(vad_settings: VadSettings) -> Self {
        info!("Initializing SenseVoice STT (Shared Model)...");

        let model = match SenseVoiceSmall::init(VadConfig::default()) {
            Ok(m) => Some(m),
            Err(e) => {
                error!("Failed to initialize SenseVoice model: {}", e);
                None
            }
        };

        Self {
            shared_model: Arc::new(Mutex::new(model)),
            vad_settings,
        }
    }

    // Used internally if we needed to create instance in same thread
    #[allow(dead_code)]
    fn create_stream_instance(&self) -> anyhow::Result<SenseVoiceSmall> {
        let mut model = SenseVoiceSmall::init(VadConfig::default())
            .map_err(|e| anyhow::anyhow!("Failed to init SenseVoice for stream: {}", e))?;
        model.set_vad_silence_notification(Some(self.vad_settings.silence_duration_ms));
        Ok(model)
    }
}

#[async_trait]
impl SttTrait for SenseVoiceStt {
    async fn recognize(&self, audio_pcm: &[u8]) -> anyhow::Result<String> {
        let start = Instant::now();
        if audio_pcm.len() % 2 != 0 {
             return Err(anyhow::anyhow!("Invalid PCM byte length"));
        }
        let pcm_i16: Vec<i16> = audio_pcm.chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        let mut model_guard = self.shared_model.lock().map_err(|_| anyhow::anyhow!("Poisoned lock"))?;

        if let Some(sv) = model_guard.as_mut() {
             let result = sv.infer_vec(pcm_i16, 16000)
                 .map_err(|e| anyhow::anyhow!("Inference failed: {}", e))?;
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

    async fn stream_speech(
        &self,
        input_stream: BoxStream<'static, Vec<i16>>
    ) -> anyhow::Result<BoxStream<'static, anyhow::Result<SttEvent>>> {
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let vad_settings = self.vad_settings.clone();

        // Spawn a dedicated thread to handle the !Send stream from sensevoice-rs
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();

            if let Ok(rt) = rt {
                rt.block_on(async move {
                    let model_init = SenseVoiceSmall::init(VadConfig::default());
                    if let Ok(mut model) = model_init {
                        model.set_vad_silence_notification(Some(vad_settings.silence_duration_ms));

                        // Ensure input is chunked to 512 samples as required by sensevoice-rs infer_stream
                        let chunked_input = chunk_stream(input_stream, 512);
                        // chunk_stream returns !Unpin stream, but infer_stream requires Unpin.
                        let pinned_input = Box::pin(chunked_input);
                        let stream = model.infer_stream(pinned_input);

                        let mut pinned_stream = Box::pin(stream);

                        while let Some(result) = pinned_stream.next().await {
                            let event = match result {
                                Ok(vt) => {
                                     match vt.language {
                                         SenseVoiceLanguage::NoSpeech => Ok(SttEvent::NoSpeech),
                                         _ => {
                                             if !vt.content.is_empty() {
                                                 Ok(SttEvent::Text(vt.content))
                                             } else {
                                                 continue;
                                             }
                                         }
                                     }
                                },
                                Err(e) => Err(anyhow::anyhow!("SenseVoice Stream Error: {}", e.to_string())),
                            };

                            if tx.send(event).await.is_err() {
                                break;
                            }
                        }
                    } else if let Err(e) = model_init {
                        let _ = tx.send(Err(anyhow::anyhow!("Failed to init model: {}", e))).await;
                    }
                });
            } else {
                 error!("Failed to create runtime for STT thread");
            }
        });

        let output_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(output_stream))
    }
}

// Helper to chunk the stream into fixed size vectors
fn chunk_stream<S>(mut input: S, chunk_size: usize) -> impl Stream<Item = Vec<i16>>
where S: Stream<Item = Vec<i16>> + Unpin {
    stream! {
        let mut buffer: Vec<i16> = Vec::new();
        while let Some(chunk) = input.next().await {
            buffer.extend(chunk);
            while buffer.len() >= chunk_size {
                let send = buffer.drain(0..chunk_size).collect::<Vec<i16>>();
                yield send;
            }
        }
        if !buffer.is_empty() {
            buffer.resize(chunk_size, 0);
            yield buffer;
        }
    }
}
