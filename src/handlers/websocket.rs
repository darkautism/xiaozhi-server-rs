use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State, ConnectInfo},
    response::IntoResponse,
};
use std::net::SocketAddr;
use futures_util::{stream::StreamExt, SinkExt};
use serde_json::Value;
use tracing::{info, warn, error};

use crate::state::AppState;
use crate::handlers::ota_types::*; // Re-using types if needed or define new ones

// Re-using the message structs from the original main.rs
// Ideally these should be moved to a shared types file, but for now I'll include them here
// or define them in a dedicated types file.
// Since I already have `ota_types.rs`, I will create `websocket_types.rs` or just put them here for now.
// To keep it clean, let's put them here as they are specific to the WS protocol.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct AudioParams {
    pub format: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub frame_duration: u32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    Hello {
        version: u32,
        transport: String,
        audio_params: AudioParams,
    },
    Listen {
        session_id: String,
        state: String, // Simplified for brevity
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
    // ... other variants ...
}

pub async fn handle_websocket(
    ws: WebSocketUpgrade,
    State(_state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    info!("New WebSocket connection from {}", addr);
    ws.on_upgrade(move |socket| handle_socket(socket, addr))
}

async fn handle_socket(mut socket: WebSocket, addr: SocketAddr) {
    while let Some(msg) = socket.recv().await {
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
                         // Simple echo/hello response logic from original code
                         match client_message {
                             ClientMessage::Hello { .. } => {
                                 let response = ServerMessage::Hello {
                                     transport: "websocket".to_string(),
                                     audio_params: Some(AudioParamsResponse { sample_rate: 16000 }),
                                 };
                                 let response_text = serde_json::to_string(&response).unwrap();
                                 if let Err(e) = socket.send(Message::Text(response_text.into())).await {
                                     error!("Failed to send hello response: {}", e);
                                     break;
                                 }
                             },
                             _ => {
                                 info!("Unhandled message type: {:?}", client_message);
                             }
                         }
                    }
                    Err(e) => {
                        error!("Error deserializing message from {}: {}", addr, e);
                    }
                }
            }
            Message::Binary(bin) => {
                info!("Received binary message from {}: {} bytes", addr, bin.len());
            }
            Message::Ping(_) => {
                info!("Received ping from {}", addr);
            }
            Message::Pong(_) => {
                info!("Received pong from {}", addr);
            }
            Message::Close(_) => {
                info!("Connection closed by {}", addr);
                break;
            }
        }
    }
}
