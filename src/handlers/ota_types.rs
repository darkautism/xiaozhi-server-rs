use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug)]
pub struct OtaResponse {
    #[serde(rename = "websocket")]
    pub websocket: WebsocketInfo,
    #[serde(rename = "mqtt", skip_serializing_if = "Option::is_none")]
    pub mqtt: Option<MqttInfo>,
    #[serde(rename = "server_time")]
    pub server_time: ServerTimeInfo,
    #[serde(rename = "activation", skip_serializing_if = "Option::is_none")]
    pub activation: Option<ActivationInfo>,
    #[serde(rename = "firmware")]
    pub firmware: FirmwareInfo,
}

#[derive(Serialize, Debug)]
pub struct WebsocketInfo {
    #[serde(rename = "url")]
    pub url: String,
    #[serde(rename = "token")]
    pub token: String,
}

#[derive(Serialize, Debug)]
pub struct MqttInfo {
    #[serde(rename = "endpoint")]
    pub endpoint: String,
    #[serde(rename = "client_id")]
    pub client_id: String,
    #[serde(rename = "username")]
    pub username: String,
    #[serde(rename = "password")]
    pub password: String,
    #[serde(rename = "publish_topic")]
    pub publish_topic: String,
    #[serde(rename = "subscribe_topic")]
    pub subscribe_topic: String,
}

#[derive(Serialize, Debug)]
pub struct ServerTimeInfo {
    #[serde(rename = "timestamp")]
    pub timestamp: i64,
    #[serde(rename = "timeZone")]
    pub time_zone: String,
    #[serde(rename = "timezone_offset")]
    pub timezone_offset: i32,
}

#[derive(Serialize, Debug)]
pub struct ActivationInfo {
    #[serde(rename = "code")]
    pub code: String,
    #[serde(rename = "message")]
    pub message: String,
    #[serde(rename = "challenge")]
    pub challenge: String,
    #[serde(rename = "timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Serialize, Debug)]
pub struct FirmwareInfo {
    #[serde(rename = "version")]
    pub version: String,
    #[serde(rename = "url")]
    pub url: String,
}

#[derive(Deserialize, Debug)]
pub struct ActivationRequest {
    #[serde(rename = "Payload")]
    pub payload: ActivationPayload,
}

#[derive(Deserialize, Debug)]
pub struct ActivationPayload {
    #[serde(rename = "algorithm")]
    pub algorithm: String, // Expect "hmac-sha256"
    #[serde(rename = "challenge")]
    pub challenge: Option<String>, // It seems usually the challenge or random bytes are signed.
    #[serde(rename = "signature")]
    pub signature: Option<String>,
    #[serde(rename = "digest")] // Some implementations use digest
    pub digest: Option<String>,
    #[serde(rename = "device_id")]
    pub device_id: Option<String>,
}
