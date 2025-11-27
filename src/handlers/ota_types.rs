use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug)]
pub struct OtaResponse {
    #[serde(rename = "Websocket")]
    pub websocket: WebsocketInfo,
    #[serde(rename = "Mqtt", skip_serializing_if = "Option::is_none")]
    pub mqtt: Option<MqttInfo>,
    #[serde(rename = "ServerTime")]
    pub server_time: ServerTimeInfo,
    #[serde(rename = "Activation", skip_serializing_if = "Option::is_none")]
    pub activation: Option<ActivationInfo>,
    #[serde(rename = "Firmware")]
    pub firmware: FirmwareInfo,
}

#[derive(Serialize, Debug)]
pub struct WebsocketInfo {
    #[serde(rename = "Url")]
    pub url: String,
    #[serde(rename = "Token")]
    pub token: String,
}

#[derive(Serialize, Debug)]
pub struct MqttInfo {
    #[serde(rename = "Endpoint")]
    pub endpoint: String,
    #[serde(rename = "ClientId")]
    pub client_id: String,
    #[serde(rename = "Username")]
    pub username: String,
    #[serde(rename = "Password")]
    pub password: String,
    #[serde(rename = "PublishTopic")]
    pub publish_topic: String,
    #[serde(rename = "SubscribeTopic")]
    pub subscribe_topic: String,
}

#[derive(Serialize, Debug)]
pub struct ServerTimeInfo {
    #[serde(rename = "Timestamp")]
    pub timestamp: i64,
    #[serde(rename = "TimezoneOffset")]
    pub timezone_offset: i32,
}

#[derive(Serialize, Debug)]
pub struct ActivationInfo {
    #[serde(rename = "Code")]
    pub code: String,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "Challenge")]
    pub challenge: String,
    #[serde(rename = "TimeoutMs")]
    pub timeout_ms: u64,
}

#[derive(Serialize, Debug)]
pub struct FirmwareInfo {
    #[serde(rename = "Version")]
    pub version: String,
    #[serde(rename = "Url")]
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
