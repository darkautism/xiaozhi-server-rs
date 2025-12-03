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
use tokio_stream::wrappers::ReceiverStream;
use tokio::time::{Instant, Duration};

use crate::state::AppState;
use crate::services::audio::opus_codec::OpusService;
use crate::traits::SttEvent;

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

    let device_id = headers.get("x-device-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown_device")
        .to_string();

    ws.on_upgrade(move |socket| handle_socket_inner(socket, addr, state, device_id))
}

async fn process_text_pipeline(
    state: &AppState,
    tx: &Sender<Message>,
    text: &str,
    device_id: &str,
) {
     if text.trim().is_empty() {
         return;
     }

     info!("Processing text: {}", text);

     // 1. Prepare Chat History
     let mut messages = match state.db.get_chat_history(device_id, state.history_limit).await {
         Ok(h) => h,
         Err(e) => {
             error!("Failed to fetch chat history: {}", e);
             Vec::new()
         }
     };

     messages.push(crate::traits::Message {
         role: "user".to_string(),
         content: text.to_string(),
     });

     // 2. Call LLM
     match state.llm.chat(messages).await {
         Ok(response_text) => {
             info!("LLM Response: {}", response_text);

             // Check for [SLEEP] tag
             let should_sleep = response_text.contains("[SLEEP]");
             let clean_response = response_text.replace("[SLEEP]", "").trim().to_string();

             let llm_msg = ServerMessage::Llm { emotion: Some("happy".to_string()), text: Some(clean_response.clone()) };
             let _ = tx.send(Message::Text(serde_json::to_string(&llm_msg).unwrap().into())).await;

             // Save history
             let _ = state.db.add_chat_history(device_id, "user", text).await;
             let _ = state.db.add_chat_history(device_id, "model", &clean_response).await;

             // 3. TTS
             let tts_start = ServerMessage::Tts { state: "start".to_string(), text: None };
             let _ = tx.send(Message::Text(serde_json::to_string(&tts_start).unwrap().into())).await;

             let mut frame_count = 0;
             match state.tts.speak(&clean_response).await {
                 Ok(frames) => {
                     frame_count = frames.len();
                     for frame in frames {
                         let _ = tx.send(Message::Binary(frame.into())).await;
                     }
                 }
                 Err(e) => error!("TTS Error: {}", e),
             }

             // Wait for audio duration (estimated 120ms per frame) to prevent cutting off
             let wait_ms = frame_count as u64 * 120;
             if wait_ms > 0 {
                 tokio::time::sleep(Duration::from_millis(wait_ms)).await;
             }

             let tts_stop = ServerMessage::Tts { state: "stop".to_string(), text: None };
             let _ = tx.send(Message::Text(serde_json::to_string(&tts_stop).unwrap().into())).await;

             if should_sleep {
                 info!("LLM requested sleep. Closing connection.");
                 // Extra buffer to ensure client processes TTS Stop
                 tokio::time::sleep(Duration::from_secs(1)).await;
                 let _ = tx.send(Message::Close(None)).await;
             }
         }
         Err(e) => {
             error!("LLM Error: {}", e);
         }
     }
}

async fn trigger_tts_only(
    state: &AppState,
    tx: &Sender<Message>,
    text: &str,
) {
    info!("Triggering TTS only: {}", text);
    let tts_start = ServerMessage::Tts { state: "start".to_string(), text: None };
    let _ = tx.send(Message::Text(serde_json::to_string(&tts_start).unwrap().into())).await;

    let mut frame_count = 0;
    match state.tts.speak(text).await {
        Ok(frames) => {
            frame_count = frames.len();
            for frame in frames {
                let _ = tx.send(Message::Binary(frame.into())).await;
            }
        }
        Err(e) => error!("TTS Error: {}", e),
    }

    // Wait for audio duration
    let wait_ms = frame_count as u64 * 120;
    if wait_ms > 0 {
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
    }

    let tts_stop = ServerMessage::Tts { state: "stop".to_string(), text: None };
    let _ = tx.send(Message::Text(serde_json::to_string(&tts_stop).unwrap().into())).await;
}

async fn handle_socket_inner(mut socket: WebSocket, addr: SocketAddr, state: AppState, device_id: String) {
    info!("WebSocket connection established with {} (Device: {})", addr, device_id);

    let mut current_session_id = String::new();
    let mut is_listening = false;
    let mut accumulated_text = String::new();

    let mut opus_decoder = match OpusService::new_decoder() {
        Ok(d) => Some(d),
        Err(e) => {
            error!("Failed to create Opus decoder: {}", e);
            None
        }
    };

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(128);

    let mut writer_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(e) = sender.send(msg).await {
                // error!("Error sending message: {}", e);
                break;
            }
        }
    });

    // STT Streams
    let mut stt_input_tx: Option<Sender<Vec<i16>>> = None;
    let mut stt_output_stream: Option<futures_util::stream::BoxStream<'static, anyhow::Result<SttEvent>>> = None;

    // Idle Timer
    let max_idle_duration = Duration::from_millis(state.config.chat.max_idle_duration);
    let mut last_activity = Instant::now();
    let mut is_standby = false;

    loop {
        let now = Instant::now();
        let timeout_at = last_activity + max_idle_duration;
        let sleep_duration = if timeout_at > now {
            timeout_at - now
        } else {
            Duration::from_millis(100)
        };

        tokio::select! {
            msg_opt = receiver.next() => {
                last_activity = Instant::now();
                is_standby = false;

                match msg_opt {
                    Some(Ok(msg)) => {
                        match msg {
                            Message::Text(text) => {
                                info!("Received text message from {}: {}", addr, text);
                                match serde_json::from_str::<ClientMessage>(&text) {
                                    Ok(client_message) => {
                                         match client_message {
                                             ClientMessage::Hello { .. } => {
                                                 let response = ServerMessage::Hello {
                                                     transport: "websocket".to_string(),
                                                     audio_params: Some(AudioParamsResponse { sample_rate: 16000 }),
                                                 };
                                                 let _ = tx.send(Message::Text(serde_json::to_string(&response).unwrap().into())).await;
                                             },
                                             ClientMessage::Listen { session_id, state: listen_state, .. } => {
                                                 current_session_id = session_id;
                                                 if listen_state == "start" {
                                                     info!("Client started listening. Session: {}", current_session_id);
                                                     is_listening = true;
                                                     accumulated_text.clear();

                                                     // Start STT Stream
                                                     let (input_tx, input_rx) = tokio::sync::mpsc::channel(100);
                                                     stt_input_tx = Some(input_tx);

                                                     let input_stream = ReceiverStream::new(input_rx);
                                                     let boxed_input = Box::pin(input_stream);

                                                     match state.stt.stream_speech(boxed_input).await {
                                                         Ok(stream) => {
                                                             stt_output_stream = Some(stream);
                                                         },
                                                         Err(e) => {
                                                             error!("Failed to start STT stream: {}", e);
                                                         }
                                                     }

                                                 } else if listen_state == "stop" {
                                                     info!("Client stopped listening.");
                                                     is_listening = false;
                                                     // Close STT input
                                                     stt_input_tx = None;

                                                     // Process any accumulated text if not empty
                                                     if !accumulated_text.is_empty() {
                                                          process_text_pipeline(&state, &tx, &accumulated_text, &device_id).await;
                                                          last_activity = Instant::now(); // Reset idle timer
                                                          accumulated_text.clear();
                                                     }
                                                 }
                                             },
                                             ClientMessage::Abort { reason, .. } => {
                                                 info!("Client aborted: {}", reason);
                                                 is_listening = false;
                                                 stt_input_tx = None;
                                                 stt_output_stream = None;
                                                 accumulated_text.clear();
                                             },
                                             ClientMessage::Iot { .. } => {
                                                 info!("Received IoT message");
                                             }
                                         }
                                    }
                                    Err(e) => error!("Error deserializing message: {}", e),
                                }
                            }
                            Message::Binary(bin) => {
                                if is_listening {
                                    if let Some(decoder) = opus_decoder.as_mut() {
                                        let mut output = vec![0i16; 5760];
                                        match decoder.decode(&bin, &mut output, false) {
                                            Ok(len) => {
                                                let pcm_chunk = output[..len].to_vec();
                                                if let Some(tx) = stt_input_tx.as_ref() {
                                                    let _ = tx.send(pcm_chunk).await;
                                                }
                                            }
                                            Err(e) => error!("Opus decode error: {}", e),
                                        }
                                    }
                                }
                            }
                            Message::Ping(_) => { let _ = tx.send(Message::Pong(vec![].into())).await; }
                            Message::Pong(_) => {}
                            Message::Close(_) => break,
                        }
                    }
                    Some(Err(e)) => { error!("Error receiving message: {}", e); break; }
                    None => break,
                }
            }

            stt_res = async {
                if let Some(stream) = stt_output_stream.as_mut() {
                    stream.next().await
                } else {
                    std::future::pending().await
                }
            } => {
                 last_activity = Instant::now();
                 match stt_res {
                     Some(Ok(event)) => {
                         match event {
                             SttEvent::Text(text) => {
                                 accumulated_text.push_str(&text);
                                 accumulated_text.push(' ');
                                 let stt_msg = ServerMessage::Stt { text: accumulated_text.clone() };
                                 let _ = tx.send(Message::Text(serde_json::to_string(&stt_msg).unwrap().into())).await;
                             }
                             SttEvent::NoSpeech => {
                                 info!("STT NoSpeech (End of Turn). Triggering pipeline.");
                                 // Stop listening/STT to prevent interference and release resources
                                 stt_input_tx = None;
                                 stt_output_stream = None;

                                 process_text_pipeline(&state, &tx, &accumulated_text, &device_id).await;
                                 last_activity = Instant::now(); // Reset idle timer
                                 accumulated_text.clear();
                             }
                         }
                     }
                     Some(Err(e)) => {
                         error!("STT Stream Error: {}", e);
                         stt_output_stream = None;
                     }
                     None => {
                         stt_output_stream = None;
                     }
                 }
            }

            _ = tokio::time::sleep(sleep_duration) => {
                 if !is_standby {
                     info!("Idle timeout detected. Sending standby prompt.");
                     is_standby = true;
                     trigger_tts_only(&state, &tx, &state.config.chat.standby_prompt).await;
                     last_activity = Instant::now(); // Reset idle timer
                 }
            }
        }
    }

    writer_handle.abort();
}
