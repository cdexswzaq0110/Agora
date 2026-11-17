# 15 · 可稽核的審查與 Serendipity skill（v0.6）

> 原始碼：[`review.rs`](../../crates/agora-core/src/review.rs)（合約、審查回覆的驗證，純函式）、[`skills.rs`](../../crates/agora-core/src/skills.rs)（skill 登錄、對應表、注入）、`engine/modes.rs` 的 `plan_build_loop`、`git.rs` 的 `fingerprint`、[`harness/se/`](../../harness/se/)（Serendipity skill bundle 的固定副本）。
> 設計：[docs/design/2026-11-17-v0.6](../design/2026-11-17-v0.6/05-final-plan.md)（Claude 與 GPT 提案、互評、整合；GPT 紅隊 15 條全部採用）。審查機制參考 [review-bridge](https://github.com/fortes1219/review-bridge)。

## 審查：由程式判定，不由模型自稱

### 驗收合約

方案核准的那一刻，程式從**核准的文字**切出驗收條件，連同方案文字一起以 SHA-256 封存在 `Plan.contract`：

1. 方案裡有 ```` ```acceptance_contract ```` 區塊（JSON 陣列 `[{id, text, verification}]`）就用它；id 必須是 `AC` 加數字且不重複。
2. 否則用固定的 Markdown 文法：第一個含「驗收標準」的標題底下、到下一個標題為止的最外層編號清單（`1.`、`1)`、`1、`），每一項連同縮排的續行算一條；code fence 裡的內容忽略。
3. 最多 30 條、每條最多 500 字；超過的部分**記在合約的 notes 裡**，不會靜默丟掉。

人在核准時修改了方案，合約就依修改後的文字重新計算。切不出任何條件時，實作後改用 v0.5 的文字審查，結論最高為推論。【已確認，`review::tests`】

### 實作後的流程

```
實作 → 指紋① → 驗證指令 → 規格審查（指紋②、③）
  ├─ 有條件未通過 → 帶著未通過的條件回到實作（修訂）
  ├─ 沒有未通過、但有無法確認 → 停下來交給你（acceptance_unverified）
  └─ 全部通過（且驗證指令通過）→ 品質審查
        ├─ 有「高」→ 帶著發現回到實作
        └─ 沒有「高」→ 審查通過（中、低列在結論的待辦）
```

- **規格審查**只判定合約裡的每一條：`pass`（一定要有證據）、`fail`、`cannot_verify`（一定要寫原因）。回覆必須是審查者**這一次回答裡最後一個** `review.result` 區塊；逐字稿或程式碼裡偽造的區塊不算。
- 程式拒收：JSON 無效、合約 hash 不符、缺漏／重複／未知的條件、沒有證據的 pass、`code` 證據指向工作區外或不存在的檔案、`command` 證據引用不存在的驗證紀錄、整體說通過卻有條件沒過。被拒收的回覆**不算通過**；每個階段最多重試一次，仍不行就停下來交給你（`review_unusable`）。【已確認，`review_contract.rs`】
- **品質審查**只在規格全部通過時才花這次呼叫，而且看不到規格審查的文字（最小上下文）；兩位審查者是同一家模型時，紀錄會標 `same_family`，不宣稱交叉獨立。
- 兩種審查都**看不到逐字稿**：審查者自己讀工作區，不聽執行者怎麼說。
- 每一次審查（含被拒收的）都即時寫進專案紀錄的 `reviews`：哪一輪、哪個階段、第幾次、結果、原因、指紋。重啟後看得到停在哪一步；從那一步續跑放在之後的版本。

### 版本指紋

`git::fingerprint` 只讀（`--no-optional-locks`），涵蓋 HEAD、每個內容和 HEAD 不同的檔案（不論有沒有加入暫存區）與未被 ignore 的未追蹤檔——以路徑＋目前內容的 hash 表示（邊讀邊算，不整份放進記憶體）——以及 submodule 狀態。它刻意不看暫存區：列出變更時 Agora 會 `git add -A`，同樣的內容必須得到同樣的指紋（v0.6 驗收時抓到：第一版用 `git diff HEAD` 的輸出當指紋，暫存前後不同，導致第一次就通過的審查被誤判為「驗證後又有變動」）。**不涵蓋**：被 ignore 的檔案、環境變數；也不能排除審查期間「改了又改回來」的瞬間變動。

| 比較 | 不一致時 |
|---|---|
| 驗證前 vs 交給審查者前 | 記錄「驗證之後又有變動」，結論不能是已確認 |
| 審查前 vs 審查後 | 這次審查不採計（`version_changed`），算一次重試 |
| 沒有 git | 版本未知，結論上限為推論 |

### 證據等級的上限

「已確認」必須同時滿足：驗證指令通過、驗證與審查是同一個版本、審查者的唯讀**已被證明**。唯讀證明在設定頁按一次（`POST /api/skills/prove-readonly`）：Agora 以唯讀模式請那個 CLI 在暫存資料夾寫一個檔案，檔案不存在才算證明（花一次很小的呼叫）。模擬模型不會寫入，視為已證明。【已確認，API 測試】

### 呼叫上限

工作坊每一輪實作最壞要 5 次呼叫（實作、規格審查與一次重試、品質審查與一次重試）。開始前就用最壞情況計算，超過每個專案 24 次的硬上限時，自動降低修訂次數並寫明原因。【已確認，`the_call_budget_covers_the_worst_case_and_never_passes_the_hard_cap`】

## Serendipity skill

### 來源與副本

skill 的唯一來源是使用者的 harness [Serendipity-Epiphany](https://github.com/cdexswzaq0110/Serendipity-Epiphany) 的 `skills/`（Agent Skills 標準格式）。它的 `scripts/sync_skills.py build` 產生 bundle，`install` 寫入 Agora 的 `harness/se/`：

- `manifest.json`：`schema_version`、`bundle_version`、`source_repo`、`source_commit`、每個檔案的正規化 SHA-256（CRLF→LF、去掉行尾空白；非 UTF-8 原樣）。
- Agora 在編譯時把 `harness/se/` 嵌進程式，啟動時逐檔驗證；某個 skill 的檔案不符就**停用那個 skill**並在 Skills 頁顯示原因，其他 skill 照常。【已確認，`skills::tests`】
- 更新 skill：在 SE 改 → commit → `build` → 對 Agora 執行 `install --bundle <bundle> --dest harness/se`（來源路徑一定要明確給）→ 重新編譯。

### 對應表（每次呼叫最多 2 個）

| 模式 | 階段／操作 | skill |
|---|---|---|
| 團隊 | 構想收斂（產出、修訂） | `se-problem-definition` |
| 團隊 | 可行性分析、需求與體驗、技術設計（產出） | `se-feasibility`／`se-product-contract`／`se-technical-design` |
| 團隊 | 同上（整合、修訂） | 該階段的 skill＋`se-spec-review` |
| 團隊 | 任務拆分 | `se-work-order` |
| 工作坊 | 提案、整合、修訂方案 | `se-work-order` |
| 工作坊 | 實作／修訂實作 | `se-minimal-implementation`（＋`se-debug`） |
| 工作坊 | 規格審查／品質審查 | `se-spec-review`／`se-quality-review` |
| 其他模式 | — | 不注入 |

Agora **自己把 skill 的內容放進提示詞**（「方法」區塊，每個最多 4,000 字），不靠 CLI 自動觸發——設定重的時候，`claude -p` 的 skill 自動觸發可能完全不發生。`references/` 這一版不內嵌，收據標 `references_omitted`。延伸 skill（`extended/`）是選用的舊版本與其他能力，**永遠不會被自動注入**，即使與核心 skill 同名。【已確認，兩個對應表測試的期望值寫在測試裡，不與正式程式共用】

### 收據與統計

每則發言的 `skills` 是**程式寫的**收據：skill 名稱、版本、hash、是否截斷、是否省略 references。Skills 頁的數字：注入次數＝實際送出的非分岔發言數（截斷另列）；審查的採用／不採用分開計，格式拒收與呼叫失敗不混進通過／不通過。

## 已知限制

- 從審查中途續跑（重啟後）還沒做；紀錄完整，但目前重啟會把進行中的專案標為中斷。【已確認，程式行為】
- 唯讀證明只證明「那一次嘗試寫入失敗」；模型若根本沒嘗試，證明較弱。【推論】
- 證據的**語義**是否正確仍是模型判斷：程式只確認引用的檔案與驗證紀錄存在且屬於這個版本。【推論】
- skill 對實際品質的影響還沒用真的 CLI 做開／關對照。【未驗證】
