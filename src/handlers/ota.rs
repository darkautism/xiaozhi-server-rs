use axum::{
    extract::{State, Request},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use std::time::SystemTime;
// use uuid::Uuid; // Unused
use hmac::{Hmac, Mac};
use sha2::Sha256;
use hex;
use rand::{distr::Alphanumeric, Rng};

use crate::state::AppState;
use crate::handlers::ota_types::*;

pub async fn handle_ota(
    State(state): State<AppState>,
    headers: HeaderMap,
    _request: Request,
) -> impl IntoResponse {
    let device_id = match headers.get("Device-Id") {
        Some(v) => v.to_str().unwrap_or("").to_string(),
        None => return (StatusCode::BAD_REQUEST, "Missing Device-Id").into_response(),
    };
    let client_id = match headers.get("Client-Id") {
        Some(v) => v.to_str().unwrap_or("").to_string(),
        None => return (StatusCode::BAD_REQUEST, "Missing Client-Id").into_response(),
    };

    // Auth Check
    let mut activation_info: Option<ActivationInfo> = None;
    if state.config.auth.enable {
        // We use the async trait methods now
        let is_activated = state.db.is_activated(&device_id).await.unwrap_or(false);

        if !is_activated {
             // Generate Challenge
             let challenge: String = rand::rng()
                 .sample_iter(&Alphanumeric)
                 .take(32)
                 .map(char::from)
                 .collect();

             // Add challenge to DB
             // Ignore errors for now or log them
             if let Err(e) = state.db.add_challenge(&device_id, &challenge, 300).await {
                 tracing::error!("Failed to save challenge: {}", e);
             }

             activation_info = Some(ActivationInfo {
                 code: "0".to_string(), // Placeholder
                 message: "Device not activated".to_string(),
                 challenge,
                 timeout_ms: 300000,
             });
        }
    }

    let ota_config = &state.config.ota;

    // WebSocket URL construction
    let websocket_url = if let Some(url) = &ota_config.websocket_url {
        url.clone()
    } else {
        // Construct dynamic URL from Host header
        let host = headers.get("Host")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("localhost:8002"); // Fallback
        format!("ws://{}/xiaozhi/v1/", host)
    };

    let websocket_token = ota_config.websocket_token.clone().unwrap_or_default();

    // MQTT Info Construction
    let mqtt_info = if ota_config.mqtt.enable {
        // Generate MQTT credentials (mock)
        let signature_key = &state.config.auth.signature_key;
        let password_raw = format!("{}:{}:{}", device_id, client_id, signature_key);
        // Simple hash for password
        let mut mac = Hmac::<Sha256>::new_from_slice(signature_key.as_bytes()).expect("HMAC can take key of any size");
        mac.update(password_raw.as_bytes());
        let result = mac.finalize();
        let password = hex::encode(result.into_bytes());

        Some(MqttInfo {
            endpoint: ota_config.mqtt.endpoint.clone(),
            client_id: format!("{}_client", device_id),
            username: device_id.clone(),
            password,
            publish_topic: format!("device/{}/pub", device_id),
            subscribe_topic: format!("device/{}/sub", device_id),
        })
    } else {
        None
    };

    let response = OtaResponse {
        websocket: WebsocketInfo {
            url: websocket_url,
            token: websocket_token,
        },
        mqtt: mqtt_info,
        server_time: ServerTimeInfo {
            timestamp: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as i64,
            timezone_offset: 480, // UTC+8
            time_zone: "Asia/Shanghai".to_string(),
        },
        activation: activation_info,
        firmware: FirmwareInfo {
            version: ota_config.firmware_version.clone(),
            url: "".to_string(),
        },
    };

    // Log the response payload for debugging
    tracing::debug!("OTA response for device {}: {:?}", device_id, response);

    Json(response).into_response()
}

pub async fn handle_ota_activate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ActivationRequest>,
) -> impl IntoResponse {
    let device_id = match headers.get("Device-Id") {
        Some(v) => v.to_str().unwrap_or("").to_string(),
        None => return (StatusCode::BAD_REQUEST, "Missing Device-Id").into_response(),
    };

    if req.payload.algorithm != "hmac-sha256" {
         return (StatusCode::BAD_REQUEST, "Unsupported algorithm").into_response();
    }

    // Find the challenge we issued for this device
    let stored_challenge = match state.db.get_challenge(&device_id).await {
        Ok(Some(c)) => c,
        Ok(None) => return (StatusCode::FORBIDDEN, "No pending challenge or expired").into_response(),
        Err(e) => {
            tracing::error!("DB Error: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    let signature_to_verify = req.payload.signature.clone().or(req.payload.digest.clone()).unwrap_or_default();

    let secret_key = &state.config.auth.signature_key;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(stored_challenge.as_bytes());
    let expected_signature_bytes = mac.finalize().into_bytes();
    let expected_signature = hex::encode(expected_signature_bytes);

    if signature_to_verify == expected_signature {
        if let Err(e) = state.db.activate_device(&device_id).await {
             tracing::error!("Failed to activate device: {}", e);
             return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
        (StatusCode::OK, "Activation successful").into_response()
    } else {
        tracing::warn!("Signature mismatch. Expected: {}, Got: {}", expected_signature, signature_to_verify);
        (StatusCode::ACCEPTED, "Device verification failed").into_response()
    }
}
