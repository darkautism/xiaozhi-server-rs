pub mod gemini;
pub mod openai;
pub mod ollama;

pub const TECH_INSTRUCTION: &str = "If the user indicates they want you to sleep, stop, or shut up, please politely reply that you are taking a break and append the [SLEEP] tag to the end of your response.";
