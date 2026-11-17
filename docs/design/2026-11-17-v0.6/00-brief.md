# v0.6 · 任務簡報：改進兩個 harness、導入 Agora、制定 skill、審查機制

> Claude 整理，作為 Claude 與 GPT 第一輪各自提案的共同輸入。證據等級照 Agora 的五級。

## 使用者原話（2026-11-17）

> https://github.com/fortes1219/review-bridge 議題改叫專案，且 Agora 的審查機制可參考這個專案。
>
> 目前第一個任務，我希望你和 GPT 參考各大網站論壇以及 GitHub 熱門的 harness、Agent 幫我深度思考、改進我的這兩個專案
> https://github.com/cdexswzaq0110/Serendipity-Epiphany-Codex
> https://github.com/cdexswzaq0110/Serendipity-Epiphany
> 我希望你們能改進它的缺點、強化優點，讓我能在往後工作開發專案中使用。也就是我希望你們改進這兩個 harness 後，導入 Agora 這個 Agent 專案裡，並且開始制定 skill。我之後會開始用 Agora 開始工作驗收。請你現在開始規劃實作我的要求。

## 範圍內的四個 repo（本機路徑，唯讀參考）

| repo | 本機 | 是什麼 |
|---|---|---|
| Serendipity-Epiphany（SE） | `D:\Github repo\Serendipity-Epiphany` | Claude Code harness：`.claude/` 6 條常駐規則、18 個 skill、14 個 agent、hooks（bash）、`ABLATION.md`（規則要有失敗證據才准常駐、消融只能用來刪）、`docs/lessons/` 領悟帳本、`docs/eval/`（觸發案例、ablate.py）。README 530 行 |
| Serendipity-Epiphany-Codex（SE-Codex） | `D:\Github repo\Serendipity-Epiphany-Codex` | Codex 版：`AGENTS.md`、`.agents/skills/`（8 個）、`.codex/agents`（Sol／Terra／Luna）、Python 標準庫 runtime `se.py`（kernel 規劃、寫入範圍衝突檢查、最多 3 個 worker、固定驗收命令、有限修復與升級、記憶准入／污染隔離／恢復、app-server） |
| Agora | `D:\Github repo\Agora` | Rust＋原生 JS 的多模型平台（v0.5）：專案團隊工作流（18 角色、5 階段、核准關卡、對帳、過期、任務交付）、方案工作坊（提案→互評→整合→核准→worktree 實作→驗證→審查）、只用 CLI 訂閱 |
| review-bridge（參考） | `C:\Users\HUANG\AppData\Local\Temp\claude\bd7765272b8dbb52\bde022de-86b5-427a-aa95-98fb0e72f6a9\scratchpad\ref\review-bridge`（第三方，只讀；`ARCHITECTURE.md`、`README.md`） | Claude Code／Codex 的可稽核 harness：工作單（Markdown）由**程式**切出合約並以 SHA-256 封存 → 一張授權卡、人按一次「是」→ 寫手迴圈 → 測試在沙盒外跑 → **獨立的唯讀審查者**逐條（AC1、AC2…）回傳 pass（附證據）／fail／cannot_verify，程式拒收缺漏、重複、未知 id、沒證據的 pass、蓋過失敗條件的 PASS → 測試時、交給審查者時、審查後三次取 git 指紋（HEAD＋diff＋未追蹤檔），不同就 `version_changed` → 工作證明（每次呼叫前後快照，範圍外的變更標為未驗證）→ 交付。審查者的唯讀旗標會先實際探測證明 |

## Claude 的網路研究摘要（2026-11-17）

- **Agent Skills 已是開放標準**（agentskills.io，2025-12 由 Anthropic 發布，2026 年中約 30–40 個工具採用，含 Codex CLI）：`SKILL.md`＋YAML（必填 `name`、`description`）。Claude Code 讀 `.claude/skills/`，Codex 讀 `.agents/skills/`；各家有自己的擴充欄位，其他工具會忽略。【推論：多個二手來源一致】→ SE 與 SE-Codex 目前維護兩套分歧的 skill，可以共用一份來源。
- **skill 自動觸發不可靠**：設定重（40+ skill、MCP）時，`claude -p` 模式下 skill 可能完全不觸發（Claude Code issue #34648）；描述會被 token 預算擠掉；一篇估計自動觸發約 50%。Claude Code 新增 `/skill-doctor` 回報 skill 是否真的被呼叫、花多少 context。【候選：社群回報與 issue，未在本機重現】→ **Agora 以 `claude -p`／`codex exec` 跑，應由 Agora 依階段與角色「確定性地」注入 skill，而不是賭模型自己載入**。
- **熱門 harness**：obra/superpowers（腦力激盪 → 設計 → 隔離 worktree → 詳細計畫 → 每個任務新的子代理＋TDD → **兩階段審查：先規格符合、再程式品質** → 最終驗證 → 合併）、Everything Claude Code、mattpocock/skills、addyosmani/agent-skills、wshobson/agents、Karpathy 行為準則。星數各來源差很多。【候選】
- **Harness engineering 共識**（Anthropic／OpenAI 整理、arXiv 2609.00006 十一個系統的原始碼研究）：入口檔約 100 行、當目錄用，細節漸進揭露；驗證要快、當作背壓（型別、lint、測試、瀏覽器）；**生成與評估分開**（模型會高估自己）；一個 session 一個任務、用檔案交接；禁止 stub；每次換模型重新評估、一次拿掉一個元件（＝SE 的消融紀律）；verify-on-stop（改了 code 卻沒有新的驗證證據就不准結束）；外部驗證優於自評。【推論】
- **反對聲音**：規則與 skill 會「單向長大」（skill hoarding tax、CLAUDE.md 超過 200 行是反模式、模型換代讓規則過期；Anthropic 宣稱新模型移除八成系統提示無損失——二手）。【候選】

## 使用者已經確立的約束

- 只用 CLI 訂閱，絕不使用付費 API key。
- 使用者自己的桌寵角色永遠不公開（Agora 的 `web/local/`）。
- 先討論再實作：各自提案 → 互評 → 整合 → 紅隊（實作前）。
- 介面與文件用繁體中文。
- 「議題改叫專案」：Agora 現有的「專案」是工作資料夾（`Project`），改名會撞名，需要提出處理方式。
