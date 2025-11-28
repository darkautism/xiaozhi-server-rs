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

use crate::state::AppState;
// use crate::handlers::ota_types::*;

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
        mode: String,
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

async fn handle_socket(mut socket: WebSocket, addr: SocketAddr, state: AppState) {
    info!("WebSocket connection established with {}", addr);

    // Track session state
    // For this simple implementation, we might accumulate audio until we receive a stop signal
    // or silence detection (mocked).
    let mut current_session_id = String::new();
    let mut is_listening = false;

    // Split socket
    let (mut sender, mut receiver) = socket.split();

    // We need a way to send messages from within the loop or async tasks.
    // Ideally use an MPSC channel if we spawn tasks, but for this simple Request-Response-Stream flow,
    // we can manage it in the main loop if we are careful.
    // However, sending audio (TTS) streams while receiving might block if we are not careful.
    // Let's keep it simple: Read loop processes input, Write loop processes output.
    // But axum's SplitSink needs to be locked or passed around.

    // For simplicity in this iteration: We process messages sequentially.
    // When we need to speak (TTS), we send the whole stream.

    // Since `sender` is consumed by sending, we might need to wrap it in an Arc<Mutex> or use a channel.
    // Let's use a channel to send messages to a writer task.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(32);

    // Spawn writer task
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
                                 } else if listen_state == "stop" {
                                     info!("Client stopped listening. Session: {}", current_session_id);
                                     is_listening = false;

                                     // 1. Trigger STT processing (mocked accumulation)
                                     // In a real app, we would have been buffering the binary audio.
                                     // Here we just call the mock STT.

                                     // Mock audio buffer
                                     let dummy_audio = vec![0u8; 10];
                                     match state.stt.recognize(&dummy_audio).await {
                                         Ok(text) => {
                                             info!("STT Result: {}", text);
                                             // Send STT result to client
                                             let stt_msg = ServerMessage::Stt { text: text.clone() };
                                             let _ = tx.send(Message::Text(serde_json::to_string(&stt_msg).unwrap().into())).await;

                                             // 2. Call LLM
                                             match state.llm.chat(&text).await {
                                                 Ok(response_text) => {
                                                     info!("LLM Response: {}", response_text);
                                                     // Send LLM emotion/text (optional)
                                                     let llm_msg = ServerMessage::Llm { emotion: Some("happy".to_string()), text: Some(response_text.clone()) };
                                                     let _ = tx.send(Message::Text(serde_json::to_string(&llm_msg).unwrap().into())).await;

                                                     // 3. TTS
                                                     // Send TTS Start
                                                     let tts_start = ServerMessage::Tts { state: "start".to_string(), text: None };
                                                     let _ = tx.send(Message::Text(serde_json::to_string(&tts_start).unwrap().into())).await;

                                                     // Generate Audio
                                                     match state.tts.speak(&response_text).await {
                                                         Ok(audio_bytes) => {
                                                             // Send Audio
                                                             let _ = tx.send(Message::Binary(audio_bytes.into())).await;
                                                         }
                                                         Err(e) => error!("TTS Error: {}", e),
                                                     }

                                                     // Send TTS Stop
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
                             },
                             ClientMessage::Abort { reason, .. } => {
                                 info!("Client aborted: {}", reason);
                                 is_listening = false;
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
                // info!("Received binary message from {}: {} bytes", addr, bin.len());
                // In a real app, append to buffer if is_listening is true.
                if is_listening {
                    // buffer.append(&mut bin);
                }
            }
            Message::Ping(_) => {
                let _ = tx.send(Message::Pong(vec![].into())).await;
            }
            Message::Pong(_) => {
                // info!("Received pong from {}", addr);
            }
            Message::Close(_) => {
                info!("Connection closed by {}", addr);
                break;
            }
        }
    }

    // Clean up
    writer_handle.abort();
}
