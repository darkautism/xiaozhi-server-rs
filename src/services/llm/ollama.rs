use crate::services::llm::TECH_INSTRUCTION;
use crate::traits::{LlmTrait, Message};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::info;

pub struct OllamaLlm {
    client: Client,
    model: String,
    system_instruction: Option<String>,
    base_url: String,
}

impl OllamaLlm {
    pub fn new(
        model: String,
        system_instruction: Option<String>,
        base_url: String,
    ) -> Self {
        // Merge user instruction with shared technical instruction
        let final_instruction = match system_instruction {
            Some(user_inst) => format!("{} {}", user_inst, TECH_INSTRUCTION),
            None => TECH_INSTRUCTION.to_string(),
        };

        // Ensure base_url doesn't end with slash
        let clean_base = base_url.trim_end_matches('/').to_string();

        Self {
            client: Client::new(),
            model,
            system_instruction: Some(final_instruction),
            base_url: clean_base,
        }
    }
}

#[async_trait]
impl LlmTrait for OllamaLlm {
    async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut request_messages = Vec::new();

        // Add system instruction as the first message if present
        if let Some(instruction) = &self.system_instruction {
            request_messages.push(json!({
                "role": "system",
                "content": instruction
            }));
        }

        // Map internal messages to OpenAI/Ollama format
        for msg in messages {
            request_messages.push(json!({
                "role": msg.role,
                "content": msg.content
            }));
        }

        let body = json!({
            "model": self.model,
            "messages": request_messages,
            "stream": false 
        });

        info!("Sending request to Ollama model: {} at {}", self.model, self.base_url);
        
        // Ollama doesn't typically require an API key
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(60)) // Ollama can be slow on CPU
            .send()
            .await
            .context("Failed to send request to Ollama")?;
            
        info!("Ollama response status: {}", resp.status());

        if !resp.status().is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Ollama API error: {}", error_text));
        }

        let json: Value = resp
            .json()
            .await
            .context("Failed to parse Ollama response")?;

        // Extract text from response structure: choices[0].message.content
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .context("Invalid response format from Ollama")?;

        Ok(content.to_string())
    }
}
