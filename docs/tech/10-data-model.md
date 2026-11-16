# 10 · 資料模型與儲存

原始碼：`model.rs`、`store.rs`、`digest/mod.rs`、`history.rs`、`scheduler.rs`、`templates.rs`。

## 檔案配置

`%APPDATA%\Agora`（可以用環境變數 `AGORA_DATA_DIR` 或 `--data` 改位置）：

```text
settings.json            設定、配對的裝置、預設模型、不支援的模型、情報設定、排程、自訂範本
projects.json            專案（含專案記憶）
quota.json               每個提供者最後一次的額度快照
quota_history.jsonl      額度歷史（每行一筆 QuotaSample）
sessions/<id>.json       完整的議題紀錄
digests/<YYYY-MM-DD>.json 每日情報
experiments/<id>.json    模式比較實驗
worktrees/<session>/     隔離工作區（套用或丟棄後移除）
sandbox/                 預設專案的工作目錄
```

寫入一律是原子的：先寫 `.tmp` 再 rename。id 只允許英數和 `-`；日期必須是 `YYYY-MM-DD`。

## SessionSpec（建立議題）

| 欄位 | 型別 | 說明 |
|---|---|---|
| `project_id` | string | |
| `mode` | `workshop`、`solo`、`relay`、`roundtable`、`debate` | |
| `participants` | `[{provider, model?}]` | 提案者、與談人、正反方…… |
| `judge` | Participant? | 總結者或裁判；辯論不指定時是交叉裁判 |
| `builder` / `reviewer` | Participant? | 工作坊的執行者與審查者 |
| `prompt` | string | 上限 40,000 字 |
| `rounds` | number? | 協做與工作坊是修訂次數（0–3）；討論與辯論是輪數（1–4） |
| `access` | `read_only`、`write` | 工作坊實作時一律 `write` |
| `verify_command` | string? | 例如 `cargo test` |
| `max_turns` | number? | 只能比計算值小，硬上限 24 |
| `blind` / `red_team` / `plan_only` / `auto_approve` | bool | |
| `isolate` | bool（預設 true） | 寫入時使用 worktree |
| `seed` | `{session_id, upto}`? | 分岔 |
| `group_id` / `template` | string? | 比較實驗、範本來源 |

## SessionRecord（議題紀錄）

`id`、`spec`、`title`、`status`、`phase`、`created_at`、`updated_at`、`turns[]`、`notes[]`、`interjections[]`、`verify_runs[]`、`verdict?`、`plan?`、`workspace?`、`error?`

- **Turn**：`index`、`round`、`role`、`provider`、`family`、`model`、`started_at`、`ended_at`、`text`、`ok`、`error`、`usage{input_tokens, output_tokens, cached_input_tokens}`、`tools[{name, detail}]`、`delta_seq`、`context_compacted`、`seeded`、`rerouted_from`
- **Verdict**：`outcome`（approved、open_issues、verify_failed、answered、verified、synthesized、judged、split、planned）、`headline`、`grade`、`summary`、`open_issues[]`
- **Plan**：`text`、`revision`、`approved`、`edited`、`verify_command`、`decided_at`、`feedback[]`
- **Workspace**：`kind`（worktree、direct）、`path`、`root`、`branch`、`base`、`state`（active、applied、discarded）、`message`
- **Note**：`at`、`level`（info、warn、guard）、`text`

## 證據等級（`grade.rs`）

由弱到強：`unknown` 未知 < `unverified` 未驗證 < `candidate` 候選 < `inferred` 推論 < `confirmed` 已確認。程式用 `min()` 取「最弱的一環」，並把只靠模型判斷的結論上限設在「推論」。

## Digest（每日情報）

`date`、`generated_at`、`items[DigestItem]`、`markdown`、`llm_ok`、`llm_error`、`summarizer`、`usage`、`sources[SourceStatus{source, label, ok, count, error, ms}]`

**DigestItem**：`n`、`source`、`source_label`、`title`、`url`、`discussion_url`、`score`、`comments`、`author`、`summary`、`tags[]`、`language`、`stars_today`、`also_on[]`、`matched_keywords[]`、`rank`

## Settings 的重點欄位

| 欄位 | 預設 | 說明 |
|---|---|---|
| `port` / `lan_port` / `lan_enabled` | 4717 / 4718 / false | |
| `quota_stop_percent` | 97 | 額度保護門檻 |
| `auto_reroute` | true | 額度不足時改派 |
| `context_pack` | true | 提案時附上專案快照 |
| `isolate_cli_config` | true | 不載入個人的 MCP 工具、外掛、hooks |
| `transcript_budget_chars` | 24,000 | 記憶壓縮門檻 |
| `turn_timeout_secs` / `verify_timeout_secs` | 1200 / 600 | |
| `default_models` / `unsupported_models` | {} | 依提供者設定 |
| `digest` | 08:00、6 個來源、10 個關注主題、36 則 | |
| `schedules` / `templates` / `devices` | [] | |
