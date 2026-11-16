# 0007 — CLI 會把你的個人設定帶進 agent 執行

- **Date**: 2026-11-16
- **Grade**: 已確認

## 發生了什麼

Agora 用 `codex exec` 執行工作坊的實作階段。工具紀錄裡出現了兩次 `mcp:js · cua_repl`：這是使用者自己在 `~/.codex/config.toml` 設定的 MCP 工具（看起來是電腦操控類），不是 Agora 給的。Claude 也有同樣的情況：`claude -p` 的串流裡有 `hook_started`／`hook_response`，代表使用者自己的 Claude hooks 也在 Agora 的議題裡執行了。

這不在 ADR 0001（計費）的範圍內，但它是同一類問題：**CLI 預設會繼承使用者的整個環境**。一個由 Agora 排程、你沒有在旁邊看的 agent，不應該拿得到操控你桌面的工具。

## 證據

- 失敗：議題 `s-1a1e0abf42beda1ad` 的 Codex 實作發言，工具紀錄有 `('mcp:js', 'cua_repl')` ×2。
- 修正後：
  - `claude -p --strict-mcp-config --setting-sources project,local` 的 init 事件：`mcp_servers: []`、外掛 0 個、沒有 hook 事件。
  - `codex exec --ignore-user-config` 正常回覆，輸入 tokens 從 24,926 降到 20,643（不再載入 MCP 工具的定義）。
- 測試：`read_only_turns_cannot_edit`（Claude 參數）、`args_pick_sandbox_from_access_and_read_prompt_from_stdin`（Codex 參數）。

## 規則

- 新設定 `isolate_cli_config` 預設開啟，經由 `TurnRequest.clean_config` 傳給每個提供者。
- 關掉它是使用者的明確選擇（設定頁「隔離 CLI 的個人設定」），例如真的想讓 agent 用自己的 MCP 工具時。
- 副作用：Codex 不再讀 `config.toml`，所以裡面不支援的預設模型（[0002](0002-codex-configured-model-not-on-plan.md)）和 `service_tier` 警告也一起消失；推理強度改回 CLI 預設值。
