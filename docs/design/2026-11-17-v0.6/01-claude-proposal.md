# v0.6 · Claude 的提案（第一輪，獨立撰寫）

> 寫完之前沒有看 GPT 的提案。輸入：[00-brief](00-brief.md)。

## 1. 診斷

### Serendipity-Epiphany（Claude Code harness）

| | 項目 | 證據 | 等級 |
|---|---|---|---|
| 優點 | **規則要有失敗證據才准常駐，消融只能用來刪**（單向門檻） | `.claude/ABLATION.md` 開頭；這正是 2026 年 harness engineering 的共識（每次換模型一次拿掉一個元件重測） | 已確認（檔案） |
| 優點 | 五級證據等級、領悟帳本（outcome、hits、失效條件） | `rules/evidence-grades.md`、`docs/lessons/INDEX.md` | 已確認 |
| 優點 | SDLC 角色模型（PM…DevOps）與 Agora 的 18 角色同源 | `.claude/ROLE_MODEL.md`、se-discovery／se-feasibility／se-design／se-system-design | 已確認 |
| 優點 | 最小實作階梯、兩軸審查（規格／標準分開）、派發單位（Coroutine／Thread／Process） | `se-minimal-change`、`se-two-axis-review`、`rules/dispatch.md` | 已確認 |
| 缺點 | **常駐面偏大**：入口＋規則 343 行；18 個 skill 的 description 共 7,599 字，全部競爭觸發預算 | `wc -l`、description 長度統計 | 已確認 |
| 缺點 | **自動觸發沒有量過**：`docs/eval/trigger-cases.md` 有案例，但沒有定期跑；在 `claude -p`、設定重的情況下 skill 可能完全不觸發（社群 issue） | eval runs 只有 2026-09-02／03 五份 | 推論 |
| 缺點 | **消融只跑過一次且無結論**（0/3 vs 0/3），門檻機制還沒有真的刪掉任何規則 | `docs/lessons/0001`、`docs/eval/runs/*ablate*` | 已確認 |
| 缺點 | **只服務 Claude**：Codex 版另外複製了 8 個 skill，兩邊已分歧（SE 有 se-feasibility、se-design、se-preflight；Codex 版有 se-git、se-kernel、se-memory） | 兩邊 `ls skills` | 已確認 |
| 缺點 | hooks 是 bash：Windows 上曾經靜默放行（lesson L0002）；plugin 安裝方式不帶常駐規則 | `.claude/hooks/*.sh`、README「B. 裝成 plugin」 | 已確認 |
| 缺點 | 入口分散：CLAUDE.md、.claude/CLAUDE.md、WORKFLOW、RUNBOOK、EXECUTION_MODEL、ROLE_MODEL、README 530 行，同一概念多處描述 | 檔案清單 | 已確認 |

### Serendipity-Epiphany-Codex（Codex harness＋runtime）

| | 項目 | 證據 | 等級 |
|---|---|---|---|
| 優點 | 寫入範圍衝突檢查才准並行、固定驗收命令、有限修復 | `src/se_codex/execution.py`、`routing.py` | 已確認（檔案） |
| 優點 | **記憶是資料不是政策**：准入、過期、隔離、撤銷、污染恢復演練 | `AGENTS.md`「Memory boundary」、`memory.py`、`recovery.py`、demo | 已確認 |
| 優點 | 誠實的試跑紀錄：明說「沒有證據宣稱比原版強」 | `docs/PILOT_RESULTS.md` | 已確認 |
| 缺點 | **預設核心模型不存在**：`config.toml` 的 `model = "gpt-6-astra"`，自己的文件說 CLI 清單沒有它，`doctor --live` 回 exit 3 | `.codex/config.toml`、README | 已確認 |
| 缺點 | **閉環比單次更差又更貴**：同任務 single-pass/Luna 原契約 2/2、closed-loop/Luna 1/2、closed-loop/Terra 0/2；token 400k → 523k–545k | `docs/PILOT_RESULTS.md` | 已確認（樣本小） |
| 缺點 | **runtime 與 Agora 重疊**：規劃、派工、worktree、驗收、整合、用量——Agora 的引擎都有，維護兩套 | `se.py run/agent` vs Agora `engine/` | 推論 |
| 缺點 | 模型名稱寫死（Sol／Terra／Luna／Astra），換代就壞；Agora 已經會讀 `~/.codex/models_cache.json` | `routing.py`、Agora `provider/codex.rs` | 已確認 |
| 缺點 | 專案 hooks 沒被信任就不跑（自己的評估寫到），記憶治理對一般使用太重 | README、`docs/EVALUATION.md` | 已確認 |

## 2. 改進方向：一份來源、兩種格式、Agora 當執行層

| 選項 | 做法 | 取捨 |
|---|---|---|
| A. 各自改進 | 兩個 repo 各修各的 | 分歧繼續擴大 |
| **B. 一份 skill 來源（建議）** | **SE 成為 harness 的唯一來源**：skill 一律寫成 Agent Skills 標準格式（只用 `name`、`description` 與共通內容），一個同步腳本產生 `.claude/skills/` 與 `.agents/skills/`；入口檔 `CLAUDE.md`／`AGENTS.md` 都由同一份 ≤100 行的目錄產生。SE-Codex 保留它獨有的價值（記憶治理、範圍衝突檢查、獨立使用的 runtime），skill 從 SE 同步 | 要寫同步腳本與測試；Claude 專屬欄位（例如 `context: fork`）放擴充區 |
| C. 合併成一個 repo | 刪掉其中一個 | 使用者明說要改進兩個，不合併 |

具體改進：

- **SE**：入口檔縮到 ≤100 行的目錄；把 18 個 skill 收斂成**有明確階段對應**的一組（下一節），重疊的併入（se-sdlc ↔ discovery／feasibility／design；se-clarify ↔ se-discovery；se-focus／se-scheduling 併入 dispatch 參考）；description 一律「做什麼＋何時用」一句話；觸發評估與消融做成可以重跑的腳本並真的跑一次；hooks 改成跨平台（Python 或 Node，與 Claude Code 官方 hooks 相容），每個 hook 附「它會擋」的自我測試（L0002 的教訓）。
- **SE-Codex**：模型改成讀 CLI 的能力清單（沒有就降級到 CLI 預設並說明）；預設改為**單次執行＋外部驗證**，閉環只在驗證失敗時有限修復（依自己的試跑結果）；skill 從 SE 同步；runtime 標成「不用 Agora 時的獨立模式」，Agora 存在時由 Agora 執行。

## 3. 導入 Agora

1. **harness 套件**：Agora repo 內 `harness/`（從 SE 同步、固定版本號與 commit），內含 `skills/<name>/SKILL.md`＋`references/`、`roles/`（18 個角色的職責與檢查清單）、`rules/`（證據等級、最小變更、驗證先於宣稱）。
2. **確定性注入，不賭自動觸發**：Agora 的 skill 登錄表解析 SKILL.md；**階段與角色 → skill** 的對應寫在程式裡（例如可行性分析 → `se-feasibility`；審查 → `se-two-axis-review`）。每次 CLI 呼叫把對應 skill 的本文放進提示詞的「方法」區塊（有字數上限，references 只給路徑讓模型按需讀）。`Turn.skills` 記錄這次用了哪些 skill 與版本——稽核與量測都靠它。
3. **安裝到使用者專案（第二階段）**：在專案設定按「套用 harness」，預覽差異後寫入 `.claude/skills`、`.agents/skills` 與入口檔的一小段；不覆蓋使用者自己的檔案。這讓使用者不透過 Agora 直接用 Claude Code／Codex 時也有同一套 skill。
4. **量測**：每個 skill 附兩種檢查——輸出契約（例如可行性分析必須有「建議：做／調整／不做」，由程式檢查段落，像現在的 RoleCheck）與觸發案例（給 `/skill-doctor` 或我們自己的評估腳本）。Agora 的「Skills」頁顯示每個 skill 被用了幾次、輸出契約通過率。

## 4. 第一批 skill（Claude／Codex 共用）

| skill | 用途 | Agora 使用處 |
|---|---|---|
| `se-discovery` | 把構想收斂成問題、使用者、範圍、成功指標、待你回答的問題 | 團隊：構想收斂 |
| `se-feasibility` | 技術／資料／AI／時程／成本／風險／安全，給「做／調整／不做」 | 團隊：可行性分析 |
| `se-prd` | 使用者故事＋可檢查的驗收標準、UX 流程、非功能需求 | 團隊：需求與體驗 |
| `se-system-design` | 架構、API、資料模型、ADR、韌性檢查（沿用既有 references） | 團隊：技術設計 |
| `se-work-order` | 把設計拆成**工作單**：每項有需求、`AC1…` 驗收條件、驗證指令、範圍 | 團隊：任務拆分；工作坊的方案 |
| `se-implement` | 最小實作階梯、TDD、禁止 stub、改完必須有新的驗證證據才准說完成 | 工作坊：執行者 |
| `se-two-axis-review` | 先逐條判定驗收條件（附證據），再看工程品質；輸出固定 JSON | 工作坊：審查者；團隊：整合者 |
| `se-debug` | 可重現、量測假設、一次一個變因 | 驗證失敗時的修訂 |
| `se-epiphany` | 收尾時把會再用到的領悟寫成 lesson（附失效條件） | 議題結束時建議 |
| `se-role-cards` | 18 個角色的職責與交叉檢查（references） | 團隊所有階段 |

撰寫規格：`name`＋一句 `description`；本文 ≤ 200 行，固定段落「何時用／步驟／輸出契約／驗證」；長內容進 `references/`。每個 skill 要有一個 Agora 能自動檢查的輸出契約。

## 5. 審查機制（參考 review-bridge）

| review-bridge 做法 | Agora | 理由 |
|---|---|---|
| 程式切出驗收條件（AC1…）、合約以 SHA-256 封存 | **採用**：方案核准時由程式從「驗收標準」切出 AC 清單，與方案文字一起封存（hash） | 不讓模型在審查時改寫標準 |
| 審查者逐條回傳 pass（附證據）／fail／cannot_verify，程式拒收缺漏、重複、未知 id、沒證據的 pass | **採用**：審查者輸出固定的 JSON 區塊；程式驗證；全部 pass 才是「審查通過」，有 cannot_verify 就交給你 | 現在的 ✅／❌ 表格與 `VERDICT:` 行是鬆散的文字解析 |
| 兩階段（superpowers：先規格、再品質） | **採用為一次呼叫的兩段**（規格判定＋品質發現），避免多一倍的呼叫 | 成本 |
| 三次 git 指紋（測試時、交給審查時、審查後） | **採用簡化版**：驗證時與審查結束時取 worktree 指紋，不同就記錄並把結論上限降為「推論」 | 便宜、能抓到「審查的不是測試的那一版」 |
| 審查者唯讀要先實際證明 | **第二階段**：Agora 已傳唯讀旗標；之後加一次性探測 | 這一輪先做邏輯 |
| 人按一次「是」才開始（授權卡） | 已有（核准關卡） | — |
| PostgreSQL、程式圖譜 MCP、PTY | **不做** | 對個人使用過重 |

## 6. 「議題改叫專案」

撞名的是 `Project`（工作資料夾）。建議：**議題 → 專案**（介面上所有「議題」改成「專案」），**原本的「專案」（工作資料夾）→「程式庫」**（英文 Repo）。API 與資料檔名不變（避免遷移），只改介面與文件用語。這是使用者要決定的命名，列為待決定事項。

## 7. 分階段實作

**第一階段（這一輪，完成後使用者就能用 Agora 驗收）**

1. SE：Agent Skills 格式的 10 個 skill、入口檔 ≤100 行、同步腳本（產生 `.claude/skills`、`.agents/skills`）＋測試；消融與觸發評估腳本可重跑（先不付費大量跑）。
2. SE-Codex：模型讀能力清單、預設單次＋外部驗證、skill 從 SE 同步、文件更新。
3. Agora：`harness/` 套件與登錄表（`/api/skills`）、階段／角色 → skill 的確定性注入、`Turn.skills` 紀錄、Skills 頁。
4. Agora：AC 切分＋封存、審查 JSON 驗證、規格／品質兩段、驗證與審查的指紋、結論上限規則。
5. Agora：議題 → 專案、專案 → 程式庫（依使用者決定）。

驗收：同步腳本產出的兩份 skill 內容一致（測試）；Agora 每個團隊階段與工作坊角色的 turn 都記錄了 skill；模擬審查回傳缺一條 AC 時被拒（測試）；有沒證據的 pass 時不算通過（測試）；指紋不同時結論上限為推論（測試）；介面上不再出現「議題」；`cargo test`、`check-web` 全過；SE 與 SE-Codex 各自的測試全過。

**第二階段**：安裝 harness 到使用者專案、審查者唯讀探測、用真的 CLI 跑觸發與消融評估、lesson → 規則的升級流程在 Agora 裡完成。

## 8. 風險與不做

- 確定性注入會讓提示詞變長：每個 skill 本文設上限，references 不內嵌。
- 共用 skill 寫成最小公約數，可能少了 Claude 專屬能力：專屬欄位放擴充區，兩邊都能讀。
- 改名牽動很多介面文字：只改顯示用語，不改 API。
- 不做：PostgreSQL、程式圖譜、多人協作、自動刪除規則（消融結果只給建議，由使用者決定）。

整體證據等級：推論。
