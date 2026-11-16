# 09 · API 參考

伺服器：`agora-server`。預設在 `http://127.0.0.1:4717`；開啟區網連線時另外監聽 `0.0.0.0:4718`。
所有會改變狀態的請求都要帶 `Content-Type: application/json`（`DELETE` 例外）。權限欄位：**L** = 只限本機；**P** = 已配對裝置也可以；**公開** = 不必配對。

## 身分與配對

| 方法 | 路徑 | 權限 | 說明 |
|---|---|---|---|
| GET | `/api/whoami` | 公開 | `{local, paired, device, version, demo}` |
| POST | `/api/pair` | 公開 | `{code, name}` → 設定 `agora_token` cookie |
| GET | `/api/pairing` | L | 配對碼、到期時間、區網或 Tailscale 網址、已配對的裝置；v0.4 起多了 `lan_candidates:[{ip,name,virtual}]`（這台電腦的私有 IPv4 與網卡名稱）與 `lan_running` |
| POST | `/api/pairing/rotate` | L | 換一組配對碼 |
| DELETE | `/api/pairing/devices/{id}` | L | 撤銷裝置 |
| GET | `/api/pairing/qr.svg?target=lan\|tailscale&ip=` | L | 配對用的 QR code（網址片段帶配對碼）。`ip` 必須是 `lan_candidates` 裡的位址，否則 400 |
| POST | `/api/network` | L | `{lan_enabled}` → 啟動或停止區網監聽 |

## 系統

| 方法 | 路徑 | 權限 | 說明 |
|---|---|---|---|
| GET | `/api/health` | P | `{ok, version, data_dir, mcp_path}` |
| GET / PATCH | `/api/settings` | P / L | 設定（不含配對 token）；PATCH 只更新有帶的欄位。v0.4：`default_efforts:{provider: effort\|null}`、`digest.effort`；不認得的推理強度回 400 |
| GET | `/api/providers` | P | 提供者、模型清單、健康狀態、額度、不支援的模型；v0.4：每個模型的 `efforts`（可用的推理強度）與 `default_effort`，以及提供者的 `default_effort`（設定值） |
| POST | `/api/providers/refresh` | P | 重新做健康檢查並查詢額度 |
| GET | `/api/usage` | P | 所有額度快照 |
| POST | `/api/usage/refresh` | P | 免費查詢額度 |
| POST | `/api/usage/probe-claude` | P | 送一個極短的請求給 Claude haiku，更新額度（會用到極少額度） |
| GET | `/api/usage/history?days=7` | P | 額度歷史樣本 |
| GET | `/api/usage/stats?days=30` | P | 依日期、模式、提供者彙整的發言數與 tokens |

## 專案、範本

| 方法 | 路徑 | 權限 | 說明 |
|---|---|---|---|
| GET / POST | `/api/projects` | P | 列出專案；建立專案 `{name, path}`（路徑必須存在） |
| PATCH | `/api/projects/{id}` | P | `{name?, path?, memory?}` |
| GET / POST | `/api/templates` | P | 內建加自訂範本；儲存自訂範本 |
| DELETE | `/api/templates/{id}` | P | 刪除自訂範本 |

## 議題

| 方法 | 路徑 | 權限 | 說明 |
|---|---|---|---|
| GET | `/api/sessions?project=&q=` | P | 議題摘要清單；`q` 會搜尋標題、議題內容和所有發言 |
| POST | `/api/sessions` | P | 用 `SessionSpec` 建立議題（見 [10](10-data-model.md)） |
| GET | `/api/sessions/{id}` | P | 完整紀錄 |
| GET | `/api/sessions/{id}/events` | P | **SSE**：先送 `snapshot`，之後是即時事件 |
| POST | `/api/sessions/{id}/interject` | P | `{text}` |
| POST | `/api/sessions/{id}/cancel` | P | 停止（等待核准中的議題也可以） |
| POST | `/api/sessions/{id}/decide` | P | `{"action":"approve","plan"?,"verify_command"?}`、`{"action":"revise","feedback"?,"comments"?:[{block_id,heading,quote,text}],"plan_revision"?}`、`{"action":"reject","reason"?}`。段落留言最多 20 則、必須對應**目前版本**的段落（`plan.blocks`）、`plan_revision` 要相符；`feedback` 與留言不能都空，否則 400 |
| GET | `/api/sessions/{id}/diff` | P | `{files:[{path,added,removed}], added, removed, patch, truncated}` |
| POST | `/api/sessions/{id}/workspace/apply` | P | 把 worktree 的變更套用到專案（不 commit） |
| POST | `/api/sessions/{id}/workspace/discard` | P | 丟棄 worktree |
| POST | `/api/sessions/{id}/remember` | P | 把結論加進專案記憶。body 可省略（空的或 `{}` → 舊格式）；`{"line":"…"}` → 去掉換行、上限 300 字、不可為空，寫成 `- [日期] …【證據等級】` |
| GET | `/api/sessions/{id}/export.md` | P | 下載 Markdown（`Content-Disposition: attachment`） |
| POST | `/api/sessions/{id}/export` | L | `{kind:"adr"\|"lesson"}` → 寫進專案的 `docs/adr/` 或 `docs/lessons/` |

## 比較實驗、情報、排程

| 方法 | 路徑 | 權限 | 說明 |
|---|---|---|---|
| POST | `/api/experiments` | P | `{spec, modes:[…]}`：依序跑多種模式，一律唯讀 |
| GET | `/api/experiments` / `/{id}` | P | 實驗清單；單一實驗與各議題的摘要 |
| GET | `/api/digests` | P | `{dates, running, enabled, time}` |
| GET | `/api/digests/{date\|latest}` | P | 一份情報（`Digest`）；v0.4 起含 `compared_with`、`topic_trends` 與每個項目的 `trend`（v0.4 以前存的檔案在讀取時補算，不回寫） |
| GET | `/api/digests/state` | P | `{latest, generated_at, unread, running}`：導覽列「情報」的未讀點 |
| POST | `/api/digests/{date}/read` | P | `{generated_at}`：必須等於伺服器上那一份的 `generated_at`，否則 400；游標只會變大；所有裝置共用 |
| POST | `/api/digests/run` | P | 立即整理今天的情報（在背景執行） |
| GET / POST | `/api/schedules` | P | 列出或新增、更新排程 |
| DELETE | `/api/schedules/{id}` | P | 刪除排程 |
| POST | `/api/schedules/{id}/run` | P | 立即執行一次 |

## SSE 事件

`/api/sessions/{id}/events` 送出的事件（JSON 的 `type` 欄位）：

| type | 欄位 | 說明 |
|---|---|---|
| `snapshot` | `session` | 連線時先送一份完整快照 |
| `status` | `status, title, error` | `running`、`awaiting_approval`、`done`、`failed`、`cancelled` |
| `phase` | `phase` | 提案、互評、整合、紅隊審視、修訂方案、等待核准、實作、驗證、審查、修訂實作 |
| `turn_started` / `turn_ended` | `turn` | 一則發言開始或結束（`turn_ended` 帶最終文字與用量） |
| `delta` | `index, seq, text` | 串流文字片段；用 `seq` 去重 |
| `tool` | `index, note` | 工具呼叫 |
| `note` | `note` | 過程紀錄（Info、Warn、Guard） |
| `interjection` / `verify` / `verdict` / `plan` / `workspace` | … | 對應欄位更新 |
| `quota` | `snapshot` | 額度更新 |
| `lagged` | — | 用戶端跟不上，應該重新抓一次紀錄 |

`/api/events`（全域串流）只送低頻事件：先送 `hello`（附目前所有額度），之後是 `status`、`quota`、`digest`。`digest` 的欄位是 `status`（running、done、failed）、`date`、`error`。
