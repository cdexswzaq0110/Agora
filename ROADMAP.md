# Roadmap

狀態：✅ 已完成 · 🧪 已實作，但還沒在真實環境驗證 · ⏸ 延後 · ⛔ 被擋住。證據見 [開發日誌](docs/devlog/README.md)。

## 已完成

| 版本 | 項目 |
|---|---|
| v0.1 | ✅ 單人、協做、討論、辯論 · ✅ 用量儀表與置頂小工具 · ✅ 三層計費防護 · ✅ 記憶壓縮 · ✅ 驗證指令與證據等級 · ✅ 網頁、桌面、手機配對 |
| v0.2 | ✅ 方案工作坊與核准關卡 · ✅ git worktree 隔離、diff、套用 · ✅ 每日技術情報 · ✅ 9 個議題範本 · ✅ ADR 與 lesson 匯出 · ✅ 額度改派 · ✅ 分岔 · ✅ 盲審 · ✅ 檢索式記憶 · ✅ 專案快照 · ✅ 模式比較實驗 · ✅ 排程議題 · ✅ 用量歷史圖表 · ✅ MCP server · ✅ 隔離 CLI 個人設定 · 🧪 QR 配對 · 🧪 語音輸入 |
| v0.3 | 介面改進：由 Claude 與 GPT 先討論再實作，討論過程見 [docs/design/2026-11-16-ui-v0.3](docs/design/2026-11-16-ui-v0.3/) |

## 下一步

| # | 項目 | 為什麼 | 等級 |
|---|---|---|---|
| 1 | **原生 session 續接**（`claude --resume`、`codex exec resume`） | 執行者在修訂時保留自己的工具上下文，省額度 | ⏸ 候選：要先實測兩個 CLI 在 headless 模式下 resume 是否穩定 |
| 2 | **第三家模型**（Gemini CLI 等） | 討論模式本來就支援 2–4 位與談人 | ⛔ 這台電腦沒有安裝；ADR 0001 要求計費來源能被程式確認 |
| 3 | 每天的情報對照**昨天**的趨勢（新出現、持續升溫） | 看出變化，而不只是今天的快照 | 推論 |
| 4 | Codex 推理強度在介面可調（現在隔離個人設定後用 CLI 預設值） | 實作品質與成本的取捨交給使用者 | 推論 |
| 5 | 工作坊的方案逐段留言（不只整段修改） | 核准時的回饋更精準 | 候選 |
| 6 | 議題結論自動建議「存入專案記憶」的那一行 | 減少手動整理 | 推論 |

## 已知限制

- Claude 的額度要跑過一次 `claude -p` 才有數字：CLI 沒有免費的查詢介面（`claude auth status` 只有登入狀態）。【已確認】
- `rate_limit_event.unifiedWindows` 在 Claude Code 的 schema 裡標成 `@internal`，未來可能改變；解析器有退路，會改讀單一的 `rateLimitType` 加 `utilization`。【已確認】
- Reddit 的匿名存取時好時壞（[lessons/0006](docs/lessons/0006-reddit-anonymous-access-is-best-effort.md)）。
- worktree 不包含被 gitignore 的目錄，有些驗證指令要先安裝相依套件，這種情況可以關閉隔離。
- 實作階段的成本：一次可能用到數十萬 tokens（大多是快取），在 Plus 方案大約占 5 小時額度的 8%。
- 區網模式是 HTTP，手機無法安裝成 App，也收不到通知；請改用 `tailscale serve`（HTTPS）。
