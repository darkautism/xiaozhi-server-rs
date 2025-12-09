use crate::services::llm::TECH_INSTRUCTION;
use crate::traits::{ChatResponse, LlmTrait, Message, ToolDefinition};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{info, warn};

pub struct GeminiLlm {
    api_key: String,
    client: Client,
    model: String,
    system_instruction: Option<String>,
}

impl GeminiLlm {
    pub fn new(api_key: String, model: String, system_instruction: Option<String>) -> Self {
        // Merge user instruction with shared technical instruction
        let final_instruction = match system_instruction {
            Some(user_inst) => format!("{} {}", user_inst, TECH_INSTRUCTION),
            None => TECH_INSTRUCTION.to_string(),
        };

        Self {
            api_key,
            client: Client::new(),
            model,
            system_instruction: Some(final_instruction),
        }
    }
}

#[async_trait]
impl LlmTrait for GeminiLlm {
    async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<ChatResponse> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        // Map internal Message to Gemini Content format
        let contents: Vec<Value> = messages
            .iter()
            .map(|msg| {
                let role = if msg.role == "assistant" { "model" } else if msg.role == "tool" { "function" } else { &msg.role };

                // If it's a tool response (role "tool" in OpenAI, "function" in Gemini)
                if role == "function" {
                     json!({
                        "role": "function",
                        "parts": [{
                            "functionResponse": {
                                "name": msg.tool_call_id.clone().unwrap_or("unknown".to_string()), // Gemini uses name, OpenAI uses ID. Mapping might be tricky if we don't have name stored in ID or separate field.
                                // Wait, Gemini functionResponse needs 'name' of the function, and 'response' object.
                                // Our Message struct has 'tool_call_id' which is usually the ID.
                                // If we don't store the function name in history for tool results, Gemini might complain.
                                // For now, let's assume tool_call_id holds the name, or we need to fix Message struct to store tool name.
                                // But let's look at standard OpenAI tool usage: tool_call_id matches the call.
                                // Actually, for Gemini, the "role" is "function" and part is "functionResponse".
                                // { "functionResponse": { "name": "...", "response": { ... } } }
                                "response": { "content": msg.content } 
                            }
                        }]
                    })
                } else {
                    // User or Model
                    let mut parts = Vec::new();
                    if !msg.content.is_empty() {
                        parts.push(json!({ "text": msg.content }));
                    }
                    if !msg.tool_calls.is_empty() {
                        for tc in &msg.tool_calls {
                            parts.push(json!({
                                "functionCall": {
                                    "name": tc.function.name,
                                    "args": serde_json::from_str::<Value>(&tc.function.arguments).unwrap_or(json!({}))
                                }
                            }));
                        }
                    }
                    json!({
                        "role": role,
                        "parts": parts
                    })
                }
            })
            .collect();

        let mut body = json!({
            "contents": contents
        });

        // Add system_instruction if present
        if let Some(instruction) = &self.system_instruction {
            body["system_instruction"] = json!({
                "parts": [{ "text": instruction }]
            });
        }

        if let Some(tools) = tools {
            if !tools.is_empty() {
                let gemini_tools: Vec<Value> = tools
                    .iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters // Gemini supports standard JSON schema in 'parameters'
                        })
                    })
                    .collect();

                body["tools"] = json!([{
                    "function_declarations": gemini_tools
                }]);
            }
        }

        info!("Sending request to Gemini model: {}", self.model);
        tracing::debug!(
            "[LLM DUMP] Request Body: {}",
            serde_json::to_string_pretty(&body).unwrap_or_default()
        );

        let resp = self
            .client
            .post(&url)
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

        let json: Value = resp
            .json()
            .await
            .context("Failed to parse Gemini response")?;

        tracing::debug!(
            "[LLM DUMP] Response Body: {}",
            serde_json::to_string_pretty(&json).unwrap_or_default()
        );

        let candidate = &json["candidates"][0];
        let parts = &candidate["content"]["parts"];

        if let Some(parts_array) = parts.as_array() {
            // Check for function calls
            let mut tool_calls = Vec::new();
            let mut text_content = String::new();

            for part in parts_array {
                if let Some(func_call) = part.get("functionCall") {
                    let name = func_call["name"].as_str().unwrap_or("").to_string();
                    let args = &func_call["args"];
                    tool_calls.push(crate::traits::ToolCall {
                        id: name.clone(), // Gemini doesn't use IDs, so use name as ID? Or generate one? OpenAI uses IDs.
                        type_: "function".to_string(),
                        function: crate::traits::ToolFunction {
                            name,
                            arguments: args.to_string(),
                        },
                    });
                }
                if let Some(text) = part.get("text") {
                    text_content.push_str(text.as_str().unwrap_or(""));
                }
            }

            if !tool_calls.is_empty() {
                return Ok(ChatResponse::ToolCall(tool_calls));
            }
            return Ok(ChatResponse::Text(text_content));
        }

        Err(anyhow::anyhow!("Invalid response format from Gemini"))
    }
}
