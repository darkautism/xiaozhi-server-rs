use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub server: ServerSettings,
    pub auth: AuthSettings,
    pub ota: OtaSettings,
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
    pub external: OtaEnvironment,
    pub test: OtaEnvironment,
}

#[derive(Debug, Deserialize)]
pub struct OtaEnvironment {
    pub websocket: WebsocketConfig,
    pub mqtt: MqttConfig,
}

#[derive(Debug, Deserialize)]
pub struct WebsocketConfig {
    pub url: String,
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct MqttConfig {
    pub enable: bool,
    pub endpoint: String,
}

impl ServerConfig {
    pub fn new() -> Result<Self, config::ConfigError> {
        let builder = config::Config::builder()
            .add_source(config::File::with_name("Settings.toml").required(false))
            .add_source(config::Environment::with_prefix("XIAOZHI").separator("__"));

        builder.build()?.try_deserialize()
    }
}
