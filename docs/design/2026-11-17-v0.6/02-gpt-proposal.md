# Agora v0.6 第一輪獨立提案

本提案依本機原始碼、測試紀錄與官方資料獨立形成；未參考或推測 Claude 的方案。證據標級沿用：**已確認／推論／候選／未知／未驗證**。

## 1. 兩個 harness 的診斷

### Serendipity-Epiphany（SE／Claude）

| 診斷 | 證據 | 等級 |
|---|---|---|
| 優：Experience→Lesson→Ablation→Rule 的治理鏈值得保留 | [README.md](<D:/Github repo/Serendipity-Epiphany/README.md:17>)、[ABLATION.md](<D:/Github repo/Serendipity-Epiphany/.claude/ABLATION.md>)；規則沒有失敗證據便不得常駐 | 已確認 |
| 優：Process／Thread／Coroutine 清楚區分隔離、唯讀與同 context 能力 | [EXECUTION_MODEL.md](<D:/Github repo/Serendipity-Epiphany/.claude/EXECUTION_MODEL.md>)；README 也明確區分 mutex 與 dependency | 已確認 |
| 優：規格與工程品質採雙軸審查 | `se-two-axis-review`、`thread-reviewer-spec`、`thread-reviewer-standards` | 已確認 |
| 優：記錄自己的工具失敗，而非掩飾 | `docs/lessons/0002` 記錄 Windows hook 靜默放行；`0003` 記錄 eval 量尺從 1/8 變 4/4 的問題 | 已確認 |
| 缺：skill 自動路由不能作為可靠控制面 | [CLAUDE.md](<D:/Github repo/Serendipity-Epiphany/CLAUDE.md:9>) 說依語意載入；最新紀錄僅 4/8 案例通過，debug、minimal-change、review、branch 均未穩定觸發 | 已確認 |
| 缺：常駐規則仍有自證循環 | `0001-ablation-first-run.md` 指出 19 個常駐項目有 4 個「未登記」；dispatch 消融 full/absent 都是 0/3，沒有測得效益 | 已確認 |
| 缺：hook 宣稱是 gate，但跨平台 fail-open | `0002-hook-silent-failure-windows.md` 明載「寫錯會放行」；hook 為 Bash，Windows 安裝卻只是褨貝檔案 | 已確認 |
| 缺：18 skills、14 agents、6 rules 的邊界重疊 | design/discovery/clarify/feasibility、scheduling/SDLC、角色模型均要求模型先正確選路；INDEX 本身已成第二套路由規則 | 推論 |
| 缺：Lesson 預設每輪都寫，容易造成帳本與規則單向增長 | [CLAUDE.md](<D:/Github repo/Serendipity-Epiphany/CLAUDE.md:29>)；`0005` 自己承認尚未通過 VALIDATE | 已確認 |
| 缺：完成證據主要是文字紀律，缺少統一 runtime receipt | evidence grade、HANDOFF、review 都由模型產文；沒有像 Agora/review-bridge 的程式化逐條驗收與版本指紋 | 已確認 |

**處置：**強化消融、最小變更、雙軸審查；刪除強制每輪 Lesson、無效果證據的常駐 dispatch 細則；合併 discovery/design/clarify/feasibility 為一個「問題定義」skill，由參數決定輸出，不讓模型在四個近義入口間猜。

### Serendipity-Epiphany-Codex（SE-Codex）

| 診斷 | 證據 | 等級 |
|---|---|---|
| 優：真正以程式落實依賴、寫入衝突、有限修復與整合前驗收 | `routing.py`、`execution.py`；tests 涵蓋重疊寫入、循環、repair、budget、隔離整合 | 已確認 |
| 優：記憶有 revision、receipt、epoch、污染隔離與恢復 | [IMPLEMENTATION_PLAN.md](<D:/Github repo/Serendipity-Epiphany-Codex/docs/IMPLEMENTATION_PLAN.md:124>)、`memory.py` 與相應測試 | 已確認 |
| 優：不把環境缺陷誤報成功 | [README.md](<D:/Github repo/Serendipity-Epiphany-Codex/README.md:80>) 明載 Astra 缺少、hooks 未受信任，doctor 回 exit 3 | 已確認 |
| 缺：runtime 過重，偏離日常專案 harness 的 MVP | `memory.py` 約 97 KB，另有 agent/execution/runtime/transport/benchmark 等多套子系統；安裝後帶入 `.se-codex` runtime | 已確認 |
| 缺：實測成本與成功率不理想 | `PILOT_RESULTS.json`：closed-loop 六次僅一次 accepted；單次約 24–28 萬 tokens，direct 兩次皆 accepted | 已確認 |
| 缺：驗收仍可能漏掉關鍵條件 | pilot 的 routing 任務通過當時 acceptance，事後 review 才發現 kernel/specialist fallback 缺陷 | 已確認 |
| 缺：模型配置過時會直接阻塞核心路徑 | README 指定 `gpt-6-astra`，同頁又確認本機清單不存在且不降級 | 已確認 |
| 缺：安全保證依賴尚未受信任或覆蓋不全的 hooks | [INSPECTION.md](<D:/Github repo/Serendipity-Epiphany-Codex/docs/INSPECTION.md:83>) 明載 prompt 唯讀不等於 OS 唯讀、hook fail-open 且部分工具不覆蓋 | 已確認 |
| 缺：skills 是 SE 的縮寫副本，已經分叉 | SE 18 個，Codex 8 個；同名 `se-debug`、`se-system-design` 等內容與長度不同，沒有共同版本來源 | 已確認 |
| 缺：記憶事故系統超出 Agora v0.6 的直接需求 | Agora 已有專案記憶與工作流狀態；完整污染閉包、effects compensation 會形成第二套狀態權威 | 推論 |

**處置：**保留 task contract、write-scope、有限 repair、receipt；將記憶災難恢復與自製 agent runtime 留在 SE-Codex 的進階模式，不嵌入 Agora。Agora 不應包一層 `se.py` 再包 Codex CLI。

## 2. 共用關係與 Agora 架構

建立一份平台中立的 **Agora Harness Pack**：

```text
Agora 內建 canonical skills
        ├─ 編譯／安裝 → .claude/skills/
        └─ 編譯／安裝 → .agents/skills/
Agora engine → 依階段/角色確定選 skill → 內容直接注入每次 CLI prompt
```

canonical 來源包含 `skill_id`、語意版本、適用階段、角色、必要輸入、輸出契約及驗收案例；Claude/Codex 目錄只是產物，不再人工維護兩份。使用者專案只安裝短入口、版本 manifest 與可選平台檔，不覆寫既有 `CLAUDE.md`／`AGENTS.md`；衝突時先列出並停止。

每次呼叫由 Agora 選定 skill，將其必要正文及 `skill_id@version + SHA-256` 注入 prompt；不能依賴 `claude -p` 自動觸發。模型回覆需帶 `SKILL_RECEIPT`，但**真正採用證據**由程式判定：prompt 指紋、輸出 schema、必要段落與行為檢查。追蹤：

- eligible／injected／receipt／contract-pass 四個比例；
- 相同案例 skill-on/off 的通過率、修訂輪數、token、時間；
- 模型版本更新後重跑 trigger、negative、adversarial 案例。

官方實務也支持短入口、漸進揭露及 repo-local skills，而非巨型指令檔：[OpenAI harness engineering](https://openai.com/index/harness-engineering/)、[skill eval 指南](https://developers.openai.com/blog/eval-skills)、[OSS skill 實務](https://developers.openai.com/blog/skills-agents-sdk)。

## 3. 第一批 skills

| Skill | 用途 | 階段／角色 | 共用 |
|---|---|---|---|
| `agora-problem-definition` | 收斂問題、假設、範圍、待決策 | 構想／PM、UX、SA | 是 |
| `agora-feasibility` | 技術、資料、AI 必要性、成本與停止條件 | 可行性／Architect、QA | 是 |
| `agora-product-contract` | 產生逐條、可觀察的使用者驗收條件 | PRD／PM、UX、QA | 是 |
| `agora-technical-design` | 架構、資料、API、取捨與回滾 | 技術設計／SD、DBA、SRE | 是 |
| `agora-task-contract` | 將設計切成有 scope、AC、verify 的工作單 | 任務拆分／Dev、QA | 是 |
| `agora-minimal-implementation` | 依合約做最小改動並留下證據 | 工作坊實作／Writer | 是 |
| `agora-spec-review` | 逐 AC 判定 pass/fail/cannot_verify | 第一階段審查／Reviewer | 是 |
| `agora-quality-review` | 規格通過後才查品質、安全與維護性 | 第二階段審查／Reviewer | 是 |
| `agora-debug` | 重現→假設→量測→最小根因→回歸驗證 | 修復輪／Dev | 是 |

規格限制：單一職責、明確 trigger/anti-trigger、必填輸入、固定輸出 schema、最多一個主流程、不得自行授權或改驗收；細節放 references。驗收至少含 5 positive、5 negative、3 adversarial，以及 on/off 對照；無可量測改善者不進預設包。

## 4. 審查機制

| 機制 | 決定 |
|---|---|
| 程式從核准方案切出 AC1…ACn、scope、verify，封存 SHA-256 | **照搬概念**；Agora 已有人核准關卡，可直接封存核准版本，不另做第二張授權卡 |
| reviewer 獨立 session、逐條 pass/fail/cannot_verify，pass 必附證據 | **照搬** |
| 測試由 Agora 在 agent sandbox 外執行 | **照搬**，避免作者自報與 Windows sandbox 誤判 |
| 審查前後取 HEAD＋tracked diff＋untracked manifest 指紋 | **照搬**；不一致即 `version_changed` |
| 唯讀只寫在 prompt | **廢除**；Codex 實測寫 sentinel，Claude 檢查 plan mode/tool 清單；無法證明則 `cannot_verify` |
| 兩階段審查 | **採用但有門檻**：先規格、再品質；小型低風險任務可在同一唯讀 session 分兩段，不能把兩種判準混成一個總評 |
| PostgreSQL、常駐服務、全域工作單資料庫 | **不採用**；Agora 已有狀態權威，重做會增加部署與一致性風險 |
| 團隊五階段文件 | 每階段採「文件契約完整性審查」；只查本階段必填項與上游版本，不套用程式碼品質審查 |

目前 Agora 只用 prompt 宣告唯讀（[prompts.rs](<D:/Github repo/Agora/crates/agora-core/src/prompts.rs:47>)），整合者也是自由文字指出錯漏（[team.rs](<D:/Github repo/Agora/crates/agora-core/src/team.rs:744>)）；這正是 v0.6 應改成機械合約的缺口。

## 5. 「議題改叫專案」

建議 UI 將現有 `Project`（工作資料夾）改稱 **「工作區」**，原「議題」改稱 **「專案」**：

- 工作區：實體 repo／資料夾及其記憶、CLI 環境。
- 專案：一次有目標、討論、核准、實作與審查的工作。
- 團隊工作流可稱「專案計畫」，內部資料型別暫不全面改名；先改顯示文案與 API DTO，避免高風險 migration。

## 6. 分階段實作計畫

| 階段 | 交付與可檢查驗收 |
|---|---|
| P1：立即可驗收 | canonical manifest、上述 9 skills、每次 CLI 確定注入、skill/version/hash receipt；工作坊建立 AC 合約；reviewer 逐條回覆。以固定 fixture 證明 Claude/Codex prompt 都含相同 skill hash，缺 AC、未知 ID、無證據 pass 均拒收 |
| P2：可信審查 | sandbox 唯讀探測、外部 verification、三次 Git 指紋、`version_changed`、兩階段審查。用故意寫檔 reviewer、審查途中改檔、假稱測試通過三個案例驗收 |
| P3：安裝更新 | project installer、dry-run、衝突拒絕、lockfile、升降版與 migration。重複安裝須冪等；不覆寫使用者檔案 |
| P4：評測與精簡 | skill on/off、跨模型版本、token/時間/修訂數 dashboard；連續兩版無效的 skill 降為 opt-in 或刪除 |

## 7. 風險與不做的事

主要風險是 prompt 膨脹、skill 過度切碎、Claude/Codex sandbox 能力不對稱、模型升版造成 eval 漂移，以及驗收條件寫得太弱卻被機械「正確」通過。

本輪不做：付費 API、另建 agent runtime、複製 SE-Codex 記憶資料庫、導入 PostgreSQL、全域覆寫使用者設定、讓模型自行核准、把 reviewer prompt 當唯讀證明、一次搬入全部 26 個既有 skills。

本輪僅唯讀分析，未修改檔案，也未執行 repo 測試；數據引用既有測試與 pilot 紀錄。

**整體證據等級：已確認。** 架構建議本身屬「推論」；repo 現況、eval/pilot 數據與現有 Agora/review-bridge 行為均有原始檔案證據。