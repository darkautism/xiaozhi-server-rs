use crate::services::llm::TECH_INSTRUCTION;
use crate::traits::{LlmTrait, Message};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::info;

pub struct OpenAiLlm {
    api_key: String,
    client: Client,
    model: String,
    system_instruction: Option<String>,
    base_url: String,
}

impl OpenAiLlm {
    pub fn new(
        api_key: String,
        model: String,
        system_instruction: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        // Merge user instruction with shared technical instruction
        let final_instruction = match system_instruction {
            Some(user_inst) => format!("{} {}", user_inst, TECH_INSTRUCTION),
            None => TECH_INSTRUCTION.to_string(),
        };

        let base = base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        // Ensure base_url doesn't end with slash for cleaner appending
        let clean_base = base.trim_end_matches('/').to_string();

        Self {
            api_key,
            client: Client::new(),
            model,
            system_instruction: Some(final_instruction),
            base_url: clean_base,
        }
    }
}

#[async_trait]
impl LlmTrait for OpenAiLlm {
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

        // Map internal messages to OpenAI format
        for msg in messages {
            request_messages.push(json!({
                "role": msg.role,
                "content": msg.content
            }));
        }

        let body = json!({
            "model": self.model,
            "messages": request_messages,
        });

        info!("Sending request to OpenAI model: {} at {}", self.model, self.base_url);
        
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .context("Failed to send request to OpenAI")?;
            
        info!("OpenAI response status: {}", resp.status());

        if !resp.status().is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("OpenAI API error: {}", error_text));
        }

        let json: Value = resp
            .json()
            .await
            .context("Failed to parse OpenAI response")?;

        // Extract text from response structure: choices[0].message.content
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .context("Invalid response format from OpenAI")?;

        Ok(content.to_string())
    }
}
