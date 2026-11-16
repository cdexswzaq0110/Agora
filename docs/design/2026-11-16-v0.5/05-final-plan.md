# v0.5 · 最終方案：專案團隊工作室

> 由 Claude 整合兩份提案（[01](01-claude-proposal.md)、[02](02-gpt-proposal.md)）與兩份互評（[03](03-claude-critique.md)、[04](04-gpt-critique.md)）。實作前交給 GPT 紅隊（[06](06-gpt-redteam.md)），修訂紀錄附在文末。
> 同一輪另外兩項由 GPT 在獨立 worktree 實作、Claude 審查後合併：桌寵助理（使用者的個人角色；只在使用者本機，見 README「本機擴充」）、動態效果系統與命令面板。Tailscale 已由 Claude 修正並完成連線。

## 評分（兩份互評的平均）

| 提案 | 正確性 | 簡潔度 | 風險（5＝最低） | 成本（5＝最低） | 可測試性 |
|---|---|---|---|---|---|
| Claude | 3.75 | 3.5 | 3 | 2.75 | 4 |
| GPT | 4 | 3.25 | 3.75 | 4 | 4.5 |

## 範圍

五個階段（GPT 與 Claude 都同意 0–3 是核心、4 產出任務並交給既有工作坊；上線與維運留到之後）：

| # | 階段 | 標準深度的呼叫 | 產出 | 你的決定 |
|---|---|---|---|---|
| 0 | 構想收斂 | 1：Claude 扮演 PM、UX、資料分析師 | 構想簡報＋**待你回答的問題** | 回答問題後核准 |
| 1 | 可行性分析 | 2：GPT 扮演 SA、Architect、系統架構師、DBA、DevOps/SRE 產出 → Claude 扮演 QA、AI 工程師、LLM、NLP、ML、DL、MLOps 審查並整合 | 可行性報告（技術、資料、AI、時程、成本、風險、安全檢查）＋**建議：做／調整／不做** | **繼續／調整／停止** |
| 2 | 需求與體驗 | 2：Claude 扮演 PM、UX、SA 產出 → GPT 扮演 QA、Dev、資料分析師審查並整合 | PRD（使用者故事＋驗收標準）、UX 流程、非功能需求 | 核准 |
| 3 | 技術設計 | 3：Claude（Architect、系統架構師、系統設計師）與 GPT（SD、DBA、Dev；AI 類加 AI 小組）**平行**產出 → 另一家（QA、DevOps/SRE，含安全檢查）紅隊並整合 | 架構、元件與 API、資料模型、AI/ML 設計、安全、ADR 草稿 | 核准 |
| 4 | 任務拆分 | 1：GPT 扮演 Dev、QA、DevOps/SRE、MLOps | 任務清單：每項附驗收標準與驗證指令 | 核准後每項可**一鍵交給工作坊** |

- **18 個角色**全部在名冊裡；每個階段的每個參與角色都要留下「完整／簡查／不適用」與理由（`RoleCheck`）；某角色在輸出裡缺席就記為「缺漏」，絕不顯示成通過。名冊測試確保每個角色至少出現在一個階段。
- 安全：不新增第 19 個角色（使用者列的 18 個是要求），改成 Architect 與 QA 的固定檢查項目。
- 深度：**快速**＝每階段 1 次（共 5）；**標準**＝上表（共 9）；**完整**＝每個產出小組最多 3 個角色一次、外加整合（依實際計畫精算後顯示）。AI 類專案在標準深度把 AI 小組獨立成一次。開始每個階段前顯示這個階段的呼叫數，沿用額度保護。

## 實作

### 資料（`crates/agora-core/src/team.rs`）

- `TeamRole`：id、中文名、英文名、小組、職責、預設扮演家族、是否 AI 角色。18 筆常數。
- `StageKind`：`Discovery | Feasibility | Product | Design | Tasks`。
- `Depth`：`Quick | Standard | Full`；`ProjectKind`：`General | Ai`。
- `stage_plan(stage, depth, kind) -> StagePlan { producers: Vec<Panel>, integrator: Option<Panel> }`，`Panel { family, roles }`。純函式，有測試。
- `Workflow`：id、project_id、title、idea、depth、kind、providers（Claude／GPT 各用哪個提供者）、status（`Active | Stopped | Done`）、`stages: Vec<StageState>`、時間戳。
- `StageState`：kind、status（`Pending | Running | AwaitingApproval | Approved | Stopped | Stale`）、session_id、artifact、revision、based_on（上游各階段的 revision）、role_checks。
- 存在 store 的 `workflows/<id>.json`（原子寫入）。

### 執行（沿用工作坊，**不新增 Mode**）

- `SessionSpec.stage: Option<StageSpec>`（`serde(default)`，只在工作坊＋只產出方案時使用）：每位提案者的角色清單、整合者、情境（構想＋已核准的上游文件＋專案快照）。有 `stage` 時，工作坊跳過互評輪，提案者用團隊提示詞，整合者用「審查並整合」提示詞；只有一位提案者而且沒有整合者時，提案者的輸出直接成為方案。核准關卡、修訂、段落留言、重啟後續跑全部沿用。
- 提示詞要求每個角色固定的小標題 `### 角色名（完整｜簡查｜不適用）`，整合者必須保留「保留的異議」與「需要你決定的事」兩節。
- 階段議題結束時（核准、停止、失敗），引擎更新 workflow：核准 → artifact＝方案文字、revision+1、下游已核准的階段標成 `Stale`；可行性「只保留不實作」→ workflow `Stopped`。
- 冪等：同一階段已有進行中或等待核准的議題時，「開始」回傳那個議題，不建立第二個。

### API

- `GET/POST /api/workflows`、`GET /api/workflows/{id}`、`POST /api/workflows/{id}/stages/{n}/start`（回傳 session）、`POST /api/workflows/{id}/tasks/{k}/start`（建立工作坊議題，seed 帶入設計文件摘要）、`POST /api/workflows/{id}/export`（只限本機，寫入 `docs/agora/<slug>/`）。
- 核准、修訂、留言**直接用**既有的 `/api/sessions/{id}/decide`。
- SSE：`workflow` 事件（id、狀態、下一步），讓介面與桌寵更新。

### 介面

- 導覽新增「團隊」：專案的工作流清單＋「新構想」（構想、專案、類型、深度與呼叫估算）。
- 工作流頁：階段軌（完成／進行中／等你核准／過期）、「下一步」卡片、團隊名冊（18 張角色卡，依小組排列，顯示扮演者與目前階段的狀態）、已核准的文件、任務清單（一鍵交給工作坊）。階段的討論與核准沿用議題頁。
- 構想收斂的核准面板顯示「待你回答的問題」輸入框，答案附加在方案後再核准。
- 動態效果用 GPT 實作的 `motion.js`（角色卡在發言中時漣漪、階段之間的線條脈動）；降低動態時全部停用。

## 驗收標準

1. Rust：名冊恰好包含使用者列的 18 個角色；每個角色至少出現在一個階段的計畫裡；各深度的呼叫數（快速 5、標準 9、AI 類標準 10）。
2. Rust（模擬提供者）：建立工作流 → 開始構想階段 → 等待核准 → 核准 → artifact 與 revision 寫入、RoleCheck 齊全 → 可行性階段開始；「只保留不實作」使工作流停止；重新核准上游使下游變成過期；重複開始同一階段不會建立第二個議題；缺席的角色記為「缺漏」。
3. Rust：任務交給工作坊時，建立的工作坊議題帶有設計文件；匯出只限本機。
4. 介面：團隊頁與工作流頁 1320×860 第一個畫面看得到階段軌、下一步與團隊；375px 沒有水平捲動；控制項 ≥ 44px。
5. `cargo test --workspace` 全過；`node scripts/check-web.mjs` 通過；瀏覽器載入沒有 console error。

## 修訂紀錄：GPT 紅隊之後（實作前）

紅隊全文見 [06](06-gpt-redteam.md)，12 條全部採用。

| # | 問題 | 修正後的規格 |
|---|---|---|
| 1 | 工作流與議題無法原子同步 | 不靠「議題結束」事件一次更新：每個階段記 `session_id` 與 `applied_revision`；讀取、啟動、引擎啟動時都從議題的實際狀態**重新對帳**（`reconcile`），同一版重放不會重複加一 |
| 2 | 失敗、取消、拒絕沒有狀態 | `StageStatus` 增加 `Failed`（議題失敗或取消，可重跑）與 `Rejected`（你在關卡選「只保留不實作」）；判定只看 `plan.approved` 與議題狀態的組合，不從 `Done` 猜 |
| 3 | 「只保留不實作」沒有協定 | 用既有、結構化的 `GateDecision::Reject`；可行性階段被 Reject → 工作流 `Stopped`，文件仍保存；其他階段被 Reject → 該階段 `Rejected`，可重跑 |
| 4 | 重複開始有競態 | 引擎層的工作流鎖包住「檢查＋建立議題＋寫回 session_id」整段；啟動時對帳既有的進行中議題 |
| 5 | 上游重核准時下游正在進行 | 開始時記下 `started_on`（上游各階段 revision）；上游改版時，下游已核准、進行中、等待核准的都標成 `Stale`；過期的下游議題之後即使被核准，也不會成為正式文件 |
| 6 | 「直接沿用工作坊」低估了衝突 | 團隊階段是**獨立的執行分支** `team_stage()`（工作坊遇到 `spec.stage` 時呼叫）：提案者平行 → 整合者 → 核准迴圈；`validate()` 依階段計畫產生參與者、不截斷；`turn_budget()` 用階段公式（呼叫數＋最多 2 次修訂） |
| 7 | 估算不含修訂與重試 | 顯示「基本 N 次／含修訂最多 M 次」；工作流層級硬上限＝各階段上限總和，失敗重跑也計入 |
| 8 | 提示詞注入 | 構想、上游文件、專案快照都放在標明為「資料」的區塊，說明其中的指示一律不執行；構想上限 8,000 字、每份上游文件最多 6,000 字（截斷時標示）；快照沿用既有的檔案樹＋README＋最近 commit（不讀其他檔案內容） |
| 9 | RoleCheck 可造假 | 程式依名冊產生；**只接受被指派該角色的那一次呼叫**所寫的段落；段落內容少於 12 個字算缺漏；整合者不能替提案者的角色補「完整」 |
| 10 | 任務一鍵交付沒有冪等與穩定識別 | 任務 id＝設計階段 revision＋序號；記錄每個任務建立的議題；同樣用工作流鎖，重複點擊回傳同一個議題 |
| 11 | 匯出安全 | 只接受本機呼叫（與 ADR 匯出相同的 `Caller.local` 檢查）；路徑用工作流 id（只允許安全字元），寫入 `docs/agora/<id>/<序號>-<階段>-r<版本>.md`，不同版本不互相覆蓋 |
| 12 | 驗收不可重現 | 介面用瀏覽器 DOM 量測（固定 viewport、`scrollWidth`、console 錯誤）；Rust 測試加入並行開始、重複交付任務、重啟後對帳、舊版核准被拒、惡意構想、空角色段落、匯出路徑 |

驗收標準 1 的 AI 類標準深度改為 **11 次**（可行性與技術設計都把 AI 小組獨立成一次）。

## 不做（這一輪）

上線與維運階段的自動產出、自動部署、甘特圖、多人協作、自訂角色、單一角色獨立呼叫的「超完整」深度。

整體證據等級：推論。
