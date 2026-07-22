# AI Code Agent (Beta)

一個執行在你本機的 VS Code AI 編碼助手擴充功能，支援 **DeepSeek**、**OpenAI**、**Anthropic (Claude)** 及 **Ollama** 等多種 AI 供應商。

> ⚠️ **Beta 階段**：此擴充功能仍在測試中，功能可能變動，歡迎回報問題。

---

## 架構

```
┌─────────────────────────────────────┐
│         VS Code 擴充功能              │
│  ┌───────────────────────────────┐  │
│  │  Webview UI (聊天介面)        │  │
│  └───────────┬───────────────────┘  │
│              │ JSON-RPC (stdio)      │
│  ┌───────────▼───────────────────┐  │
│  │  Rust Sidecar (ai-agent.exe)  │  │
│  │  - AI Provider 路由            │  │
│  │  - 工具呼叫 (Tool Calls)       │  │
│  │  - 對話管理                    │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
```

擴充功能由兩部分組成：

- **TypeScript 前端**（`vscode-ext/`）：負責 VS Code 整合、設定同步、Webview UI
- **Rust 後端**（`src/`）：作為 sidecar 行程執行，處理 AI API 呼叫、工具執行、對話管理

---

## 支援的 AI 供應商

| 供應商 | 設定值 | 需要 API Key | 支援原生 Tool Calls |
|---|---|---|---|
| DeepSeek | `deepseek` | ✅ | ✅ |
| OpenAI | `openai` | ✅ | ✅ |
| Anthropic | `anthropic` | ✅ | ✅ |
| Ollama | `ollama` | ❌（本機） | ❌ |

---

## 功能

- 🗨️ **對話式聊天介面**：在 VS Code 側邊欄直接與 AI 對話
- 🔧 **自動工具呼叫**：AI 可以讀取檔案、修改程式碼、執行指令、搜尋專案
- 📂 **工作區感知**：AI 能瀏覽和理解你的整個專案結構
- 🌊 **串流回應**：即時顯示 AI 生成內容
- 🧠 **思考模式（Reasoning）**：AI 回覆前先進行分析與規劃（支援 DeepSeek reasoning）
- 🔄 **自動重試**：API 暫時性錯誤（429 / 5xx）自動重試 3 次，指數退避
- ⚙️ **細緻設定**：供應商、模型、溫度、逾時時間皆可調
- 💾 **對話管理**：支援多個對話、清除歷史

---

## 可用工具

AI 可以呼叫以下工具來操作你的專案：

| 工具 | 說明 |
|---|---|
| `read_file` | 讀取檔案內容 |
| `write_to_file` | 建立或覆寫檔案 |
| `replace_in_file` | 精準替換檔案片段 |
| `search_files` | 以正則表達式搜尋專案 |
| `list_files` | 列出目錄內容 |
| `execute_command` | 執行 CLI 指令 |
| `create_project` | 建立新專案結構 |
| `compile` | 編譯專案 |
| `run_tests` | 執行測試 |

---

## 安裝（開發者）

### 前置需求

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/) 1.75+
- [VS Code](https://code.visualstudio.com/) 1.125+

### 建置步驟

```bash
# 1. 安裝 npm 依賴
npm install

# 2. 編譯 Rust sidecar
cargo build --release

# 3. 複製 sidecar 執行檔
node scripts/copy-binary.js

# 4. 編譯 TypeScript
npm run compile

# 5. 打包 .vsix
npx vsce package
```

### 安裝 .vsix

```bash
code --install-extension ai-code-agent-0.1.0.vsix
```

或在 VS Code 中：`Extensions` → `...` → `Install from VSIX...`

---

## 設定

在 VS Code 設定中搜尋 `aiCodeAgent`，或於 `settings.json` 手動設定：

```json
{
  "aiCodeAgent.provider": "deepseek",
  "aiCodeAgent.apiKey": "sk-your-api-key-here",
  "aiCodeAgent.model": "deepseek-chat",
  "aiCodeAgent.endpoint": "https://api.deepseek.com",
  "aiCodeAgent.temperature": 0.3,
  "aiCodeAgent.timeoutSeconds": 120,
  "aiCodeAgent.reasoning": false,
  "aiCodeAgent.reasoningEffort": "high"
}
```

### 使用 Ollama（本機）

```json
{
  "aiCodeAgent.provider": "ollama",
  "aiCodeAgent.model": "llama3",
  "aiCodeAgent.endpoint": "http://localhost:11434/v1"
}
```

使用 Ollama 時不需設定 `apiKey`。

---

## 專案結構

```
├── src/                    # Rust 原始碼（sidecar）
│   ├── main.rs             # 進入點、JSON-RPC 伺服器
│   ├── config.rs           # 設定管理
│   ├── providers.rs        # AI 供應商抽象層
│   ├── providers/          # 各供應商實作
│   │   ├── deepseek.rs
│   │   ├── openai.rs
│   │   ├── anthropic.rs
│   │   └── ollama.rs
│   ├── tools/              # 工具實作
│   ├── session.rs          # 對話管理
│   ├── agent.rs            # Agent 主邏輯
│   ├── context.rs          # 工作區上下文
│   ├── tool_parser.rs      # AI 回應中的工具呼叫解析
│   ├── edit_plan.rs        # 編輯計劃
│   └── rpc.rs              # JSON-RPC 協定
├── vscode-ext/             # TypeScript 擴充功能
│   ├── extension.ts        # 擴充功能入口
│   ├── sidecar.ts          # Sidecar 行程管理
│   ├── chatView.ts         # Webview 提供者
│   ├── constants.ts        # 常數定義
│   └── webview/            # Webview 資源
├── webview/                # Webview 前端
│   ├── client.js           # RPC 客戶端
│   ├── markdown.js         # Markdown 渲染
│   ├── stream.js           # 串流處理
│   ├── session-ui.js       # 對話 UI
│   ├── plan-ui.js          # 計劃顯示 UI
│   ├── dom-helpers.js      # DOM 輔助函式
│   ├── toast.js            # 通知元件
│   └── styles.css          # 樣式
├── scripts/                # 建置腳本
├── Cargo.toml              # Rust 依賴
├── package.json            # Node 依賴 + 擴充功能資訊
└── .vscodeignore           # vsce 打包排除清單
```

---

## API Key 安全

- API Key **永遠不會**儲存在專案原始碼中
- Key 透過 VS Code 設定介面輸入，儲存在本機 VS Code 設定檔（不進 Git）
- 專案預設 `apiKey` 為空字串，無硬編碼金鑰

---

## 授權

MIT License

---

## 已知限制

- 部分 AI 供應商不支援原生 tool_calls，依賴文字解析
- Windows 上 `execute_command` 使用 `cmd /C`，與 Unix shell 行為有差異
- Ollama 工具呼叫僅支援文字解析模式（無原生 function calling）