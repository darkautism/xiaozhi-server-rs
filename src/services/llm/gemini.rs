use crate::traits::{LlmTrait, Message};
use async_trait::async_trait;
use reqwest::Client;
use tracing::info;
use serde_json::{json, Value};
use anyhow::{Context, Result};

pub struct GeminiLlm {
    api_key: String,
    client: Client,
    model: String,
    system_instruction: Option<String>,
}

impl GeminiLlm {
    pub fn new(api_key: String, model: String, system_instruction: Option<String>) -> Self {
        Self {
            api_key,
            client: Client::new(),
            model,
            system_instruction,
        }
    }
}

#[async_trait]
impl LlmTrait for GeminiLlm {
    async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        // Map internal Message to Gemini Content format
        let contents: Vec<Value> = messages.iter().map(|msg| {
            json!({
                "role": msg.role,
                "parts": [{ "text": msg.content }]
            })
        }).collect();

        let mut body = json!({
            "contents": contents
        });

        // Add system_instruction if present
        if let Some(instruction) = &self.system_instruction {
            body["system_instruction"] = json!({
                "parts": [{ "text": instruction }]
            });
        }

        info!("Sending request to Gemini model: {}", self.model);
        let resp = self.client.post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .context("Failed to send request to Gemini")?;
        info!("Gemini response status: {}", resp.status());

        if !resp.status().is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Gemini API error: {}", error_text));
        }

        let json: Value = resp.json().await.context("Failed to parse Gemini response")?;

        // Extract text from response structure: candidates[0].content.parts[0].text
        let content = json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .context("Invalid response format from Gemini")?;

        Ok(content.to_string())
    }
}
