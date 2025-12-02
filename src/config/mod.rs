use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub server: ServerSettings,
    pub auth: AuthSettings,
    pub ota: OtaSettings,
    pub llm: LlmSettings,
    pub stt: SttSettings,
    pub tts: TtsSettings,
    #[serde(default)]
    pub db: DbSettings,
}

#[derive(Debug, Deserialize)]
pub struct ServerSettings {
    pub port: u16,
    pub host: String,
}

#[derive(Debug, Deserialize)]
pub struct AuthSettings {
    pub enable: bool,
    pub signature_key: String,
}

#[derive(Debug, Deserialize)]
pub struct OtaSettings {
    pub firmware_version: String,
    pub websocket_url: Option<String>,
    pub websocket_token: Option<String>,
    pub mqtt: MqttConfig,
}

#[derive(Debug, Deserialize)]
pub struct MqttConfig {
    pub enable: bool,
    pub endpoint: String,
}

#[derive(Debug, Deserialize)]
pub struct LlmSettings {
    pub provider: String,
    pub api_key: String,
    #[serde(default = "default_llm_model")]
    pub model: String,
}

fn default_llm_model() -> String {
    "gemini-3-pro-preview".to_string()
}

#[derive(Debug, Deserialize)]
pub struct SttSettings {
    pub provider: String,
}

#[derive(Debug, Deserialize)]
pub struct TtsSettings {
    pub provider: String,
}

#[derive(Debug, Deserialize)]
pub struct DbSettings {
    #[serde(rename = "type")]
    pub db_type: String,
    #[serde(default = "default_db_url")]
    pub url: String,
}

fn default_db_url() -> String {
    "sqlite://xiaozhi.db".to_string()
}

impl Default for DbSettings {
    fn default() -> Self {
        Self {
            db_type: "memory".to_string(),
            url: default_db_url(),
        }
    }
}

impl ServerConfig {
    pub fn new() -> Result<Self, config::ConfigError> {
        let builder = config::Config::builder()
            .add_source(config::File::with_name("Settings.toml").required(false))
            .add_source(config::Environment::with_prefix("XIAOZHI").separator("__"));

        builder.build()?.try_deserialize()
    }
}
