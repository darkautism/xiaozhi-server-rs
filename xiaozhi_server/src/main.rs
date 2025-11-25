use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};
use futures_util::{StreamExt, SinkExt};


// ... (data structures from previous step remain the same) ...
// Client -> Server Messages

#[derive(Serialize, Deserialize, Debug)]
pub struct AudioParams {
    pub format: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub frame_duration: u32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ListenState {
    Start,
    Stop,
    Detect,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ListenMode {
    Auto,
    Manual,
    Realtime,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ClientMessage {
    Hello {
        version: u32,
        transport: String,
        audio_params: AudioParams,
    },
    Listen {
        session_id: String,
        state: ListenState,
        mode: ListenMode,
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


// Server -> Client Messages

#[derive(Serialize, Deserialize, Debug)]
pub struct AudioParamsResponse {
    pub sample_rate: u32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TtsState {
    Start,
    Stop,
    SentenceStart,
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
    Llm {
        emotion: String,
        text: String,
    },
    Tts {
        state: TtsState,
        #[serde(default)]
        text: Option<String>,
    },
    Iot {
        commands: Vec<Value>,
    },
}


#[tokio::main]
async fn main() {
    let addr = "127.0.0.1:9002";
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");
    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(handle_connection(stream));
    }
}

async fn handle_connection(stream: TcpStream) {
    let addr = stream.peer_addr().expect("connected streams should have a peer address");
    println!("Peer address: {}", addr);

    let mut ws_stream = accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");

    println!("New WebSocket connection: {}", addr);

    while let Some(msg) = ws_stream.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(e) => {
                println!("Error receiving message from {}: {}", addr, e);
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                println!("Received text message from {}: {}", addr, text);
                match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(client_message) => {
                        handle_client_message(client_message, &mut ws_stream, &addr).await;
                    }
                    Err(e) => {
                        println!("Error deserializing message from {}: {}", addr, e);
                    }
                }
            }
            Message::Binary(bin) => {
                println!("Received binary message from {}: {} bytes", addr, bin.len());
                // For now, we just log that we received a binary message.
            }
            Message::Ping(_) => {
                println!("Received ping from {}", addr);
            }
            Message::Pong(_) => {
                println!("Received pong from {}", addr);
            }
            Message::Close(_) => {
                println!("Connection closed by {}", addr);
                break;
            }
            Message::Frame(_) => {
                // This is not expected from the client.
            }
        }
    }
}

async fn handle_client_message(
    message: ClientMessage,
    ws_stream: &mut tokio_tungstenite::WebSocketStream<TcpStream>,
    addr: &SocketAddr,
) {
    match message {
        ClientMessage::Hello { version, transport, audio_params } => {
            println!("Received hello from {}: version={}, transport={}, audio_params={:?}", addr, version, transport, audio_params);
            let response = ServerMessage::Hello {
                transport: "websocket".to_string(),
                audio_params: Some(AudioParamsResponse { sample_rate: 16000 }),
            };
            let response_text = serde_json::to_string(&response).unwrap();
            ws_stream.send(Message::Text(response_text)).await.unwrap();
        }
        ClientMessage::Listen { session_id, state, mode, text } => {
            println!("Received listen from {}: session_id={}, state={:?}, mode={:?}, text={:?}", addr, session_id, state, mode, text);
        }
        ClientMessage::Abort { session_id, reason } => {
            println!("Received abort from {}: session_id={}, reason={}. Feature not implemented.", addr, session_id, reason);
        }
        ClientMessage::Iot { session_id, descriptors, states } => {
            println!("Received iot from {}: session_id={}, descriptors={:?}, states={:?}. Feature not implemented.", addr, session_id, descriptors, states);
        }
    }
}
