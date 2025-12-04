# Xiaozhi Server (小智伺服器)


[English](README.md)
[中文](README.zh-TW.md)

這是一個個人使用的 Xiaozhi 機器人伺服器端實作，旨在提供開箱即用的個人伺服器解決方案。
透過整合 LLM、STT 與 TTS 技術，實現與硬體裝置（如 ESP32-S3）的語音互動。

**⚠️ 免責聲明：** 本專案僅供個人學習與娛樂使用，並非產品級伺服器。架構設計不支援多使用者高並發，請勿用於生產環境或多人共享服務。

## 功能特性

- **語音轉文字 (STT):**
  - **SenseVoice** (預設): 使用 `sensevoice-rs` 運行於本地的高準確度語音識別模型。
- **大語言模型 (LLM):**
  - **Google Gemini**: 支援多輪對話與上下文記憶（Context Aware）。
- **文字轉語音 (TTS):**
  - **Microsoft Edge TTS** (預設): 使用 `msedge-tts`，提供自然且免費的語音合成。
  - **Google Gemini TTS**: 使用 Gemini 的語音生成能力。延遲過高，不推薦使用。
  - *Opus*: 測試用。
- **OTA 更新:** 內建 OTA 伺服器，支援裝置韌體更新與啟用（Activation）流程。
- **對話記憶:** 支援 In-Memory 或 SQLite 資料庫儲存對話歷史。

## 系統需求

- **作業系統:** Linux (建議) 或 Windows
- **Rust:** 版本 1.75 或更高
- **依賴庫:** `libonnxruntime` (用於 SenseVoice STT)

## 安裝指南

### Linux 環境

1.  **安裝編譯工具:**
    ```bash
    sudo apt-get update
    sudo apt-get install cmake clang libclang-dev pkg-config libssl-dev
    ```

2.  **安裝 ONNX Runtime:**
    專案依賴 `onnxruntime` v1.23.2。
    ```bash
    # 下載並解壓
    curl -L https://github.com/microsoft/onnxruntime/releases/download/v1.23.2/onnxruntime-linux-x64-1.23.2.tgz | tar zxv
    
    # 進入目錄
    cd onnxruntime-linux-x64-1.23.2
    
    # 複製庫文件到系統庫目錄
    sudo cp lib/libonnxruntime.so* /usr/local/lib/
    
    # 更新 linker cache
    sudo ldconfig
    ```

3.  **編譯與執行:**
    ```bash
    git clone <repository_url>
    cd xiaozhi-server
    cargo run --release
    ```

### Windows 環境

1.  **安裝 Rust:** 請至 [rust-lang.org](https://www.rust-lang.org/) 下載並安裝。
2.  **下載 ONNX Runtime:**
    - 前往 [Microsoft ONNX Runtime Releases](https://github.com/microsoft/onnxruntime/releases).
    - 下載 `onnxruntime-win-x64-1.23.2.zip`.
    - 解壓縮後，找到 `lib/onnxruntime.dll`。
    - **重要:** 將 `onnxruntime.dll` 複製到本專案的根目錄下（與 `Cargo.toml` 同層），或放置於系統 `PATH` 路徑中。
3.  **編譯與執行:**
    ```powershell
    cargo run --release
    ```

## 配置說明 (settings.toml)

專案根目錄下的 `settings.toml` 為主要設定檔。請依據需求修改：

```toml
[server]
port = 8002           # 伺服器監聽端口
host = "0.0.0.0"      # 綁定位址

[auth]
enable = false        # 是否啟用裝置驗證
signature_key = "..." # HMAC 簽章密鑰

[llm]
provider = "gemini"
api_key = "YOUR_GEMINI_API_KEY"       # 請填入 Gemini API Key
model = "gemini-2.0-flash-lite"
history_limit = 5                     # 對話記憶輪數
system_instruction = "..."            # 系統提示詞 (System Prompt)

[stt]
provider = "sensevoice"

[tts]
provider = "edge"

[tts.edge]
voice = "zh-TW-HsiaoChenNeural" # 語音角色
rate = "+0%"                    # 語速
pitch = "+0Hz"                  # 音調
volume = "+0%"                  # 音量

[db]
type = "memory"                 # "memory" (不保存) 或 "sql" (保存)
# url = "sqlite://xiaozhi.db"   # 若使用 sql 需指定路徑

[vad]
silence_duration_ms = 2500      # 靜音檢測閾值 (毫秒)
```

## LICENSE

MIT