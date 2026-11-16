# CONTEXT — 共用詞彙

| 詞 | 程式中的名稱 | 意思 |
|---|---|---|
| 議題 | `SessionRecord` / `SessionSpec` | 一次完整的協作：模式、參與者、題目、發言、結論 |
| 發言 | `Turn` | 一次 CLI 呼叫，也就是某個模型以某個角色說一次話 |
| 輪 | `Turn.round` | 同一輪的平行發言看到的是同一份逐字稿 |
| 模式 | `Mode` | 方案工作坊 workshop · 單人 solo · 協做 relay · 討論 roundtable · 辯論 debate |
| 方案工作坊 | `Mode::Workshop` | 獨立提案 → 互評 → 整合（→ 紅隊）→ 核准關卡 → 實作 → 驗證 → 依驗收標準審查 |
| 核准關卡 | `Status::AwaitingApproval` / `GateDecision` | 方案整合好之後停下來等人：核准（可以修改）、要求修訂、只保留方案 |
| 方案 | `Plan` | 整合後的最終方案，包含驗收標準；記錄修訂次數、是否被修改、是否已核准 |
| 驗收標準 | 方案裡的「驗收標準」段落 | 審查者逐條打勾 ✅／❌／⚠️ 的依據 |
| 紅隊 | `spec.red_team` | 方案整合後，另一個模型專門找會讓它失敗的情境 |
| 角色 | `Role` | 提案者、評論者、總結者、執行者、審查者、與談人、正方、反方、裁判、情報編輯 |
| 提供者 | `Provider` | 驅動某個模型家族的 CLI 包裝：`claude`、`codex`，以及模擬用的 `mock-*` |
| 工作區 | `Workspace` | 實作的地方：`worktree`（隔離）或 `direct`（直接寫入專案） |
| 套用／丟棄 | `apply_workspace` / `discard_workspace` | 把 worktree 的變更變成專案裡未 commit 的修改，或整個捨棄 |
| 結論 | `Verdict` | 議題的最終判斷：outcome、標題、證據等級、未解決或待驗證的事項 |
| 證據等級 | `EvidenceGrade` | 已確認 · 推論 · 候選 · 未知 · 未驗證 |
| 驗證指令 | `verify_command` / `VerifyRun` | 專案自己的檢查；結論升到「已確認」的唯一途徑 |
| 插話 | `Interjection` | 議題進行中你補充的話，下一則發言起生效 |
| 分岔 | `SessionSpec.seed` | 從某一則發言之後另開新議題，延續之前的討論 |
| 盲審 | `spec.blind` | 逐字稿裡隱藏作者是哪個模型（顯示為成員 A、B） |
| 記憶壓縮 | `compact.rs` | 逐字稿超過預算時，把較早的發言改成摘要，並加上可見的標記 |
| 專案記憶 | `Project.memory` / `memory.rs` | 跨議題的決策與教訓；太長時依議題檢索最相關的條目 |
| 專案快照 | `repo.rs` | 檔案樹、README 開頭、最近的 commit，附在提案與討論的提示詞裡 |
| 額度 | `QuotaSnapshot` / `QuotaWindow` | 訂閱方案的時段額度（5 小時窗、每週窗） |
| 額度保護／改派 | `QuotaSnapshot::blocks` / `reroute_candidate` | 快用完時改派給另一個就緒的模型，或直接拒絕 |
| 發言上限 | `Engine::turn_budget` | 每個議題最多幾次 CLI 呼叫（硬上限 24） |
| 計費保護 | `PAID_API_ENV`、`apiKeySource`、`account.type` | 確保永遠不會用到 API key 計費 |
| 專案團隊／工作流 | `team::Workflow` | 從一個構想到任務清單的五個階段（構想收斂、可行性分析、需求與體驗、技術設計、任務拆分），存在 `workflows/<id>.json` |
| 團隊角色 | `team::ROLES` / `TeamRole` | 使用者列的 18 個職能角色（PM…DL 工程師）；和流程角色 `Role` 不同，是資料不是 enum |
| 階段 | `StageKind` / `StageSpec` | 一個階段＝一個 plan-only 工作坊議題（`SessionSpec.stage`），沿用核准關卡 |
| 呼叫計畫 | `stage_plan` / `StagePlan` / `Panel` | 每個階段由哪家模型、一次扮演哪些角色；深度：快速、標準、完整 |
| 角色紀錄 | `RoleCheck` | 每個角色在階段裡的「完整／簡查／不適用／缺漏」，由程式依指派產生 |
| 已過期 | `StageStatus::Stale` | 上游文件改版後，依據舊版的下游文件；要重跑才算數 |
| 對帳 | `team::reconcile` | 從議題紀錄重新推導階段狀態，冪等 |
| 本機擴充 | `web/local/`、`whoami.extensions` | 只放在自己電腦上的介面擴充（例如個人的桌寵角色與人設）；不進 git，公開版沒有，執行時才依伺服器回報載入 |
| 個人設定隔離 | `isolate_cli_config` / `clean_config` | CLI 不載入你個人的 MCP 工具、外掛和 hooks |
| 每日技術情報 | `Digest` / `DigestItem` | 每天整理一次國外社群的熱門項目，附引用編號 |
| 範本 | `Template` | 一鍵套用的議題設定（取自 skill 庫） |
| 同題比較 | `Experiment` | 同一題依序跑多種模式，比較等級與成本 |
| 排程議題 | `Schedule` | 每天或每週固定時間自動開的議題（一律唯讀） |
| 配對 | `Pairing` / `PairedDevice` | 手機用 6 位數配對碼換得長效 cookie |
