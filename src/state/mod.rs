use std::sync::Arc;
use crate::config::ServerConfig;
use crate::traits::{LlmTrait, SttTrait, TtsTrait, DbTrait};
use crate::services::{
    llm::gemini::GeminiLlm,
    stt::sensevoice::SenseVoiceStt,
    tts::{opus::OpusTts, local::LocalTts, gemini::GeminiTts, edge::EdgeTts},
    db::{memory::InMemoryDb, sql::SqlDb},
};
use tracing::{info, warn};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub db: Arc<dyn DbTrait + Send + Sync>,
    pub llm: Arc<dyn LlmTrait + Send + Sync>,
    pub stt: Arc<dyn SttTrait + Send + Sync>,
    pub tts: Arc<dyn TtsTrait + Send + Sync>,
    pub history_limit: usize,
}

impl AppState {
    pub async fn new(config: ServerConfig) -> Self {
        let db: Arc<dyn DbTrait + Send + Sync> = match config.db.db_type.as_str() {
            "sql" => {
                info!("Initializing SQL DB at {}", config.db.url);
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
            config.llm.model.clone(),
            config.llm.system_instruction.clone(),
        ));

        let stt = Arc::new(SenseVoiceStt::new(config.vad.clone()));

        let tts: Arc<dyn TtsTrait + Send + Sync> = match config.tts.provider.as_str() {
            "edge" => {
                 if let Some(edge_config) = &config.tts.edge {
                     Arc::new(EdgeTts::new(
                         edge_config.voice.clone(),
                         edge_config.rate.clone(),
                         edge_config.pitch.clone(),
                         edge_config.volume.clone(),
                     ))
                 } else {
                     warn!("Edge TTS selected but no specific config found. Using defaults.");
                     Arc::new(EdgeTts::new(
                         "zh-TW-HsiaoChenNeural".to_string(),
                         "+0%".to_string(),
                         "+0Hz".to_string(),
                         "+0%".to_string(),
                     ))
                 }
            },
            "gemini" => {
                if let Some(gemini_config) = &config.tts.gemini {
                    let api_key = gemini_config.api_key.clone().unwrap_or_else(|| config.llm.api_key.clone());
                    Arc::new(GeminiTts::new(
                        api_key,
                        gemini_config.model.clone(),
                        gemini_config.voice_name.clone()
                    ))
                } else {
                    // Fallback to defaults using LLM key if config missing
                     warn!("Gemini TTS selected but no specific config found. Using defaults and LLM API key.");
                     Arc::new(GeminiTts::new(
                         config.llm.api_key.clone(),
                         "gemini-2.5-flash-preview-tts".to_string(),
                         "Kore".to_string()
                     ))
                }
            },
            "opus" => Arc::new(OpusTts::new()),
            "local" | _ => Arc::new(LocalTts::new()),
        };

        let history_limit = config.llm.history_limit;

        Self {
            config: Arc::new(config),
            db,
            llm,
            stt,
            tts,
            history_limit,
        }
    }
}
