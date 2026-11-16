# 03 · CLI 提供者與計費防護

原始碼：`crates/agora-core/src/provider/`。

## Provider trait

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;                 // "claude"、"codex"、"mock-claude"…
    fn family(&self) -> Family;           // Claude | Gpt
    fn display_name(&self) -> String;
    fn models(&self) -> Vec<ModelOption>;
    async fn health(&self) -> Health;                    // 免費：執行檔在不在、有沒有登入、是不是訂閱
    async fn poll_quota(&self) -> Option<QuotaSnapshot>; // 免費查額度（Claude 沒有 → None）
    async fn run(&self, req: TurnRequest, sink: Sink) -> TurnOutput;
}
```

`run` 收到 `TurnRequest`（提示詞、模型、工作目錄、權限、逾時、取消、`clean_config`），把 `ProviderEvent`（文字片段、工具、額度、警告）送進 `sink`，最後回傳 `TurnOutput`（成功與否、最終文字、用量、模型是否被拒、是否觸發防護）。

## Claude Code（`claude.rs`）

```
claude -p --output-format stream-json --verbose --include-partial-messages --no-session-persistence
       [--model X] [--strict-mcp-config --setting-sources project,local]
       唯讀：--permission-mode dontAsk --allowedTools Read,Grep,Glob,WebSearch,WebFetch
       可寫：--permission-mode acceptEdits
```

提示詞一律從 **stdin** 傳入，避免 Windows 命令列長度限制和引號問題。串流的解析（`ClaudeStreamParser`）：

| 事件 | 處理 |
|---|---|
| `system/init` | 讀 `apiKeySource`，**不是 `none` 就立刻中止**（計費防護） |
| `stream_event` → `content_block_delta/text_delta` | 文字片段 |
| `assistant` 裡的 `tool_use` | 工具紀錄（摘要 `file_path`、`command`……） |
| `rate_limit_event` | 額度快照（見 [06](06-quota-and-usage.md)） |
| `result` | **看 `is_error`，不看 `subtype`**（[lessons/0001](../lessons/0001-claude-is-error-hides-in-success-subtype.md)）；認證錯誤轉成 `claude auth login` 提示 |

健康檢查：執行 `claude --version` 和 `claude auth status --json`（`loggedIn`、`authMethod`、`apiProvider`）。

## Codex（`codex.rs`）

```
codex exec --json --skip-git-repo-check --ephemeral -C <cwd> -s read-only|workspace-write [-m X] [--ignore-user-config] -
```

| 事件 | 處理 |
|---|---|
| `item.completed/agent_message` | 文字（最後一則訊息就是最終答案） |
| `item.started/command_execution`、`file_change`、`mcp_tool_call`、`web_search` | 工具紀錄 |
| `item.completed/error` | 非致命警告 |
| `turn.completed` | 用量（`input_tokens`、`cached_input_tokens`、`output_tokens`） |
| `turn.failed` / `error` | 轉成可讀的錯誤；`model_not_found` 會從訊息解析出模型名稱並標成「方案不支援」 |

**`codex app-server`**（JSON-RPC over stdio）：先 `initialize`，再送 `initialized`，然後：

- `account/read`（`params: {}`，**必填**）→ 帳號類型（`chatgpt`／`apiKey`……）和方案。
- `account/rateLimits/read`（`params` 不帶）→ 5 小時窗和每週窗。

一次查詢兩個方法，快取 15 秒（健康檢查）或 10 分鐘（計費防護）。

## 三層計費防護（ADR 0001）

| 層 | 做法 | 測試 |
|---|---|---|
| 1. 環境 | `proc::command` 移除 `PAID_API_ENV`（Anthropic、OpenAI、Bedrock、Vertex、Azure……共 11 個變數） | `command_scrubs_every_paid_api_variable` |
| 2. Claude | `apiKeySource != "none"` → 在模型回答前終止程序 | `api_key_billing_source_aborts_before_any_answer` |
| 3. Codex | 每輪之前確認 `account.type == "chatgpt"`；無法確認也不執行（fail closed） | `account_parsing_distinguishes_api_key_from_chatgpt` |

## 個人設定隔離（lessons/0007）

`isolate_cli_config`（預設開啟）讓 CLI 不載入你的使用者層 MCP 工具、外掛和 hooks：

- Claude：`--strict-mcp-config --setting-sources project,local`
- Codex：`--ignore-user-config`

## 行程管理（`proc.rs`、`stream.rs`、`resolve.rs`）

- **找到真正的執行檔**：Windows 上 npm 裝的是 `.cmd` 包裝，`CreateProcess` 無法直接執行。改找 `node_modules/@anthropic-ai/claude-code/bin/claude.exe`、`@openai/codex-win32-*/vendor/*/bin/codex.exe`。
- **不閃視窗**：每個子行程都設定 `CREATE_NO_WINDOW`，否則桌面版每次呼叫都會閃出主控台視窗。
- **取消與逾時**：`taskkill /T /F` 終止整個行程樹，因為 CLI 會再啟動工具子行程。
- **輸出編碼**：逐行讀取、lossy 解碼，單一個非 UTF-8 位元組不會中斷整個串流。

## 新增一個提供者

1. 實作 `Provider`。串流解析器要寫成純函式，並用**錄下來的真實輸出**做單元測試。
2. 計費防護三件事都要有測試：
   - 把該 CLI 會讀的 API key 環境變數加進 `PAID_API_ENV`。
   - 能用程式確認「這次執行用的是訂閱」，例如 init 事件或帳號查詢。
   - 無法確認時不執行。
3. 健康檢查不能花額度。
4. 在 `engine::cli_providers` 註冊；介面會自動出現在參與者選單和用量頁。
5. 寫一則 devlog，記下實測證據。

第三家模型（例如 Gemini CLI）目前**被擋住**：這台電腦沒有安裝，無法完成第 2 步的實測。
