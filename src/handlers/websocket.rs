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

// Returns true if sleep is requested
async fn process_text_logic(
    state: &AppState,
    tx: &Sender<Message>,
    text: &str,
    device_id: &str,
) -> bool {
     if text.trim().is_empty() {
         return false;
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
                     info!("Sending {} audio frames", frame_count);
                     for frame in frames {
                         let _ = tx.send(Message::Binary(frame.into())).await;
                     }
                     info!("Finished sending audio frames");
                 }
                 Err(e) => error!("TTS Error: {}", e),
             }

             // Calculate audio duration
             let wait_ms = frame_count as u64 * 120;

             // Send Stop immediately so client starts playing
             let tts_stop = ServerMessage::Tts { state: "stop".to_string(), text: None };
             let _ = tx.send(Message::Text(serde_json::to_string(&tts_stop).unwrap().into())).await;
             info!("Sent TTS Stop command");

             if should_sleep {
                 info!("LLM requested sleep. Keeping connection open for playback ({} ms)...", wait_ms);
                 if wait_ms > 0 {
                     tokio::time::sleep(Duration::from_millis(wait_ms)).await;
                 }
                 // Extra buffer
                 tokio::time::sleep(Duration::from_secs(1)).await;
             }

             return should_sleep;
         }
         Err(e) => {
             error!("LLM Error: {}", e);
             return false;
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

    match state.tts.speak(text).await {
        Ok(frames) => {
            for frame in frames {
                let _ = tx.send(Message::Binary(frame.into())).await;
            }
        }
        Err(e) => error!("TTS Error: {}", e),
    }

    // Send Stop immediately
    let tts_stop = ServerMessage::Tts { state: "stop".to_string(), text: None };
    let _ = tx.send(Message::Text(serde_json::to_string(&tts_stop).unwrap().into())).await;
}

#[derive(PartialEq)]
enum SessionState {
    Listening,
    Processing,
}

enum ControlMessage {
    LlmFinished,
    Sleep,
}

async fn handle_socket_inner(mut socket: WebSocket, addr: SocketAddr, state: AppState, device_id: String) {
    info!("WebSocket connection established with {} (Device: {})", addr, device_id);

    let mut current_session_id = String::new();
    let mut accumulated_text = String::new();

    let mut opus_decoder = match OpusService::new_decoder() {
        Ok(d) => Some(d),
        Err(e) => {
            error!("Failed to create Opus decoder: {}", e);
            None
        }
    };

    let (mut sender, mut receiver) = socket.split();
    // Increase channel size to avoid backpressure from fast TTS
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(256);

    let mut writer_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Message::Text(text) = &msg {
                info!("Writer: Sending text message: {}", text);
            }
            if let Err(e) = sender.send(msg).await {
                // error!("Error sending message: {}", e);
                break;
            }
        }
    });

    // 1. STT Worker Setup
    let (stt_audio_tx, stt_audio_rx) = tokio::sync::mpsc::channel::<Vec<i16>>(256);
    let (stt_event_tx, mut stt_event_rx) = tokio::sync::mpsc::channel::<SttEvent>(32);

    let stt = state.stt.clone();
    // We spawn the stream bridge task. stream_speech internally spawns the dedicated thread.
    // We just need to pipe stt_audio_rx -> input_stream -> stream_speech -> stt_event_tx
    tokio::spawn(async move {
        let input_stream = ReceiverStream::new(stt_audio_rx);
        let boxed_input = Box::pin(input_stream);
        match stt.stream_speech(boxed_input).await {
            Ok(mut output_stream) => {
                while let Some(res) = output_stream.next().await {
                    match res {
                        Ok(evt) => {
                            if stt_event_tx.send(evt).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => error!("STT Stream Error: {}", e),
                    }
                }
            }
            Err(e) => error!("Failed to start STT stream: {}", e),
        }
    });

    // 2. LLM Worker Setup
    let (llm_tx, mut llm_rx) = tokio::sync::mpsc::channel::<String>(16);
    let (control_tx, mut control_rx) = tokio::sync::mpsc::channel::<ControlMessage>(16);

    let state_clone = state.clone();
    let tx_clone = tx.clone();
    let dev_id = device_id.clone();

    tokio::spawn(async move {
        while let Some(text) = llm_rx.recv().await {
            let should_sleep = process_text_logic(&state_clone, &tx_clone, &text, &dev_id).await;
            if should_sleep {
                let _ = control_tx.send(ControlMessage::Sleep).await;
            } else {
                let _ = control_tx.send(ControlMessage::LlmFinished).await;
            }
        }
    });

    // 3. Main Loop
    let mut state_enum = SessionState::Listening;
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
            // A. WebSocket Messages
            msg_opt = receiver.next() => {
                last_activity = Instant::now();
                is_standby = false;

                match msg_opt {
                    Some(Ok(msg)) => {
                        match msg {
                            Message::Text(text) => {
                                info!("Received text message: {}", text);
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
                                                     info!("Client started listening.");
                                                     // We are always listening in STT thread, just need to set state to forward audio
                                                     state_enum = SessionState::Listening;
                                                     accumulated_text.clear();
                                                 } else if listen_state == "stop" {
                                                     info!("Client stopped listening.");
                                                     // Transition to Processing? Or Idle?
                                                     // Usually explicit stop means user finished speaking manually.
                                                     // We should process what we have.
                                                     if !accumulated_text.is_empty() {
                                                         state_enum = SessionState::Processing;
                                                         let _ = llm_tx.send(accumulated_text.clone()).await;
                                                         accumulated_text.clear();
                                                     } else {
                                                         // If no text, just idle/processing waiting for nothing?
                                                         // Maybe go to Listening? No, stop means stop.
                                                         // But we stay in Listening state effectively just waiting for start?
                                                         // Or we block audio.
                                                         // Let's assume Processing state blocks audio.
                                                         state_enum = SessionState::Processing;
                                                         // But we won't trigger LLM.
                                                         // We'll wait for next Listen: start.
                                                     }
                                                 }
                                             },
                                             ClientMessage::Abort { reason, .. } => {
                                                 info!("Client aborted: {}", reason);
                                                 state_enum = SessionState::Listening; // Reset?
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
                                if state_enum == SessionState::Listening {
                                    if let Some(decoder) = opus_decoder.as_mut() {
                                        let mut output = vec![0i16; 5760];
                                        match decoder.decode(&bin, &mut output, false) {
                                            Ok(len) => {
                                                let pcm_chunk = output[..len].to_vec();
                                                // Send to STT Worker
                                                let _ = stt_audio_tx.send(pcm_chunk).await;
                                            }
                                            Err(e) => error!("Opus decode error: {}", e),
                                        }
                                    }
                                }
                                // Else: Drop audio (Processing state)
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

            // B. STT Events
            stt_evt_opt = stt_event_rx.recv() => {
                last_activity = Instant::now();
                match stt_evt_opt {
                    Some(event) => {
                        match event {
                            SttEvent::Text(text) => {
                                accumulated_text.push_str(&text);
                                accumulated_text.push(' ');
                                let stt_msg = ServerMessage::Stt { text: accumulated_text.clone() };
                                let _ = tx.send(Message::Text(serde_json::to_string(&stt_msg).unwrap().into())).await;
                            }
                            SttEvent::NoSpeech => {
                                if !accumulated_text.trim().is_empty() {
                                    info!("STT NoSpeech. Triggering LLM.");
                                    state_enum = SessionState::Processing;
                                    let _ = llm_tx.send(accumulated_text.clone()).await;
                                    accumulated_text.clear();
                                } else {
                                    // Silence detected but no text.
                                    // Just continue listening?
                                    // STT thread continues automatically.
                                }
                            }
                        }
                    }
                    None => {
                        error!("STT Worker died.");
                        break;
                    }
                }
            }

            // C. Control Events (LLM Finished)
            ctrl_opt = control_rx.recv() => {
                match ctrl_opt {
                    Some(ControlMessage::LlmFinished) => {
                        info!("LLM Finished. Switching to Listening.");
                        state_enum = SessionState::Listening;
                        last_activity = Instant::now(); // Reset idle timer
                    }
                    Some(ControlMessage::Sleep) => {
                        info!("Sleep requested. Closing.");
                        let _ = tx.send(Message::Close(None)).await;
                        break;
                    }
                    None => {
                        error!("Control channel closed.");
                        break;
                    }
                }
            }

            // D. Idle Timer
            _ = tokio::time::sleep(sleep_duration) => {
                 if !is_standby {
                     info!("Idle timeout detected. Sending standby prompt.");
                     is_standby = true;

                     let state_clone = state.clone();
                     let tx_clone = tx.clone();
                     let prompt = state.config.chat.standby_prompt.clone();

                     tokio::spawn(async move {
                         trigger_tts_only(&state_clone, &tx_clone, &prompt).await;
                     });

                     last_activity = Instant::now();
                 }
            }
        }
    }

    writer_handle.abort();
}
