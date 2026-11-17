# 06 · 用量與額度

原始碼：`quota.rs`、`history.rs`、`engine/mod.rs`（`record_quota`、`quota_block`、`reroute_candidate`）、`web/js/usage.js`、`web/widget.*`。

## 額度從哪裡來

| 模型 | 來源 | 花費 | 時效 |
|---|---|---|---|
| GPT（Codex） | `codex app-server` 的 `account/rateLimits/read` → `primary`（300 分鐘）、`secondary`（10080 分鐘），各有 `usedPercent`、`resetsAt` | 免費 | 每 5 分鐘自動查；每輪 Codex 發言結束後也會查 |
| Claude | `claude -p` 串流裡的 `rate_limit_event.rate_limit_info.unifiedWindows.{five_hour, seven_day}.utilization`（0–1） | 跟著發言一起取得，不另外花 | 有發言才更新；可以手動「主動更新」（送一個極短的請求給 haiku） |

兩者都轉成同一個結構：

```rust
QuotaSnapshot { provider, family, plan, status: ok|warning|limited|unknown,
                windows: [QuotaWindow { key, label, used_percent, resets_at, window_minutes }],
                source, observed_at }
```

Claude 有時只會回報一個時段，所以用 `merged_with` 把新資料合併進舊的快照，沒有被更新的時段保留原值。

## 額度保護與改派

`QuotaSnapshot::blocks(stop_percent, now)`：只要有任一時段已用 ≥ 門檻（預設 97%）而且還沒重置，或者提供者回報已達上限，就回傳原因。

每則發言開始前：

```
被擋？ ── 否 ──→ 正常呼叫
  └ 是 → auto_reroute 開啟、而且有可以代打的提供者？
          ├ 是 → 改由它發言（Turn.rerouted_from 記錄原本的提供者，過程紀錄寫一則 Guard）
          └ 否 → 這則發言直接失敗（不呼叫 CLI，usage 為空），寫一則 Guard
```

代打者必須：跟原本的提供者同樣是真實或同樣是模擬、健康檢查通過（或還沒檢查過）、自己沒有被擋；優先選另一家模型。

## 歷史（`history.rs`）

- `quota_history.jsonl`：每個提供者至少間隔 10 分鐘存一筆；任一時段變動 ≥ 1 個百分點，或時段數量改變時，也會存。
- `usage_stats(days)`：從專案紀錄彙整每天和每種模式的發言數與 tokens，以及「已確認」的比例；分岔複製來的發言不計入。

## 介面

- **頂部列與置頂小工具**：每個提供者一條 5 格 HP 條（剩餘 > 60% 綠、> 40% 黃、> 20% 橘、其餘紅），取最緊的時段。
- **用量頁**：
  - 額度卡片：時段、已用比例、重置倒數、來源、觀測時間，以及健康檢查和修復指令。
  - 額度走勢：依時段分成兩張小圖（5 小時窗、每週窗），時間軸固定為過去 7 天，有十字線提示框和表格檢視。
  - 每日 tokens：依提供者分色堆疊，有提示框和表格檢視。
  - 各模式表現的表格。
- 圖表配色用 dataviz 的 `validate_palette.js` 驗證過：淺色 `#b65a2e`／`#008a78`，深色 `#c46c3f`／`#1f9384`，色盲差異、明度與對比全部通過。

## 證據

- 單元測試：Codex 與 Claude 的對應、達到上限時拒絕、重置後解除、部分更新的合併、歷史取樣。【已確認】
- 端到端：`quota_guard_refuses_without_calling_the_cli_when_reroute_is_off`、`quota_guard_reroutes_to_the_other_family`。【已確認】
- 實機：Codex Plus，5 小時窗 0% → 2%（一次問答）→ 12%（一次實作，約 59 萬輸入 tokens，其中多為快取）。【已確認】
- Claude 的 `rate_limit_event` 實際送出的內容：【未驗證】（CLI 登入已過期）。
