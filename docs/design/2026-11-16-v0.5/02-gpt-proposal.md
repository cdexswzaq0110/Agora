# v0.5 · GPT 的獨立提案

> Codex CLI `gpt-5.6-sol`（ChatGPT 訂閱，非 API key）· 輸入 230,175 tokens（快取 179,456）· 輸出 4,762 · 寫在看到 Claude 提案之前。

# Agora v0.5 獨立方案：專案團隊 Agent 模式

## 1. 專案團隊工作流設計

### 核心定位

v0.5 不應新增另一套與「方案工作坊」競爭的引擎，而是在現有工作坊上加入「專案生命週期、角色責任、文件產出與多重核准關卡」。

```text
構想
→ 探索與可行性
→ 產品與 UX
→ 系統與資料設計
→ AI/ML 適用性
→ 開發計畫
→ 使用者核准方案
→ worktree 實作
→ QA／安全／維運審查
→ 使用者核准套用
→ 上線與維運
```

### 階段、負責角色與產出

| 階段 | 主責／審查 | 必要產出 | 使用者關卡 |
|---|---|---|---|
| 0. 構想澄清 | PM、UX、SA；Architect 審查範圍 | 問題陳述、目標使用者、情境、限制、非目標、待確認問題 | **核准問題定義** |
| 1. 可行性分析 | SA、系統架構師、資料分析師、AI 工程師 | 商業／操作／技術／資料／AI 可行性、方案比較、成本與主要風險 | 重大不可逆選擇才核准 |
| 2. 產品與 UX | PM、UX；QA 審查可驗收性 | PRD、使用者旅程、資訊架構、主要畫面流程、無障礙要求、產品驗收標準 | **核准 PRD 與 UX 流程** |
| 3. 系統設計 | Architect、系統架構師、系統設計師、SD、DBA；Dev／DevOps 審查 | C4/模組架構、資料流、資料模型、API 契約、錯誤與安全策略、ADR 草案 | **核准架構及資料破壞性決策** |
| 4. AI/ML 設計 | LLM、NLP、AI、ML、DL、MLOps 工程師；QA／資料分析師審查 | AI 適用性、提示詞與評測、資料需求、模型／基準線、風險與回退方案 | 需要模型、資料或額度承諾時核准 |
| 5. 交付規劃 | Dev、QA、DevOps/SRE；PM 審查範圍 | 實作 checklist、測試計畫、部署／回滾／監控計畫、可量測驗收標準 | **核准可執行方案** |
| 6. 實作與驗證 | Dev 實作；QA、SD、Architect 審查 | worktree diff、測試證據、逐條驗收表、未解風險 | **核准套用 diff** |
| 7. 上線與維運 | DevOps/SRE、QA、資料分析師；PM 驗收 | release checklist、健康指標、告警、維運手冊、成效報告 | 正式部署仍需明確核准 |

審查規則採「相鄰職能交叉審查」：PM 不自行核准技術可行性；Architect 不自行宣告 UX 合格；Dev 不自行判定測試通過；AI/ML 設計必須由 QA 與資料分析師檢查可量測性。

---

## 2. 角色名冊

「一定要出場」不等於每個角色都必須各呼叫一次。每個角色都要留下 `完整評估／簡短檢查／不適用` 的可稽核紀錄。

| 角色 | 職責；輸入 → 產出 | 出場規則 | 建議模型 |
|---|---|---|---|
| PM | 問題、限制 → PRD、優先級、成功指標 | 所有專案完整 | Claude：長文件一致性 |
| UX | 使用情境 → 旅程、資訊架構、a11y | 有人機介面完整；否則檢查操作流程 | Claude：情境與文字架構 |
| SA | 構想、現況 → 可行性、需求與資料流 | 所有專案完整 | GPT：拆解與邊界分析 |
| Architect | 非功能需求 → 技術方向、ADR | 跨模組或技術選型完整 | Claude：跨文件約束 |
| SD | 架構與需求 → 模組、介面、錯誤流程 | 有實作即完整 | GPT：具體技術設計 |
| DBA | 資料生命週期 → schema、遷移、備份 | 有持久化資料完整；否則檢查「無 DB」假設 | GPT：資料結構與邊界 |
| Dev | 核准方案 → 程式碼、diff、實作紀錄 | 實作型專案完整 | 依專案選擇；Agora 可交叉分派 |
| QA | 驗收標準 → 測試矩陣、證據、缺陷 | 所有可交付專案完整 | GPT：反例與可測性 |
| DevOps/SRE | 執行環境 → 部署、回滾、監控、SLO | 要上線完整；本機工具簡查 | Claude：操作風險與完整性 |
| LLM 工程師 | LLM 情境 → prompt、額度、評測、回退 | 使用 LLM 完整；否則判定不適用 | Claude：提示詞與長上下文 |
| NLP 工程師 | 文字資料 → 任務定義、語料與指標 | 搜尋、分類、抽取等完整；否則簡查 | GPT：形式化任務與指標 |
| AI 工程師 | AI 能力需求 → AI 系統整合方案 | AI 專案完整；否則防止硬套 AI | Claude：端到端整合 |
| 系統設計師 | 使用流程 → 元件互動、狀態機、API | 多元件專案完整 | GPT：狀態與介面精確度 |
| 系統架構師 | 全域限制 → 品質屬性、邊界與演進路線 | 跨服務或長期專案完整 | Claude：全域一致性 |
| 資料分析師 | 指標與資料 → KPI、事件、分析計畫 | 所有專案至少簡查；資料產品完整 | GPT：量測與分析方法 |
| ML 工程師 | 資料與目標 → baseline、特徵、訓練評估 | 有訓練／預測完整；否則不適用聲明 | GPT：實驗設計 |
| MLOps 工程師 | ML 流程 → 版本、部署、漂移監控 | 有模型生命週期完整；否則簡查 | Claude：治理與維運 |
| DL 工程師 | 深度模型需求 → 架構、算力、訓練風險 | 明確需要 DL 才完整；否則檢查是否過度設計 | GPT：模型與算力分析 |

模型只是預設偏好；審查者應優先使用另一家模型，避免同一模型同時設計與自我核准。

---

## 3. 成本控制

### 深度等級

| 等級 | 用途 | 建議呼叫 |
|---|---|---:|
| 快速 | 小修、單頁功能 | 4–5 次 |
| 標準 | 一般功能／子系統 | 8–10 次 |
| 完整 | 新產品、跨模組或高風險 | 13–16 次 |

標準模式可控制在 **9 次**：

1. PM＋UX：1 次  
2. SA＋資料分析師：1 次  
3. Architect＋系統架構師：1 次  
4. SD＋系統設計師＋DBA：1 次  
5. AI＋LLM＋NLP＋ML＋DL＋MLOps：1 次  
6. Dev＋DevOps/SRE：1 次  
7. QA 紅隊：1 次  
8. 總結方案：1 次  
9. 實作後交叉審查：1 次  

每次呼叫可以扮演同一責任群的多個角色，但輸出必須分角色列出結論與狀態。系統先依 `project_kind` 自動選擇深度；非 AI 專案仍讓六個 AI/ML 角色在同一次呼叫完成適用性檢查，通常每角 1–3 句。

另外設定：

- 每階段 token 上限與全流程 `turn_budget`。
- 顯示預估／已用呼叫數及 Claude、GPT 額度健康度。
- 快取已核准文件；需求未變時不重新呼叫。
- 修訂只傳受影響文件、段落留言與依賴摘要。
- 額度不足時降為簡短檢查，但不得把「未檢查」顯示成「通過」。

---

## 4. 與現有引擎整合

直接重用現有：

- 工作坊的平行獨立提案、互評、總結與紅隊。
- `awaiting_approval`、重啟後續跑及段落留言。
- worktree、diff、套用／捨棄與驗證指令。
- `EvidenceGrade`、逐條驗收審查、專案記憶。
- Markdown、ADR、lesson 匯出。

現有基礎可見於 [引擎說明](../../../docs/tech/02-engine-and-modes.md)、[model.rs](../../../crates/agora-core/src/model.rs) 與 [prompts.rs](../../../crates/agora-core/src/prompts.rs)。

建議新增：

```rust
ProjectWorkflow {
    id, project_id, title, depth, project_kind,
    current_stage, status, call_budget, stages, approvals
}

StageRecord {
    kind, status, role_checks, artifacts,
    dependencies, evidence_grade, started_at, completed_at
}

RoleCheck {
    role, provider, depth, result, rationale, turn_ids
}

Artifact {
    kind, title, revision, markdown, status,
    author_roles, reviewer_roles, acceptance_criteria
}
```

API 最小集合：

- `POST /api/workflows`
- `GET /api/workflows/{id}`
- `POST /api/workflows/{id}/stages/{stage}/run`
- `POST /api/workflows/{id}/gates/{gate}/decide`
- `GET /api/workflows/{id}/artifacts/{artifact}`
- `POST /api/workflows/{id}/artifacts/{artifact}/comments`

SSE 新事件：

- `workflow_stage_changed`
- `role_check_completed`
- `artifact_created`
- `artifact_revised`
- `workflow_gate_required`
- `call_budget_changed`

MVP 可讓 `ProjectWorkflow` 編排多個既有 session，先不重寫 `Mode` 或把十八個職稱硬塞進現有 `Role` enum。

---

## 5. 專案模式介面

主畫面採三區：

```text
左：生命週期階段軌
中：目前階段、文件與執行證據
右：角色狀態、額度、待你核准
```

頂部顯示專案目標、完成率、目前階段與 Tailscale 狀態。Tailscale 僅呈現「未偵測／已連線／HTTPS 8443 可用」、網址、複製與診斷，不再設計連線核心。

階段軌能看見已完成、進行中、阻塞、待核准；角色卡顯示本階段職責、模型、完整／簡查／不適用及產出連結。文件中心提供 PRD、UX、架構、資料模型、API、測試、部署等分頁，核准項目固定集中於「待你核准」。

動態效果分級使用：

- 頁面淡入上升、標題 split-text：只在首次進入。
- IntersectionObserver：文件卡首次進場。
- SVG 脈動：只表示執行中的角色關係。
- 漣漪：只用於新產出或待核准，不作裝飾性循環。
- Cmd+K：搜尋階段、角色、文件及操作。
- 聚光燈、磁吸、自訂游標、亂碼解碼只在精準指標裝置且動態開啟時使用。
- grain 必須低對比、`pointer-events:none`、`aria-hidden`。
- 跑馬燈不用於必要資訊。

所有效果共用指定 easing；提供動態開關並尊重 `prefers-reduced-motion`。關閉時不得延遲內容顯示。維持 `esc()`、CSP、焦點還原、`aria-live`、44px 觸控與深色對比。前端延伸應遵守 [11-frontend.md](../../../docs/tech/11-frontend.md)。

---

## 6. MVP 與後續

### v0.5 必做

- 專案模式與 8 階段工作流。
- 十八角色的責任設定、適用性與可稽核出場紀錄。
- 快速／標準／完整深度及呼叫預估。
- 文件產出、相依關係與三個核准關卡。
- 既有工作坊、worktree、驗證、證據、記憶、ADR 整合。
- 階段軌、角色卡、文件中心、待核准面板。
- 動態開關、降低動態與基礎動態語彙。
- Tailscale 8443 連線狀態呈現。

### 延後

- 自由拖曳工作流編輯器。
- 自訂角色與組織模板市場。
- 完整 C4／ERD 圖形編輯。
- 自動正式部署、資料庫遷移執行。
- ML 實驗追蹤平台及模型 registry。
- 多人權限與雲端同步。

---

## 7. 主要風險與驗收標準

| 風險 | 可量測驗收 |
|---|---|
| 角色爆炸造成額度耗盡 | 標準流程規劃期 ≤ 8 次、含實作審查 ≤ 10 次；超限前停止並要求核准 |
| 角色只是換標籤、沒有責任差異 | 十八角色皆有 `RoleCheck`，且包含結果、理由、來源 turn；不得缺項 |
| 文件互相矛盾 | 依賴文件修訂後，下游標成 stale；Rust 測試覆蓋失效傳播 |
| 重啟遺失階段 | 每個 gate 重啟後仍可續跑；離線 store round-trip 測試 |
| 跳過使用者核准便寫入 | 未核准前所有 turn 都是 read-only；Rust 測試確認不建立 worktree |
| AI 輸出被誤當事實 | 沒有驗證指令成功不得標為「已確認」 |
| DOM 更新破壞焦點 | SSE 重繪後焦點留在同一控制項；核准後才移至狀態列 |
| 動態影響閱讀 | reduced-motion 下動畫 duration/iteration 為 0 或停用；內容立即可見 |
| 行動版不可用 | 360px 寬無整頁水平捲動；互動目標至少 44×44px |
| XSS／CSP 回歸 | 動態惡意字串只能以文字呈現；無 inline script；`check-web.mjs` 通過 |
| 離線測試誤用真 CLI | 引擎整合測試只使用 `MockProvider`，並驗證每組呼叫數 |
| Tailscale 誤導 | UI 明確顯示實際 port 8443；未成功探測不得顯示「可連線」 |

實作完成時至少必須通過：

```bash
cargo test --workspace
node scripts/check-web.mjs
```

並以模擬提供者執行完整專案流程，確認十八角色紀錄、核准暫停、續跑、worktree 建立時機與逐條驗收證據。

本輪僅唯讀分析，未修改檔案，也未執行測試。