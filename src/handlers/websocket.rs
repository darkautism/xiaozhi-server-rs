use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State, ConnectInfo},
    http::HeaderMap,
    response::IntoResponse,
};
use std::net::SocketAddr;
use futures_util::{stream::StreamExt, SinkExt};
use serde_json::{json, Value};
use tracing::{info, warn, error, debug};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

use crate::state::AppState;
use crate::services::audio::opus_codec::OpusService;
use voice_activity_detector::VoiceActivityDetector;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct AudioParams {
    pub format: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub frame_duration: u32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    Hello {
        version: u32,
        transport: String,
        audio_params: AudioParams,
        #[serde(default)]
        features: Option<Value>,
    },
    Listen {
        session_id: String,
        state: String,
        #[serde(default)]
        mode: Option<String>,
        #[serde(default)]
        text: Option<String>,
    },
    Abort {
        session_id: String,
        reason: String,
    },
    Iot {
        session_id: String,
        #[serde(default)]
        descriptors: Option<Value>,
        #[serde(default)]
        states: Option<Value>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AudioParamsResponse {
    pub sample_rate: u32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ServerMessage {
    Hello {
        transport: String,
        #[serde(default)]
        audio_params: Option<AudioParamsResponse>,
    },
    Stt {
        text: String,
    },
    Tts {
        state: String, // start, stop, sentence_start
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
    },
    Llm {
        #[serde(default)]
        emotion: Option<String>,
        #[serde(default)]
        text: Option<String>,
    },
    Iot {
        commands: Vec<Value>,
    }
}

pub async fn handle_websocket(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    info!("WebSocket handshake attempt from {}", addr);
    debug!("WebSocket handshake headers: {:?}", headers);

    ws.on_upgrade(move |socket| handle_socket(socket, addr, state))
}

// Helper to trigger processing pipeline
async fn trigger_pipeline(
    state: &AppState,
    tx: &Sender<Message>,
    pcm_buffer: &[u8],
) {
     if pcm_buffer.is_empty() {
         warn!("No audio received for STT");
         return;
     }

     // 1. Trigger STT processing
     match state.stt.recognize(pcm_buffer).await {
         Ok(text) => {
             info!("STT Result: {}", text);
             if text.trim().is_empty() {
                 info!("Empty STT result, ignoring.");
                 return;
             }

             let stt_msg = ServerMessage::Stt { text: text.clone() };
             let _ = tx.send(Message::Text(serde_json::to_string(&stt_msg).unwrap().into())).await;

             // 2. Call LLM
             match state.llm.chat(&text).await {
                 Ok(response_text) => {
                     info!("LLM Response: {}", response_text);
                     let llm_msg = ServerMessage::Llm { emotion: Some("happy".to_string()), text: Some(response_text.clone()) };
                     let _ = tx.send(Message::Text(serde_json::to_string(&llm_msg).unwrap().into())).await;

                     // 3. TTS
                     let tts_start = ServerMessage::Tts { state: "start".to_string(), text: None };
                     let _ = tx.send(Message::Text(serde_json::to_string(&tts_start).unwrap().into())).await;

                     match state.tts.speak(&response_text).await {
                         Ok(audio_bytes) => {
                             // Note: Sending one huge binary blob might overwhelm the client/buffer?
                             // Usually we should stream it.
                             // But `speak` returns full Vec<u8> currently.
                             let _ = tx.send(Message::Binary(audio_bytes.into())).await;
                         }
                         Err(e) => error!("TTS Error: {}", e),
                     }

                     let tts_stop = ServerMessage::Tts { state: "stop".to_string(), text: None };
                     let _ = tx.send(Message::Text(serde_json::to_string(&tts_stop).unwrap().into())).await;
                 }
                 Err(e) => {
                     error!("LLM Error: {}", e);
                 }
             }
         }
         Err(e) => {
             error!("STT Error: {}", e);
         }
     }
}

async fn handle_socket(mut socket: WebSocket, addr: SocketAddr, state: AppState) {
    info!("WebSocket connection established with {}", addr);

    let mut current_session_id = String::new();
    let mut is_listening = false;
    // Buffer for collecting PCM samples for STT
    let mut pcm_buffer: Vec<u8> = Vec::new();

    // VAD State
    let mut vad = VoiceActivityDetector::builder()
        .sample_rate(16000)
        .chunk_size(512usize) // 32ms at 16k
        .build()
        .expect("Failed to init VAD");

    let mut silence_chunks = 0;
    let silence_threshold = 20; // 20 * 32ms ~= 640ms silence
    let mut has_spoken = false;
    // Accumulate PCM for VAD processing (needs f32 usually, but this crate might take something else, let's check)
    // The crate VoiceActivityDetector typically takes `&[f32]`.
    // We need to buffer incoming PCM (i16) until we have `chunk_size` samples for VAD.
    let mut vad_accumulator: Vec<f32> = Vec::new();
    let vad_chunk_size = 512;

    // Create Opus Decoder for this session
    let mut opus_decoder = match OpusService::new_decoder() {
        Ok(d) => Some(d),
        Err(e) => {
            error!("Failed to create Opus decoder: {}", e);
            None
        }
    };

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(32);

    let mut writer_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(e) = sender.send(msg).await {
                error!("Error sending message: {}", e);
                break;
            }
        }
    });

    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(e) => {
                error!("Error receiving message from {}: {}", addr, e);
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                info!("Received text message from {}: {}", addr, text);
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(client_message) => {
                         match client_message {
                             ClientMessage::Hello { .. } => {
                                 info!("Processing Hello message from {}", addr);
                                 let response = ServerMessage::Hello {
                                     transport: "websocket".to_string(),
                                     audio_params: Some(AudioParamsResponse { sample_rate: 16000 }),
                                 };
                                 let response_text = serde_json::to_string(&response).unwrap();
                                 let _ = tx.send(Message::Text(response_text.into())).await;
                             },
                             ClientMessage::Listen { session_id, state: listen_state, .. } => {
                                 current_session_id = session_id.clone();
                                 if listen_state == "start" {
                                     info!("Client started listening. Session: {}", current_session_id);
                                     is_listening = true;
                                     pcm_buffer.clear();
                                     vad_accumulator.clear();
                                     has_spoken = false;
                                     silence_chunks = 0;
                                     // Reset decoder state if possible or needed?
                                 } else if listen_state == "stop" {
                                     info!("Client stopped listening. Session: {}", current_session_id);
                                     is_listening = false;
                                     trigger_pipeline(&state, &tx, &pcm_buffer).await;
                                     pcm_buffer.clear();
                                 }
                             },
                             ClientMessage::Abort { reason, .. } => {
                                 info!("Client aborted: {}", reason);
                                 is_listening = false;
                                 pcm_buffer.clear();
                             },
                             ClientMessage::Iot { .. } => {
                                 info!("Received IoT message");
                             }
                         }
                    }
                    Err(e) => {
                        error!("Error deserializing message from {}: {}", addr, e);
                    }
                }
            }
            Message::Binary(bin) => {
                if is_listening {
                    if let Some(decoder) = opus_decoder.as_mut() {
                        let mut output = vec![0i16; 5760]; // Max buffer
                        match decoder.decode(&bin, &mut output, false) {
                            Ok(len) => {
                                // Convert i16 samples to bytes (little endian)
                                for sample in &output[..len] {
                                    let s = *sample;
                                    pcm_buffer.extend_from_slice(&s.to_le_bytes());

                                    // VAD Processing
                                    // Convert to f32
                                    vad_accumulator.push(s as f32 / 32768.0);

                                    while vad_accumulator.len() >= vad_chunk_size {
                                        let chunk: Vec<f32> = vad_accumulator.drain(0..vad_chunk_size).collect();
                                        // VAD predict
                                        let probability = vad.predict(chunk);

                                        if probability > 0.5 {
                                            has_spoken = true;
                                            silence_chunks = 0;
                                        } else {
                                            if has_spoken {
                                                silence_chunks += 1;
                                            }
                                        }

                                        // Check trigger
                                        if has_spoken && silence_chunks > silence_threshold {
                                            info!("VAD: Silence detected after speech. Triggering pipeline.");
                                            // Stop listening logic
                                            is_listening = false; // Stop accepting new audio for this turn
                                            trigger_pipeline(&state, &tx, &pcm_buffer).await;
                                            pcm_buffer.clear();
                                            vad_accumulator.clear();
                                            has_spoken = false;
                                            silence_chunks = 0;

                                            // TODO: Should we inform client we stopped listening?
                                            // The client might expect a "tts: start" message to switch state, which trigger_pipeline sends.
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Opus decode error: {}", e);
                            }
                        }
                    }
                }
            }
            Message::Ping(_) => {
                let _ = tx.send(Message::Pong(vec![].into())).await;
            }
            Message::Pong(_) => {
            }
            Message::Close(_) => {
                info!("Connection closed by {}", addr);
                break;
            }
        }
    }

    writer_handle.abort();
}
