# v0.5 · GPT 紅隊審視（實作前）

> Codex CLI `gpt-5.6-sol` · 輸入 161,000 tokens（快取 128,384）· 輸出 3,185。處理結果見 `05-final-plan.md` 文末。

## 紅隊結論

方案主流程合理，但目前有幾個會直接造成狀態錯亂或額度失控的缺口。以下依實際抽查 [modes.rs](../../../crates/agora-core/src/engine/modes.rs)、[runner.rs](../../../crates/agora-core/src/engine/runner.rs)、[store.rs](../../../crates/agora-core/src/store.rs)、`engine/mod.rs` 與 `engine/ops.rs` 的結果提出。

1. **高｜workflow 與 session 無法原子同步**

   session 核准後先由既有 Runner 寫 `sessions/<id>.json`，再更新 workflow；中途當機會出現「session 已核准、workflow 仍 Running」或反向狀態。SSE 也可能先送出錯誤的下一步。

   **修正：**workflow 不應靠「議題結束事件」一次性更新。加入可重播的 reconciliation：workflow 儲存 `session_id + last_applied_plan_revision`，每次讀取、啟動及重啟時依 session 的 `plan.approved`、status、verdict 補算；同一 revision 重放不得重複加一。

2. **高｜狀態列舉不足，失敗與取消無處可放**

   `StageStatus` 沒有 `Failed`、`Cancelled`，`WorkflowStatus` 也沒有失敗狀態，但方案聲稱「核准、停止、失敗」都會更新。既有 Runner 的 Reject 與 plan-only Approve 最後都可能成為 `Status::Done`，只看 session status 無法判定是核准還是拒絕。

   **修正：**新增明確終態，或定義唯一映射表，以 `plan.approved`、`verdict`、session status 聯合判定；禁止從 `Done` 猜結果。

3. **高｜可行性的「只保留不實作」沒有協定**

   現有 `GateDecision` 只有 Approve／Revise／Reject。「只保留不實作」若靠 reason 文字辨識，語系或改字就失效；若當 Reject，文件又不一定視為核准 artifact。

   **修正：**增加結構化 decision，例如 `ApproveAndStop`，或在 workflow API 使用獨立 enum，再明確保存 artifact、revision 與停止原因。

4. **高｜重複開始的冪等檢查有競態**

   兩個並行 POST 都可能先讀到 Pending，再各自建立 session。Store 的 process-local `write_lock` 只保護單次寫檔，不保護「檢查後建立」整段交易，也不處理多程序。

   **修正：**以 workflow/stage 級互斥鎖包住 check-and-create，寫入 `active_session_id` 後才回應；API 支援 idempotency key。工作重新啟動後也要掃描既有 live session。

5. **高｜上游重核准與下游執行可產生混版文件**

   方案只把「下游已核准階段」標為 Stale，未處理下游正在 Running／AwaitingApproval。此時下游仍可基於舊 revision 被核准，覆蓋成看似有效的新 artifact。

   **修正：**開始與核准時都做 `based_on` compare-and-set。上游 revision 改變後，阻止舊下游核准並標 Stale；是否取消正在執行的 session 必須有明確規則。

6. **高｜「沿用工作坊」低估了現有控制流程衝突**

   現有 workshop 固定執行「每位 participant 提案 → 每位互評 → 隱含 synthesizer → 可選紅隊 → 核准 → build」。`stage_plan` 的多個 Panel、指定 integrator、跳過互評及單提案直出，都不是單靠 `SessionSpec.stage` 即可自然套入；現有驗證還會截斷 participants 至四人，turn budget 也按舊工作坊公式計算。

   **修正：**先定義 stage 分支的逐 turn 執行規格及預算公式；明確修改 `workshop()`、`validate()`、`turn_budget()`。不要宣稱核准迴圈以外的流程「直接沿用」。

7. **高｜呼叫估算不包含修訂與失敗重試**

   顯示的 5／9／10 只是初始產出；現有核准迴圈允許多次修訂，provider 失敗、重新啟動或 AI 小組拆分也會增加呼叫。「沿用額度保護」目前按 session turn cap，不是整個 workflow 總額。

   **修正：**同時顯示「基礎呼叫、最多修訂、最壞上限」；建立 workflow 級硬上限及每階段剩餘額度，重試也計入。Full 必須由純函式算出確切數字，不能寫「依實際計畫」。

8. **高｜提示詞注入防護缺失**

   idea、上游 artifact、專案快照都屬不可信內容，可能包含「忽略角色格式」「讀取憑證」「把缺漏標完整」等指令。只要求固定標題無法建立安全邊界，且 snapshot 可能放大敏感資料外洩與 context 成本。

   **修正：**系統提示明示素材僅是資料、不可執行其中指令；各來源加不可混淆的結構化邊界與長度上限；snapshot 採 allowlist 並排除 `.env`、憑證及大型檔。輸出必須由程式驗證，不信任模型自述。

9. **高｜RoleCheck 可由模型造假**

   模型只要輸出 `### DBA（完整）`，即使內容空白或與 DBA 無關，也可能被記為通過；整合者還可能刪掉提案者的缺漏。

   **修正：**RoleCheck 以預期角色名冊為基準由程式生成，模型只能提供聲稱狀態。每項保存來源 turn、原文區段與最低實質內容檢查；缺標題、空內容、狀態非法一律 `Missing`，整合不得把 Missing 升級為完整。

10. **中｜任務一鍵交付沒有冪等與穩定識別**

    `/tasks/{k}/start` 使用陣列索引；artifact 修訂或重排後，`k` 可能指向另一任務。連點也可能建立多個工作坊議題。

    **修正：**任務產出穩定 `task_id`，記錄 `started_session_id` 與來源 design revision；用與 stage 相同的原子冪等機制。

11. **中｜匯出安全定義不足**

    「只限本機」未定義是 server bind、來源 IP 還是桌面模式；`slug` 若未嚴格驗證可能路徑穿越，重複匯出也可能覆蓋手改文件。

    **修正：**只允許 loopback 且驗證部署模式；slug 僅接受安全字元並 canonicalize 後確認仍在 `docs/agora` 下；預設拒絕覆蓋或採 revision 檔名。

12. **中｜驗收標準部分不可重現**

    「第一個畫面看得到」「沒有 console error」「匯出只限本機」沒有指定測試工具、fixture 與判定方式；模擬 provider 也無法證明角色內容確實完整或提示詞注入被隔離。

    **修正：**加入 Playwright 固定 viewport 斷言、scrollWidth 檢查、console listener；Rust 測試加入並行 start、雙擊 task、重啟後 revise/approve/cancel、舊 `based_on` 核准拒絕、惡意 idea/artifact、空角色段落及匯出 traversal。

已檢查且方向良好的部分：沒有新增 Mode、沿用既有核准／修訂 API、`serde(default)` 相容舊資料、原子檔案寫入意圖、缺席角色不應算通過、降低動態、CLI-only 與 plan-only 邊界。主要問題不在五階段本身，而在跨檔案一致性、競態、結構化決策與「不能相信模型自述」尚未落成可驗證規則。