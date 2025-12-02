use crate::traits::LlmTrait;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use anyhow::{Context, Result};

pub struct GeminiLlm {
    api_key: String,
    client: Client,
    model: String,
}

impl GeminiLlm {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
            model,
        }
    }
}

#[async_trait]
impl LlmTrait for GeminiLlm {
    async fn chat(&self, text: &str) -> Result<String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let body = json!({
            "contents": [{
                "parts": [{
                    "text": text
                }]
            }]
        });

        let resp = self.client.post(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Gemini")?;

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
