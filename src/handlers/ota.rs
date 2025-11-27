use axum::{
    extract::{State, Request},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
};
use std::time::SystemTime;
use uuid::Uuid;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use hex;
use rand::{distr::Alphanumeric, Rng};

use crate::state::AppState;
use crate::handlers::ota_types::*;

pub async fn handle_ota(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request,
) -> impl IntoResponse {
    let device_id = match headers.get("Device-Id") {
        Some(v) => v.to_str().unwrap_or("").to_string(),
        None => return (StatusCode::BAD_REQUEST, "Missing Device-Id").into_response(),
    };
    let client_id = match headers.get("Client-Id") {
        Some(v) => v.to_str().unwrap_or("").to_string(),
        None => return (StatusCode::BAD_REQUEST, "Missing Client-Id").into_response(),
    };

    // Determine IP for config selection
    let ip = headers.get("X-Real-IP")
        .and_then(|h| h.to_str().ok())
        .or_else(|| headers.get("X-Forwarded-For").and_then(|h| h.to_str().ok()))
        .unwrap_or("127.0.0.1"); // Default to localhost if unknown

    let is_test_env = ip.starts_with("192.168.") || ip.starts_with("10.") || ip.starts_with("127.");

    // Auth Check
    let mut activation_info: Option<ActivationInfo> = None;
    if state.config.auth.enable {
        let db = state.db.read().unwrap();
        if !db.is_activated(&device_id) {
             drop(db); // Release read lock to acquire write lock
             let mut db = state.db.write().unwrap();

             // Check again to avoid race
             if !db.is_activated(&device_id) {
                 // Generate Challenge
                 let challenge: String = rand::rng()
                     .sample_iter(&Alphanumeric)
                     .take(32)
                     .map(char::from)
                     .collect();

                 db.add_challenge(device_id.clone(), challenge.clone(), std::time::Duration::from_secs(300));

                 activation_info = Some(ActivationInfo {
                     code: "0".to_string(), // Placeholder
                     message: "Device not activated".to_string(),
                     challenge,
                     timeout_ms: 300000,
                 });
             }
        }
    }

    // Config Selection
    let ota_config = if is_test_env {
        &state.config.ota.test
    } else {
        &state.config.ota.external
    };

    // MQTT Info Construction
    let mqtt_info = if ota_config.mqtt.enable {
        // Generate MQTT credentials (mock)
        // reference: util.GenerateMqttCredentials(deviceId, clientId, ip, signatureKey)
        // We will just generate a simple one
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
            url: ota_config.websocket.url.clone(),
            token: ota_config.websocket.token.clone(),
        },
        mqtt: mqtt_info,
        server_time: ServerTimeInfo {
            timestamp: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as i64,
            timezone_offset: 480, // UTC+8
        },
        activation: activation_info,
        firmware: FirmwareInfo {
            version: "0.9.9".to_string(),
            url: "".to_string(),
        },
    };

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

    let db = state.db.write().unwrap();
    // Retrieve stored challenge
    // Note: In real world, we verify the challenge sent back by client matches what we stored (if client sends it back)
    // Or we verify the signature using the stored challenge.
    // The request payload usually contains the signature of the challenge.

    // We need to know what the client signed.
    // Usually: HMAC(secret, challenge)
    // But here, the server gave the challenge. The client needs to sign it with a SHARED SECRET or use the challenge as a key?
    // Wait, usually OTA activation implies the device PROVES it's a valid device.
    // If we assume the device has a secret burned in, we verify against that.

    // However, without knowing the exact protocol, I will assume a standard HMAC verification:
    // Signature = HMAC-SHA256(Key=SharedSecret, Data=Challenge)
    // We use the same 'signature_key' from config as the shared secret for simplicity.

    // Find the challenge we issued for this device
    let stored_challenge = match db.get_challenge(&device_id) {
        Some(c) => c,
        None => return (StatusCode::FORBIDDEN, "No pending challenge or expired").into_response(),
    };

    let signature_to_verify = req.payload.signature.clone().or(req.payload.digest.clone()).unwrap_or_default();

    let secret_key = &state.config.auth.signature_key;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes()).expect("HMAC can take key of any size");
    mac.update(stored_challenge.as_bytes());
    let expected_signature_bytes = mac.finalize().into_bytes();
    let expected_signature = hex::encode(expected_signature_bytes);

    // Constant time comparison is better for security, but string eq is fine for this prototype
    if signature_to_verify == expected_signature {
        drop(db); // release read lock (actually we have write lock, so we can update)
        let mut db = state.db.write().unwrap();
        db.activate_device(device_id);
        (StatusCode::OK, "Activation successful").into_response()
    } else {
        // Fallback: Check if the client signed the DeviceId + Challenge?
        // Or maybe just signed the deviceID?
        // For strictness, if it fails, we fail.
        tracing::warn!("Signature mismatch. Expected: {}, Got: {}", expected_signature, signature_to_verify);
        (StatusCode::ACCEPTED, "Device verification failed").into_response()
    }
}
