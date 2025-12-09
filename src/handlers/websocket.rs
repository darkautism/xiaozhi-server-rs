use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, State,
    },
    http::HeaderMap,
    response::IntoResponse,
};
use futures_util::{stream::select_all, stream::BoxStream, stream::StreamExt, SinkExt};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::time::{Duration, Instant};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info, warn};

use crate::services::audio::opus_codec::OpusService;
use crate::services::mcp::{
    create_request, ClientInfo, JsonRpcRequest, JsonRpcResponse, McpContent, McpInitializeParams,
    McpInitializeResult, McpTool, McpToolCallParams, McpToolCallResult, McpToolListResult,
};
use crate::state::AppState;
use crate::traits::{ChatResponse, SttEvent, ToolDefinition};

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
    Mcp {
        payload: Value,
        #[serde(default)]
        session_id: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AudioParamsResponse {
    pub sample_rate: u32,
    pub frame_duration: u32,
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
    },
    Mcp {
        payload: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
}

/// Upgrades the HTTP connection to a WebSocket connection.
///
/// This is the main entry point for the real-time audio/text interaction with the Xiaozhi device.
pub async fn handle_websocket(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    info!("WebSocket handshake attempt from {}", addr);
    debug!("WebSocket handshake headers: {:?}", headers);

    let device_id = headers
        .get("x-device-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown_device")
        .to_string();

    ws.on_upgrade(move |socket| handle_socket_inner(socket, addr, state, device_id))
}

fn clean_text_and_extract_emotion(text: &str) -> (String, Option<String>) {
    static EMOJI_REGEX: OnceLock<Regex> = OnceLock::new();
    let re =
        EMOJI_REGEX.get_or_init(|| Regex::new(r"\p{Emoji_Presentation}").expect("Invalid Regex"));
    let cleaned = re.replace_all(text, "").to_string();

    let emotion =
        if text.contains('😂') || text.contains('😊') || text.contains('哈') || text.contains('嘻')
        {
            Some("happy".to_string())
        } else if text.contains('😭') || text.contains('😢') || text.contains('难') {
            Some("sad".to_string())
        } else if text.contains('😡') || text.contains('怒') {
            Some("angry".to_string())
        } else {
            None
        };

    (cleaned, emotion)
}

// Helper to send message safely with timeout
async fn send_message_safe(tx: &Sender<Message>, msg: Message) -> bool {
    match tokio::time::timeout(Duration::from_secs(5), tx.send(msg)).await {
        Ok(Ok(_)) => true,
        Ok(Err(e)) => {
            error!("Failed to send message: {}", e);
            false
        }
        Err(_) => {
            error!("Failed to send message: Timeout (Writer blocked)");
            false
        }
    }
}

// Helper to send message without blocking (for use in select! loop)
fn send_nowait(tx: &Sender<Message>, msg: Message) -> bool {
    match tx.try_send(msg) {
        Ok(_) => true,
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            warn!("Outbound buffer full, dropping message to avoid blocking loop.");
            false
        }
        Err(e) => {
            error!("Outbound channel closed: {}", e);
            false
        }
    }
}

// Returns true if sleep is requested
async fn process_text_logic(
    state: &AppState,
    tx: &Sender<Message>,
    text: &str,
    device_id: &str,
    mcp_tools: Option<Vec<ToolDefinition>>,
    mcp_rpc_tx: &Sender<RpcCall>,
) -> bool {
    if text.trim().is_empty() {
        return false;
    }

    info!("Processing text: {}", text);

    let mut messages = match state
        .db
        .get_chat_history(device_id, state.history_limit)
        .await
    {
        Ok(h) => h,
        Err(e) => {
            error!("Failed to fetch chat history: {}", e);
            Vec::new()
        }
    };

    messages.push(crate::traits::Message {
        role: "user".to_string(),
        content: text.to_string(),
        tool_calls: vec![],
        tool_call_id: None,
    });

    let mut loop_count = 0;
    const MAX_LOOPS: i32 = 5;

    loop {
        if loop_count >= MAX_LOOPS {
            error!("Too many tool loops. Breaking.");
            return false;
        }
        loop_count += 1;

        // Use mcp_tools in chat request
        match state.llm.chat(messages.clone(), mcp_tools.clone()).await {
            Ok(chat_response) => {
                match chat_response {
                    ChatResponse::Text(response_text) => {
                        info!("LLM Response: {}", response_text);

                        let should_sleep = response_text.contains("[SLEEP]");
                        let raw_response = response_text.replace("[SLEEP]", "").trim().to_string();

                        let (clean_text, emotion) = clean_text_and_extract_emotion(&raw_response);

                        // Only send text if it's not empty, to avoid empty bubbles
                        if !clean_text.is_empty() {
                            let llm_msg = ServerMessage::Llm {
                                emotion: emotion.clone().or(Some("happy".to_string())),
                                text: Some(clean_text.clone()),
                            };
                            if !send_message_safe(
                                tx,
                                Message::Text(
                                    serde_json::to_string(&llm_msg)
                                        .expect("Serialize failed")
                                        .into(),
                                ),
                            )
                            .await
                            {
                                return false;
                            }

                            let _ = state.db.add_chat_history(device_id, "user", text).await;
                            let _ = state
                                .db
                                .add_chat_history(device_id, "model", &clean_text)
                                .await;

                            let tts_start = ServerMessage::Tts {
                                state: "start".to_string(),
                                text: None,
                            };
                            if !send_message_safe(
                                tx,
                                Message::Text(
                                    serde_json::to_string(&tts_start)
                                        .expect("Serialize failed")
                                        .into(),
                                ),
                            )
                            .await
                            {
                                return false;
                            }

                            let tts_sentence = ServerMessage::Tts {
                                state: "sentence_start".to_string(),
                                text: Some(clean_text.clone()),
                            };
                            if !send_message_safe(
                                tx,
                                Message::Text(
                                    serde_json::to_string(&tts_sentence)
                                        .expect("Serialize failed")
                                        .into(),
                                ),
                            )
                            .await
                            {
                                return false;
                            }

                            let mut total_frames = 0;
                            let frame_duration = Duration::from_millis(60);
                            let cache_frame_count = 2;

                            match state.tts.speak(&clean_text, emotion.as_deref()).await {
                                Ok(frames) => {
                                    let start_time = Instant::now();
                                    info!("Sending {} audio frames (paced)", frames.len());
                                    for frame in frames {
                                        // Flow control: Sliding window
                                        if total_frames >= cache_frame_count {
                                            let target_time = start_time
                                                + frame_duration
                                                    * (total_frames - cache_frame_count) as u32;
                                            let now = Instant::now();
                                            if target_time > now {
                                                tokio::time::sleep(target_time - now).await;
                                            }
                                        }

                                        if !send_message_safe(tx, Message::Binary(frame.into()))
                                            .await
                                        {
                                            warn!("Failed to send audio frame");
                                            return false; // Abort
                                        }
                                        total_frames += 1;
                                    }
                                    info!("Finished sending audio frames");

                                    let total_duration = frame_duration * total_frames as u32;
                                    let elapsed = start_time.elapsed();
                                    if total_duration > elapsed {
                                        tokio::time::sleep(total_duration - elapsed).await;
                                    }
                                    // Ensure client buffer plays out
                                    tokio::time::sleep(Duration::from_millis(500)).await;
                                }
                                Err(e) => error!("TTS Error: {}", e),
                            }

                            let tts_stop = ServerMessage::Tts {
                                state: "stop".to_string(),
                                text: None,
                            };
                            if !send_message_safe(
                                tx,
                                Message::Text(
                                    serde_json::to_string(&tts_stop)
                                        .expect("Serialize failed")
                                        .into(),
                                ),
                            )
                            .await
                            {
                                return false;
                            }
                            info!("Sent TTS Stop command");
                        }

                        if should_sleep {
                            info!("LLM requested sleep. Closing connection.");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            return true;
                        }

                        return false; // Done processing
                    }
                    ChatResponse::ToolCall(tool_calls) => {
                        info!("LLM requested tool calls: {:?}", tool_calls);

                        // Append the assistant's tool call message to history
                        messages.push(crate::traits::Message {
                            role: "assistant".to_string(),
                            content: "".to_string(),
                            tool_calls: tool_calls.clone(),
                            tool_call_id: None,
                        });

                        for call in tool_calls {
                            let (resp_tx, resp_rx) = oneshot::channel();

                            debug!(
                                "[MCP CALL] Requesting tool execution: {}",
                                call.function.name
                            );
                            // Send execution request to Main Loop
                            if let Err(e) = mcp_rpc_tx.send(RpcCall {
                                method: "tools/call".to_string(),
                                params: json!({
                                    "name": call.function.name,
                                    "arguments": serde_json::from_str::<Value>(&call.function.arguments).unwrap_or(json!({}))
                                }),
                                resp_tx
                            }).await {
                                error!("Failed to send tool call to main loop: {}", e);
                                return false;
                            }

                            // Wait for result
                            let result_json = match resp_rx.await {
                                Ok(Ok(val)) => val,
                                Ok(Err(e)) => {
                                    error!("Tool execution error: {}", e);
                                    // Should we feed error back to LLM? Yes.
                                    json!({ "error": e })
                                }
                                Err(_) => {
                                    error!("RPC channel closed");
                                    return false;
                                }
                            };

                            info!("Tool execution result: {:?}", result_json);
                            debug!("[MCP RESULT] {:?}", result_json);

                            // Format result content.
                            // The device returns { content: [{type:text, text: "..."}], isError: false }
                            // We need to extract the text content to feed back to LLM.
                            let mut tool_output = String::new();
                            if let Some(content_array) =
                                result_json.get("content").and_then(|v| v.as_array())
                            {
                                for c in content_array {
                                    if let Some(t) = c.get("text").and_then(|v| v.as_str()) {
                                        tool_output.push_str(t);
                                    }
                                }
                            } else if let Some(err) = result_json.get("error") {
                                tool_output = format!("Error: {:?}", err);
                            } else {
                                tool_output = result_json.to_string();
                            }

                            messages.push(crate::traits::Message {
                                role: "tool".to_string(),
                                content: tool_output,
                                tool_calls: vec![],
                                tool_call_id: Some(call.id.clone()),
                            });
                        }
                        // Loop continues to feed result back to LLM
                    }
                }
            }
            Err(e) => {
                error!("LLM Error: {}", e);
                return false;
            }
        }
    }
    false
}

async fn trigger_tts_only(state: &AppState, tx: &Sender<Message>, text: &str) {
    info!("Triggering TTS only: {}", text);
    let tts_start = ServerMessage::Tts {
        state: "start".to_string(),
        text: None,
    };
    if !send_message_safe(
        tx,
        Message::Text(
            serde_json::to_string(&tts_start)
                .expect("Serialize failed")
                .into(),
        ),
    )
    .await
    {
        return;
    }

    let tts_sentence = ServerMessage::Tts {
        state: "sentence_start".to_string(),
        text: Some(text.to_string()),
    };
    if !send_message_safe(
        tx,
        Message::Text(
            serde_json::to_string(&tts_sentence)
                .expect("Serialize failed")
                .into(),
        ),
    )
    .await
    {
        return;
    }

    let mut total_frames = 0;
    let frame_duration = Duration::from_millis(60);
    let cache_frame_count = 2;

    match state.tts.speak(text, None).await {
        Ok(frames) => {
            let start_time = Instant::now();
            for frame in frames {
                if total_frames >= cache_frame_count {
                    let target_time =
                        start_time + frame_duration * (total_frames - cache_frame_count) as u32;
                    let now = Instant::now();
                    if target_time > now {
                        tokio::time::sleep(target_time - now).await;
                    }
                }

                if !send_message_safe(tx, Message::Binary(frame.into())).await {
                    return;
                }
                total_frames += 1;
            }
            let total_duration = frame_duration * total_frames as u32;
            let elapsed = start_time.elapsed();
            if total_duration > elapsed {
                tokio::time::sleep(total_duration - elapsed).await;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Err(e) => error!("TTS Error: {}", e),
    }

    let tts_stop = ServerMessage::Tts {
        state: "stop".to_string(),
        text: None,
    };
    let _ = send_message_safe(
        tx,
        Message::Text(
            serde_json::to_string(&tts_stop)
                .expect("Serialize failed")
                .into(),
        ),
    )
    .await;
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

struct RpcCall {
    method: String,
    params: Value,
    resp_tx: oneshot::Sender<Result<Value, String>>,
}

enum LoopEvent {
    Ws(Result<Message, axum::Error>),
    Stt(SttEvent),
    Control(ControlMessage),
    Rpc(RpcCall),
    McpEvent(McpToolsEvent), // Internal event to pass discovered tools
}

// Struct for internal MCP events
struct McpToolsEvent {
    tools: Vec<McpTool>,
}

#[derive(PartialEq)]
enum McpState {
    Disabled,
    Initializing,
    Ready,
}

// Function to handle MCP handshake in a separate task
async fn handle_mcp_handshake_task(tx: Sender<Message>, mcp_tx: Sender<McpToolsEvent>) {
    info!("Starting MCP handshake task...");

    // 1. Send Initialize
    let id_init = 1;
    let params = McpInitializeParams {
        capabilities: json!({}),
        protocol_version: "2024-11-05".to_string(),
        client_info: ClientInfo {
            name: "XiaoZhi Server".to_string(),
            version: "1.0.0".to_string(),
        },
    };
    let req_init = create_request(
        "initialize",
        Some(serde_json::to_value(params).unwrap()),
        Some(json!(id_init)),
    );
    let mcp_msg_init = ServerMessage::Mcp {
        payload: serde_json::to_value(req_init).unwrap(),
        session_id: None,
    };

    debug!("Sending MCP Initialize request...");
    if !send_nowait(
        &tx,
        Message::Text(
            serde_json::to_string(&mcp_msg_init)
                .expect("Serialize failed")
                .into(),
        ),
    ) {
        warn!("Failed to send MCP Initialize request");
        return;
    }

    // Note: We are not blocking here to wait for response. The main loop receives the response.
    // However, to strictly sequence "Initialize -> Tools/List", we usually need to wait.
    // But since the main loop handles *all* incoming messages, we can't easily "wait" here for a specific message
    // without complicated channel wiring for *every* response.

    // Instead, the main loop should trigger the next step.
    // BUT, the requirement is "This should be a new thread... subsequent operations are not necessarily locked".

    // If we want this task to manage the flow, we need a way to receive specific MCP responses.
    // Current architecture: Main loop receives ALL WS messages.
    // Refined approach:
    // Main loop sees "Hello" -> Spawns this task? No, main loop should just send Init.
    // Main loop sees "Init Response" -> Sends "Tools/List".
    // Main loop sees "Tools List Response" -> Updates state.

    // The user suggestion: "When you receive MCP spec... you are the initiator... This should be a new thread... and you must remember not to get stuck in select."
    // If I spawn a thread that *sends* commands, that's fine. But *receiving* the response still happens in the main loop.

    // Let's stick to the Event-Driven approach in the main loop for handshake steps, as it is non-blocking by design.
    // I will improve the state machine in the main loop to be more explicit about these steps.
}

async fn handle_socket_inner(
    socket: WebSocket,
    addr: SocketAddr,
    state: AppState,
    device_id: String,
) {
    info!(
        "WebSocket connection established with {} (Device: {})",
        addr, device_id
    );

    let mut current_session_id = String::new();
    let mut accumulated_text = String::new();

    let mut opus_decoder = match OpusService::new_decoder() {
        Ok(d) => Some(d),
        Err(e) => {
            error!("Failed to create Opus decoder: {}", e);
            None
        }
    };

    let (mut sender, receiver) = socket.split();
    // 256 buffer
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Message>(256);

    let writer_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Message::Text(text) = &msg {
                // info!("Writer: Sending text message: {}", text);
            }
            if let Err(e) = sender.send(msg).await {
                // error!("Error sending message: {}", e);
                break;
            }
        }
    });

    let (stt_audio_tx, stt_audio_rx) = tokio::sync::mpsc::channel::<Vec<i16>>(256);
    let (stt_event_tx, stt_event_rx) = tokio::sync::mpsc::channel::<SttEvent>(32);

    let stt = state.stt.clone();
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

    // Channel for logic task to receive text (and now current tools)
    let (llm_tx, mut llm_rx) =
        tokio::sync::mpsc::channel::<(String, Option<Vec<ToolDefinition>>)>(16);
    let (control_tx, control_rx) = tokio::sync::mpsc::channel::<ControlMessage>(16);
    // Channel for logic task to request RPC calls from main loop
    let (rpc_tx, rpc_rx) = tokio::sync::mpsc::channel::<RpcCall>(16);
    // Channel for internal MCP events (like tools discovery)
    let (mcp_event_tx, mcp_event_rx) = tokio::sync::mpsc::channel::<McpToolsEvent>(16);

    let state_clone = state.clone();
    let tx_clone = tx.clone();
    let dev_id = device_id.clone();
    let control_tx_llm = control_tx.clone();
    let rpc_tx_clone = rpc_tx.clone();

    tokio::spawn(async move {
        while let Some((text, tools)) = llm_rx.recv().await {
            let should_sleep = process_text_logic(
                &state_clone,
                &tx_clone,
                &text,
                &dev_id,
                tools,
                &rpc_tx_clone,
            )
            .await;
            if should_sleep {
                let _ = control_tx_llm.send(ControlMessage::Sleep).await;
            } else {
                let _ = control_tx_llm.send(ControlMessage::LlmFinished).await;
            }
        }
    });

    let ws_stream = receiver.map(LoopEvent::Ws);
    let stt_stream = ReceiverStream::new(stt_event_rx).map(LoopEvent::Stt);
    let ctrl_stream = ReceiverStream::new(control_rx).map(LoopEvent::Control);
    let rpc_stream = ReceiverStream::new(rpc_rx).map(LoopEvent::Rpc);
    let mcp_event_stream = ReceiverStream::new(mcp_event_rx).map(LoopEvent::McpEvent);

    let streams: Vec<BoxStream<'static, LoopEvent>> = vec![
        Box::pin(ws_stream),
        Box::pin(stt_stream),
        Box::pin(ctrl_stream),
        Box::pin(rpc_stream),
        Box::pin(mcp_event_stream),
    ];
    let mut all_events = select_all(streams);

    let mut state_enum = SessionState::Listening;
    let max_idle_duration = Duration::from_millis(state.config.chat.max_idle_duration);
    let mut last_activity = Instant::now();
    let mut is_standby = false;

    // MCP State
    let mut mcp_state = McpState::Disabled;
    let mut mcp_request_id_counter = 1;
    let mut pending_mcp_requests: HashMap<i64, oneshot::Sender<Result<Value, String>>> =
        HashMap::new();
    let mut mcp_tools: Vec<McpTool> = Vec::new();

    loop {
        let now = Instant::now();
        let timeout_at = last_activity + max_idle_duration;
        let sleep_duration = if state_enum == SessionState::Processing {
            Duration::from_secs(3600 * 24)
        } else if timeout_at > now {
            timeout_at - now
        } else {
            Duration::from_millis(100)
        };

        tokio::select! {
            event_opt = all_events.next() => {
                match event_opt {
                    Some(LoopEvent::Ws(res)) => {
                        match res {
                            Ok(msg) => {
                                match msg {
                                    Message::Text(text) => {
                                        last_activity = Instant::now();
                                        is_standby = false;
                                        info!("Received text message: {}", text);
                                        match serde_json::from_str::<ClientMessage>(&text) {
                                            Ok(client_message) => {
                                                 match client_message {
                                                     ClientMessage::Hello { features, .. } => {
                                                         let response = ServerMessage::Hello {
                                                             transport: "websocket".to_string(),
                                                             audio_params: Some(AudioParamsResponse {
                                                                 sample_rate: 16000,
                                                                 frame_duration: 60,
                                                             }),
                                                         };
                                                         send_nowait(&tx, Message::Text(serde_json::to_string(&response).expect("Serialize failed").into()));

                                                         // Check for MCP support
                                                         if let Some(feats) = features {
                                                             if feats.get("mcp").and_then(|v| v.as_bool()).unwrap_or(false) {
                                                                 info!("Client supports MCP. Initializing handshake in background...");
                                                                 mcp_state = McpState::Initializing;

                                                                 // Spawn a task to handle the handshake initiation (non-blocking)
                                                                 // Actually, we can just send the first message here.
                                                                 // The logic needs to be state-driven in the loop.
                                                                 let id = mcp_request_id_counter;
                                                                 mcp_request_id_counter += 1;

                                                                 let params = McpInitializeParams {
                                                                     capabilities: json!({}),
                                                                     protocol_version: "2024-11-05".to_string(),
                                                                     client_info: ClientInfo {
                                                                         name: "XiaoZhi Server".to_string(),
                                                                         version: "1.0.0".to_string(),
                                                                     }
                                                                 };

                                                                 let req = create_request("initialize", Some(serde_json::to_value(params).unwrap()), Some(json!(id)));
                                                                 let mcp_msg = ServerMessage::Mcp {
                                                                     payload: serde_json::to_value(req).unwrap(),
                                                                     session_id: Some(current_session_id.clone()),
                                                                 };
                                                                 send_nowait(&tx, Message::Text(serde_json::to_string(&mcp_msg).expect("Serialize failed").into()));
                                                             }
                                                         }
                                                     },
                                                     ClientMessage::Listen { session_id, state: listen_state, .. } => {
                                                         current_session_id = session_id;
                                                         if listen_state == "start" {
                                                             info!("Client started listening.");
                                                             state_enum = SessionState::Listening;
                                                             accumulated_text.clear();
                                                         } else if listen_state == "stop" {
                                                             info!("Client stopped listening.");
                                                             if !accumulated_text.is_empty() {
                                                                 state_enum = SessionState::Processing;
                                                                 // Convert MCP tools to ToolDefinition for LLM
                                                                 let tool_defs: Option<Vec<ToolDefinition>> = if !mcp_tools.is_empty() {
                                                                     debug!("Passing {} tools to LLM logic.", mcp_tools.len());
                                                                     Some(mcp_tools.iter().map(|t| ToolDefinition {
                                                                         name: t.name.clone(),
                                                                         description: t.description.clone(),
                                                                         parameters: t.input_schema.clone()
                                                                     }).collect())
                                                                 } else {
                                                                     debug!("No tools to pass to LLM logic.");
                                                                     None
                                                                 };

                                                                 let _ = llm_tx.send((accumulated_text.clone(), tool_defs)).await;
                                                                 accumulated_text.clear();
                                                             } else {
                                                                 state_enum = SessionState::Processing;
                                                             }
                                                         }
                                                     },
                                                     ClientMessage::Abort { reason, .. } => {
                                                         info!("Client aborted: {}", reason);
                                                         state_enum = SessionState::Listening;
                                                         accumulated_text.clear();
                                                     },
                                                     ClientMessage::Iot { .. } => {
                                                         info!("Received IoT message");
                                                     },
                                                     ClientMessage::Mcp { payload, .. } => {
                                                         debug!("Received MCP payload: {:?}", payload);
                                                         // Handle MCP responses or notifications
                                                         if let Ok(response) = serde_json::from_value::<JsonRpcResponse>(payload.clone()) {
                                                             if let Some(id_val) = response.id {
                                                                 if let Some(id) = id_val.as_i64() {
                                                                     // Check if this ID matches a pending request (like tool calls)
                                                                     if let Some(tx_chan) = pending_mcp_requests.remove(&id) {
                                                                         if let Some(error) = response.error {
                                                                             let _ = tx_chan.send(Err(format!("{}: {}", error.code, error.message)));
                                                                         } else {
                                                                             let _ = tx_chan.send(Ok(response.result.unwrap_or(Value::Null)));
                                                                         }
                                                                     } else {
                                                                         // If not a pending tool call, check if it is part of the handshake state machine
                                                                         if mcp_state == McpState::Initializing {
                                                                             // We assume this is the response to "initialize"
                                                                             // (In a robust implementation we would track the ID, but for this linear handshake, simple state checks suffice)
                                                                             info!("Received MCP Initialize response. Fetching tools list...");

                                                                             // Send tools/list
                                                                             let id = mcp_request_id_counter;
                                                                             mcp_request_id_counter += 1;
                                                                             let req = create_request("tools/list", Some(json!({ "cursor": "" })), Some(json!(id)));
                                                                 let mcp_msg = ServerMessage::Mcp {
                                                                     payload: serde_json::to_value(req).unwrap(),
                                                                     session_id: Some(current_session_id.clone()),
                                                                 };
                                                                             send_nowait(&tx, Message::Text(serde_json::to_string(&mcp_msg).expect("Serialize failed").into()));

                                                                             mcp_state = McpState::Ready; // Transition to Ready state, expecting tools list next
                                                                         } else if mcp_state == McpState::Ready {
                                                                             // We assume this is the response to "tools/list"
                                                                             if let Ok(tool_list_res) = serde_json::from_value::<McpToolListResult>(response.result.clone().unwrap_or(json!({}))) {
                                                                                 if !tool_list_res.tools.is_empty() {
                                                                                     info!("Discovered {} MCP tools.", tool_list_res.tools.len());
                                                                                     debug!("[MCP TOOLS] {:?}", tool_list_res.tools);
                                                                                     mcp_tools = tool_list_res.tools;
                                                                                 }
                                                                             }
                                                                         }
                                                                     }
                                                                 }
                                                             }
                                                         }
                                                     }
                                                 }
                                            }
                                            Err(e) => error!("Error deserializing message: {}", e),
                                        }
                                    }
                                    Message::Binary(bin) => {
                                        if state_enum == SessionState::Listening {
                                            // Do NOT reset last_activity on audio packets!
                                            is_standby = false;
                                            if let Some(decoder) = opus_decoder.as_mut() {
                                                let mut output = vec![0i16; 5760];
                                                match decoder.decode(&bin, &mut output, false) {
                                                    Ok(len) => {
                                                        let pcm_chunk = output[..len].to_vec();
                                                        let _ = stt_audio_tx.send(pcm_chunk).await;
                                                    }
                                                    Err(e) => error!("Opus decode error: {}", e),
                                                }
                                            }
                                        }
                                    }
                                    Message::Ping(_) => { send_nowait(&tx, Message::Pong(vec![].into())); }
                                    Message::Pong(_) => {}
                                    Message::Close(_) => break,
                                }
                            },
                            Err(e) => {
                                error!("WS Error: {}", e);
                                break;
                            }
                        }
                    },
                    Some(LoopEvent::Stt(evt)) => {
                        match evt {
                            SttEvent::Text(text) => {
                                last_activity = Instant::now();
                                is_standby = false;
                                accumulated_text.push_str(&text);
                                accumulated_text.push(' ');
                                let stt_msg = ServerMessage::Stt { text: accumulated_text.clone() };
                                send_nowait(&tx, Message::Text(serde_json::to_string(&stt_msg).expect("Serialize failed").into()));
                            }
                            SttEvent::NoSpeech => {
                                if !accumulated_text.trim().is_empty() {
                                    info!("STT NoSpeech. Triggering LLM.");
                                    state_enum = SessionState::Processing;

                                    // Convert MCP tools
                                    let tool_defs: Option<Vec<ToolDefinition>> = if !mcp_tools.is_empty() {
                                        debug!("Passing {} tools to LLM logic.", mcp_tools.len());
                                        Some(mcp_tools.iter().map(|t| ToolDefinition {
                                            name: t.name.clone(),
                                            description: t.description.clone(),
                                            parameters: t.input_schema.clone()
                                        }).collect())
                                    } else {
                                        debug!("No tools to pass to LLM logic.");
                                        None
                                    };

                                    let _ = llm_tx.send((accumulated_text.clone(), tool_defs)).await;
                                    accumulated_text.clear();
                                }
                            }
                        }
                    },
                    Some(LoopEvent::Control(msg)) => {
                        match msg {
                            ControlMessage::LlmFinished => {
                                info!("LLM Finished. Switching to Listening.");
                                state_enum = SessionState::Listening;
                                last_activity = Instant::now();
                                is_standby = false;
                            }
                            ControlMessage::Sleep => {
                                info!("Sleep requested. Closing.");
                                send_nowait(&tx, Message::Close(None));
                                break;
                            }
                        }
                    },
                    Some(LoopEvent::Rpc(call)) => {
                        info!("Executing MCP RPC: {}", call.method);
                        let id = mcp_request_id_counter;
                        mcp_request_id_counter += 1;

                        let req = create_request(&call.method, Some(call.params), Some(json!(id)));
                        let mcp_msg = ServerMessage::Mcp {
                            payload: serde_json::to_value(req).unwrap(),
                            session_id: Some(current_session_id.clone()),
                        };

                        if send_nowait(&tx, Message::Text(serde_json::to_string(&mcp_msg).expect("Serialize failed").into())) {
                            pending_mcp_requests.insert(id, call.resp_tx);
                        } else {
                             // Fail the RPC immediately if channel is full
                             let _ = call.resp_tx.send(Err("Outbound buffer full - dropped tool call".to_string()));
                        }
                    },
                    Some(LoopEvent::McpEvent(evt)) => {
                        // Handle internal MCP events (like updates from background tasks)
                        mcp_tools = evt.tools;
                        info!("Updated MCP tools list with {} tools.", mcp_tools.len());
                    },
                    None => {
                        info!("A stream ended (likely connection closed). Exiting loop.");
                        break;
                    }
                }
            }

            _ = tokio::time::sleep(sleep_duration) => {
                 if !is_standby && state_enum != SessionState::Processing {
                     info!("Idle timeout detected. Sending standby prompt.");
                     is_standby = true;

                     let state_clone = state.clone();
                     let tx_clone = tx.clone();
                     let prompt = state.config.chat.standby_prompt.clone();
                     let control_tx_clone = control_tx.clone();

                     tokio::spawn(async move {
                         trigger_tts_only(&state_clone, &tx_clone, &prompt).await;
                         // Signal completion to reset idle timer after speaking
                         // Also for standby prompt, we want to Close connection as per user request
                         // "Your timeout implementation is incorrect... should reply I'm going to rest, then disconnect WS"
                         // So we send Sleep.
                         let _ = control_tx_clone.send(ControlMessage::Sleep).await;
                     });
                 }
            }
        }
    }

    writer_handle.abort();
    info!("WebSocket connection with {} closed.", addr);
}
