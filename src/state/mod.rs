use std::sync::Arc;
use crate::config::ServerConfig;
use crate::traits::{LlmTrait, SttTrait, TtsTrait, DbTrait};
use crate::services::{
    llm::gemini::GeminiLlm,
    stt::local::LocalStt,
    tts::local::LocalTts,
    db::{memory::InMemoryDb, sql::SqlDb},
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub db: Arc<dyn DbTrait + Send + Sync>,
    pub llm: Arc<dyn LlmTrait + Send + Sync>,
    pub stt: Arc<dyn SttTrait + Send + Sync>,
    pub tts: Arc<dyn TtsTrait + Send + Sync>,
}

impl AppState {
    pub fn new(config: ServerConfig) -> Self {
        let db: Arc<dyn DbTrait + Send + Sync> = match config.db.db_type.as_str() {
            "sql" => Arc::new(SqlDb::new()),
            _ => Arc::new(InMemoryDb::new()),
        };

        // In a real app, we'd switch based on config.llm.provider, etc.
        let llm = Arc::new(GeminiLlm::new(config.llm.api_key.clone()));
        let stt = Arc::new(LocalStt::new());
        let tts = Arc::new(LocalTts::new());

        Self {
            config: Arc::new(config),
            db,
            llm,
            stt,
            tts,
        }
    }
}
