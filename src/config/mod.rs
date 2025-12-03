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
    #[serde(default)]
    pub vad: VadSettings,
    #[serde(default)]
    pub chat: ChatSettings,
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
    #[serde(default = "default_history_limit")]
    pub history_limit: usize,
    pub system_instruction: Option<String>,
}

fn default_llm_model() -> String {
    "gemini-3-pro-preview".to_string()
}

fn default_history_limit() -> usize {
    5
}

#[derive(Debug, Deserialize)]
pub struct SttSettings {
    pub provider: String,
}

#[derive(Debug, Deserialize)]
pub struct TtsSettings {
    pub provider: String,
    #[serde(default)]
    pub gemini: Option<GeminiTtsConfig>,
    #[serde(default)]
    pub edge: Option<EdgeTtsConfig>,
}

#[derive(Debug, Deserialize)]
pub struct GeminiTtsConfig {
    pub api_key: Option<String>,
    #[serde(default = "default_tts_model")]
    pub model: String,
    #[serde(default = "default_tts_voice")]
    pub voice_name: String,
}

#[derive(Debug, Deserialize)]
pub struct EdgeTtsConfig {
    #[serde(default = "default_edge_voice")]
    pub voice: String,
    #[serde(default = "default_edge_rate")]
    pub rate: String,
    #[serde(default = "default_edge_pitch")]
    pub pitch: String,
    #[serde(default = "default_edge_volume")]
    pub volume: String,
}

fn default_tts_model() -> String {
    "gemini-2.5-flash-preview-tts".to_string()
}

fn default_tts_voice() -> String {
    "Kore".to_string()
}

fn default_edge_voice() -> String {
    "zh-TW-HsiaoChenNeural".to_string()
}

fn default_edge_rate() -> String {
    "+0%".to_string()
}

fn default_edge_pitch() -> String {
    "+0Hz".to_string()
}

fn default_edge_volume() -> String {
    "+0%".to_string()
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

#[derive(Debug, Deserialize, Default, Clone)]
pub struct VadSettings {
    #[serde(default = "default_silence_duration")]
    pub silence_duration_ms: u32,
}

fn default_silence_duration() -> u32 {
    2500
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct ChatSettings {
    #[serde(default = "default_max_idle_duration")]
    pub max_idle_duration: u64,
    #[serde(default = "default_standby_prompt")]
    pub standby_prompt: String,
}

fn default_max_idle_duration() -> u64 {
    30000
}

fn default_standby_prompt() -> String {
    "請問你還在嗎？".to_string()
}

impl ServerConfig {
    pub fn new() -> Result<Self, config::ConfigError> {
        let builder = config::Config::builder()
            .add_source(config::File::with_name("Settings.toml").required(false))
            .add_source(config::Environment::with_prefix("XIAOZHI").separator("__"));

        builder.build()?.try_deserialize()
    }
}
