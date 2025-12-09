use crate::config::ServerConfig;
use crate::services::{
    db::{memory::InMemoryDb, sql::SqlDb},
    llm::{gemini::GeminiLlm, ollama::OllamaLlm, openai::OpenAiLlm},
    stt::sensevoice::SenseVoiceStt,
    tts::{edge::EdgeTts, gemini::GeminiTts, opus::OpusTts},
};
use crate::traits::{DbTrait, LlmTrait, SttTrait, TtsTrait};
use std::sync::Arc;
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
            }
            _ => Arc::new(InMemoryDb::new()),
        };

        let system_instruction = config.llm.system_instruction.clone();

        let llm: Arc<dyn LlmTrait + Send + Sync> = match config.llm.provider.as_str() {
            "gemini" => {
                if let Some(gemini_conf) = &config.llm.gemini {
                    Arc::new(GeminiLlm::new(
                        gemini_conf.api_key.clone(),
                        gemini_conf.model.clone(),
                        system_instruction,
                    ))
                } else {
                    // Fallback using top level (if present, but now discouraged) or defaults
                    // The old structure had api_key at top level, but new structure doesn't.
                    // This branch might panic if config is missing, but config loading should ensure validity or we handle it here.
                    // Given the migration, we assume valid config.
                    panic!("Gemini provider selected but [llm.gemini] config missing.");
                }
            }
            "openai" => {
                if let Some(openai_conf) = &config.llm.openai {
                    Arc::new(OpenAiLlm::new(
                        openai_conf.api_key.clone(),
                        openai_conf.model.clone(),
                        system_instruction,
                        openai_conf.base_url.clone(),
                    ))
                } else {
                    panic!("OpenAI provider selected but [llm.openai] config missing.");
                }
            }
            "ollama" => {
                if let Some(ollama_conf) = &config.llm.ollama {
                    Arc::new(OllamaLlm::new(
                        ollama_conf.model.clone(),
                        system_instruction,
                        ollama_conf.base_url.clone(),
                    ))
                } else {
                    panic!("Ollama provider selected but [llm.ollama] config missing.");
                }
            }
            provider => {
                panic!("Unknown LLM provider: {}", provider);
            }
        };

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
            }
            "gemini" => {
                if let Some(gemini_config) = &config.tts.gemini {
                    let api_key = gemini_config
                        .api_key
                        .clone()
                        .or_else(|| {
                            // Try to get key from llm.gemini if available
                            config.llm.gemini.as_ref().map(|g| g.api_key.clone())
                        })
                        .expect("Gemini TTS requires an API key in [tts.gemini] or [llm.gemini]");

                    Arc::new(GeminiTts::new(
                        api_key,
                        gemini_config.model.clone(),
                        gemini_config.voice_name.clone(),
                    ))
                } else {
                    panic!("Gemini TTS selected but [tts.gemini] config missing.");
                }
            }
            "opus" => Arc::new(OpusTts::new()),
            &_ => todo!(),
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
