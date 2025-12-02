use std::sync::Arc;
use crate::config::ServerConfig;
use crate::traits::{LlmTrait, SttTrait, TtsTrait, DbTrait};
use crate::services::{
    llm::gemini::GeminiLlm,
    stt::sensevoice::SenseVoiceStt,
    tts::opus::OpusTts,
    db::{memory::InMemoryDb, sql::SqlDb},
};
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub db: Arc<dyn DbTrait + Send + Sync>,
    pub llm: Arc<dyn LlmTrait + Send + Sync>,
    pub stt: Arc<dyn SttTrait + Send + Sync>,
    pub tts: Arc<dyn TtsTrait + Send + Sync>,
}

impl AppState {
    pub async fn new(config: ServerConfig) -> Self {
        let db: Arc<dyn DbTrait + Send + Sync> = match config.db.db_type.as_str() {
            "sql" => {
                info!("Initializing SQL DB at {}", config.db.url);
                // Ensure the database file exists if it's sqlite
                if config.db.url.starts_with("sqlite://") {
                    let path = config.db.url.trim_start_matches("sqlite://");
                    if !std::path::Path::new(path).exists() {
                        info!("Creating database file: {}", path);
                        std::fs::File::create(path).expect("Failed to create DB file");
                    }
                }
                match SqlDb::new(&config.db.url).await {
                    Ok(d) => Arc::new(d),
                    Err(e) => {
                        panic!("Failed to connect to SQL DB: {}", e);
                    }
                }
            },
            _ => Arc::new(InMemoryDb::new()),
        };

        let llm = Arc::new(GeminiLlm::new(
            config.llm.api_key.clone(),
            config.llm.model.clone()
        ));
        let stt = Arc::new(SenseVoiceStt::new());
        let tts = Arc::new(OpusTts::new());

        Self {
            config: Arc::new(config),
            db,
            llm,
            stt,
            tts,
        }
    }
}
