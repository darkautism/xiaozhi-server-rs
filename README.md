# Xiaozhi Server (小智伺服器)

[English](README.md)  
[中文](README.zh-TW.md)

This is a personal server-side implementation of the Xiaozhi bot, designed to provide a ready-to-run personal server solution. By integrating LLM, STT, and TTS technologies, it enables voice interaction with hardware devices (e.g., ESP32-S3).

**⚠️ Disclaimer:** This project is intended for personal learning and entertainment only and is not a production-grade server. The architecture is not designed for multi-user high concurrency; do not use it in production environments or as a shared multi-user service.

---

## Features

- **Speech-to-Text (STT):**  
  - **SenseVoice** (default): A high-accuracy speech recognition model running locally via `sensevoice-rs`.
- **Large Language Model (LLM):**  
  - **Google Gemini:** Supports multi-turn conversations and context awareness.
- **Text-to-Speech (TTS):**  
  - **Microsoft Edge TTS** (default): Uses `msedge-tts` to provide natural and free speech synthesis.  
  - **Google Gemini TTS:** Uses Gemini’s voice generation capability. High latency; not recommended.  
  - *Opus*: For testing.
- **OTA Updates:** Built-in OTA server supporting device firmware updates and activation flows.
- **Conversation Memory:** Supports storing conversation history in In-Memory or SQLite database.

---

## System Requirements

- **Operating System:** Linux (recommended) or Windows  
- **Rust:** Version 1.75 or newer  
- **Dependencies:** `libonnxruntime` (used by SenseVoice STT)

---

## Installation Guide

### Linux

1. **Install build tools:**
    ```bash
    sudo apt-get update
    sudo apt-get install cmake clang libclang-dev pkg-config libssl-dev
    ```

2. **Install ONNX Runtime:**  
   The project depends on `onnxruntime` v1.23.2.
    ```bash
    # Download and extract
    curl -L https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-linux-x64-1.23.2.tgz | tar zxv

    # Enter directory
    cd onnxruntime-linux-x64-1.23.2

    # Copy library files to system library directory
    sudo cp lib/libonnxruntime.so* /usr/local/lib/

    # Update linker cache
    sudo ldconfig
    ```

3. **Build and run:**
    ```bash
    git clone <repository_url>
    cd xiaozhi-server
    cargo run --release
    ```

### Windows

1. **Install Rust:** Download and install from [rust-lang.org](https://www.rust-lang.org/).  
2. **Download ONNX Runtime:**  
   - Visit [Microsoft ONNX Runtime Releases](https://github.com/microsoft/onnxruntime/releases).  
   - Download `onnxruntime-win-x64-1.23.2.zip`.  
   - After extracting, locate `target/debug/onnxruntime.dll` or `target/release/onnxruntime.dll`.  
   - **Important:** Copy `onnxruntime.dll` to the project root (same level as `Cargo.toml`) or place it in a directory included in the system `PATH`.  
3. **Build and run:**
    ```powershell
    cargo run --release
    ```

---

## Configuration (settings.toml)

The main configuration file is `settings.toml` in the project root. Modify as needed:

```toml
[server]
port = 8002           # Server listening port
host = "0.0.0.0"      # Bind address

[auth]
enable = false        # Enable device authentication
signature_key = "..." # HMAC signature key

[llm]
provider = "gemini"
api_key = "YOUR_GEMINI_API_KEY"       # Fill in your Gemini API Key
model = "gemini-2.0-flash-lite"
history_limit = 5                     # Number of conversation history turns
system_instruction = "..."            # System prompt

[stt]
provider = "sensevoice"

[tts]
provider = "edge"

[tts.edge]
voice = "zh-TW-HsiaoChenNeural" # Voice persona
rate = "+0%"                    # Speaking rate
pitch = "+0Hz"                  # Pitch
volume = "+0%"                  # Volume

[db]
type = "memory"                 # "memory" (no persistence) or "sql" (persistent)
# url = "sqlite://xiaozhi.db"   # Specify path if using sql

[vad]
silence_duration_ms = 2500      # Silence detection threshold (milliseconds)
```

---

## LICENSE

MIT
