# 技術文件

給想深入了解 Agora 的人：自己日後維護、向面試官或同事講解，或交接給另一個 agent。
每份文件都對應到原始碼位置，主張盡量附上證據等級。

## 建議閱讀順序

| # | 文件 | 讀完能回答 |
|---|---|---|
| 01 | [系統總覽](01-overview.md) | Agora 是什麼、由哪幾塊組成、資料怎麼流動 |
| 02 | [引擎與五種協作模式](02-engine-and-modes.md) | 每種模式的演算法、成本上限、核准關卡怎麼運作 |
| 03 | [CLI 提供者與計費防護](03-providers-and-billing-guard.md) | 怎麼驅動 Claude Code 與 Codex、怎麼保證不走 API key、怎麼新增提供者 |
| 04 | [工作區隔離（git worktree）](04-workspace-isolation.md) | 模型改檔時，你的專案怎麼保持乾淨 |
| 05 | [每日技術情報](05-daily-digest.md) | 抓哪些來源、怎麼排序與去重、怎麼摘要、怎麼排程 |
| 06 | [用量與額度](06-quota-and-usage.md) | 剩餘額度從哪來、額度保護與改派、歷史圖表 |
| 07 | [記憶與上下文](07-memory-and-context.md) | 記憶壓縮、檢索式專案記憶、專案快照、分岔 |
| 08 | [安全模型](08-security-model.md) | 本機信任、手機配對、CSRF／DNS rebinding、CSP、CLI 個人設定隔離 |
| 09 | [API 參考](09-api-reference.md) | 所有 HTTP 端點與 SSE 事件 |
| 10 | [資料模型與儲存](10-data-model.md) | 每個 JSON 檔案和結構的欄位 |
| 11 | [前端](11-frontend.md) | 模組分工、即時串流、Markdown、圖表、PWA |
| 12 | [桌面、手機與 MCP](12-desktop-mobile-mcp.md) | Tauri 外殼、系統匣、Tailscale、在 Claude Code／Codex 裡呼叫 Agora |
| 13 | [測試策略](13-testing.md) | 測了什麼、怎麼測、哪些只做過實機驗證、哪些還沒驗證 |
| 14 | [專案團隊工作流](14-team-workflow.md) | 18 個角色怎麼分成每個階段的模型呼叫、怎麼核准與過期、為什麼不會重複開始或多花額度 |
| — | [講解指南](presentation-guide.md) | 30 秒、3 分鐘、10 分鐘的講法，示範腳本，常見問答 |

## 相關文件

- 為什麼這樣設計：[ARCHITECTURE.md](../../ARCHITECTURE.md)、[ADR](../adr/)
- 做的過程：[開發日誌](../devlog/README.md)
- 踩過的坑：[lessons](../lessons/INDEX.md)
- 詞彙：[CONTEXT.md](../../CONTEXT.md)
