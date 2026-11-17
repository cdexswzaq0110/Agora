# v0.6 · 最終方案：Serendipity harness 導入 Agora、skill、可稽核的審查

> 由 Claude 整合兩份提案（[01](01-claude-proposal.md)、[02](02-gpt-proposal.md)）與兩份互評（[03](03-claude-critique.md)、[04](04-gpt-critique.md)）。實作前交給 GPT 紅隊（[06](06-gpt-redteam.md)），修訂紀錄附在文末。
> 使用者決定：議題 → **專案**；原本的專案（工作資料夾）→ **程式庫**。

## 評分（兩份互評的平均）

| 提案 | 正確性 | 簡潔度 | 風險（5＝最低） | 成本（5＝最低） | 可測試性 |
|---|---|---|---|---|---|
| Claude | 4 | 3 | 3 | 2.5 | 4 |
| GPT | 3.75 | 4 | 4 | 3.5 | 4.75 |

## 爭議的裁決

| 爭議 | 裁決 | 理由 |
|---|---|---|
| skill 的來源放 SE 還是 Agora | **SE repo 是來源**（`skills/`，Agent Skills 標準格式＋`skills/manifest.json`）；Agora 與 SE-Codex 各放一份**固定版本的副本**（版本號＋每個檔案的正規化 SHA-256），由同步腳本更新，各自的測試檢查副本與 manifest 一致 | 使用者要的是改進自己的 harness 並在一般開發中使用；SE 的消融與領悟帳本（規則演進的證據）在 SE。GPT 擔心的「Agora 發版依賴 SE 同步狀態」由固定副本解決：Agora 執行時不需要 SE |
| 名稱 `se-*` 還是 `agora-*` | **`se-*`** | skill 是可攜的方法論，Agora 之外也要用 |
| Agora 專屬格式放哪 | **放 Agora 的提示詞**：階段文件標題、角色小節、審查 JSON 由 Agora 規定；skill 只寫方法與「輸出要回答哪些問題」 | 避免 skill 綁死 Agora，也避免同一件事兩處定義 |
| 兩階段審查 | **兩次獨立呼叫**：規格審查（逐條 AC）全部通過後才做品質審查；品質審查只在規格通過時花錢 | GPT：同一呼叫無法證明獨立 |
| git 指紋 | **第一階段做三點**：驗證前、交給審查者前、審查結束後 | GPT：兩點分不出「測試後改」與「審查中改」 |
| 第一階段大小 | 9 個 skill 都做（Agora 每個階段與角色都用得到）；Skills 頁只做唯讀清單＋使用次數 | skill 多半是改寫 SE 既有內容；儀表板延後 |
| API 是否改名 | **不改 API 與資料欄位**；介面與文件改名，`docs/tech/` 加名詞對照表 | 改 JSON 欄位會打斷 MCP、排程、範本與手機快取 |

## 第一階段（這一輪）

### A. Serendipity-Epiphany（SE）

1. **canonical skills**：repo 根目錄 `skills/<name>/SKILL.md`（只用標準欄位 `name`、`description`；Claude 專屬欄位放 `metadata`），本文 ≤200 行，固定段落：何時用／不用時、步驟、輸出要回答的問題、驗證、反模式。長內容放 `references/`。
2. **9 個核心 skill**（由既有 skill 改寫、合併）：

| skill | 來源 | Agora 使用處 |
|---|---|---|
| `se-problem-definition` | se-discovery＋se-clarify＋se-design 的問題定義部分 | 團隊：構想收斂（PM、UX、資料分析師） |
| `se-feasibility` | se-feasibility | 團隊：可行性分析 |
| `se-product-contract` | se-sdlc 的需求部分＋新增「可檢查的驗收條件」 | 團隊：需求與體驗 |
| `se-technical-design` | se-system-design（references 沿用） | 團隊：技術設計 |
| `se-work-order` | 新增（參考 review-bridge 工作單：需求、AC1…、驗證、範圍） | 團隊：任務拆分；工作坊：方案 |
| `se-minimal-implementation` | se-minimal-change＋禁止 stub＋驗證先於宣稱 | 工作坊：執行者 |
| `se-spec-review` | se-two-axis-review 的規格軸 | 工作坊：規格審查；團隊：整合者 |
| `se-quality-review` | se-two-axis-review 的工程軸（smell-baseline） | 工作坊：品質審查 |
| `se-debug` | se-debug | 驗證或審查失敗後的修訂 |

   其餘既有 skill（se-ml-lifecycle、se-branch-lifecycle、se-bootstrap、se-skill-authoring、se-epiphany、se-preflight、se-context-language、se-focus、se-scheduling、se-sdlc）留在 `skills/extended/`，標為選用，不進 Agora 的預設注入。
3. **同步腳本**（Python 標準庫，`scripts/sync_skills.py`）：產生 `.claude/skills/`（Claude Code）與 manifest；`--check` 模式比對副本與 manifest，不一致就非零結束。**測試**：產物與來源一致、manifest hash 正確、正規化（CRLF→LF、去掉結尾空白）後 hash 穩定。
4. **入口精簡**：`CLAUDE.md` ≤100 行、只當目錄；「每輪收尾留 lesson」改為「有會再用到的領悟才寫」。
5. **hooks 失敗即擋**：改為跨平台（Python），每個 hook 附自我測試，證明「它會擋」（lesson L0002 的教訓）；hook 自身出錯時回傳阻擋，不是放行。
6. 文件：README 縮短並指向 `skills/`；變更記錄。

### B. Serendipity-Epiphany-Codex

1. `.agents/skills/` 改由 SE 同步（固定版本＋manifest，`scripts/sync_skills.py --check` 進測試）；SE-Codex 專屬的 `se-kernel`、`se-memory`、`se-recover`、`se-git` 保留在 `skills-local/`。
2. 模型：預設不寫死 `gpt-6-astra`；讀 Codex 的模型清單（`~/.codex/models_cache.json`），沒有指定的模型就降級到 CLI 預設並在 `doctor` 說明。
3. 預設改為**單次執行＋外部驗證**，閉環修復只在固定驗收失敗時啟動、有次數上限（依自己的試跑結果）。
4. 文件標明：有 Agora 時由 Agora 執行；`se.py` 是不用 Agora 時的獨立模式。

### C. Agora

1. **harness 副本與登錄表**：`harness/se/`（skills 副本＋manifest；不在 `web/`，不會被當成網頁資源），`build.rs` 不需要；伺服器啟動時讀取並驗證 hash，不一致就停用該 skill 並在 Skills 頁顯示原因（失敗即停，不靜默）。`GET /api/skills`：名稱、版本、hash、用途、使用處、注入次數、輸出契約通過率。
2. **確定性注入**：程式內的對應表 階段／角色 → skill。每次 CLI 呼叫把 skill 本文放進提示詞的「方法（skill：名稱@版本）」區塊；每個 skill ≤4,000 字、每次最多 2 個，超過截斷並標示；references 只列路徑（`harness/se/...` 的絕對路徑，模型需要時自己讀；唯讀呼叫也能讀）。`team.rs` 的 `stage_goal`／`duty` 保留為結構與名冊，方法論改由 skill 提供（單一來源）。
3. **skill 收據**：`Turn.skills: [{id, version, sha256, truncated}]`，只由程式寫入；輸出契約檢查結果寫入同一筆（例如可行性有沒有「建議」、規格審查 JSON 是否有效）。
4. **審查合約**（工作坊實作後）：
   - 方案核准時，程式從「驗收標準」切出 `AC1…ACn`，連同方案文字封存（`Plan.contract { sha256, criteria: [{id, text}] }`）。
   - **規格審查**（唯讀、獨立呼叫）：輸出 fenced JSON `review.result`：`{ "contract_sha256", "verdicts": [{ "id", "verdict": "pass|fail|cannot_verify", "evidence" }], "findings": [...] }`。程式拒收：JSON 無效、hash 不符、缺漏／重複／未知 id、pass 沒有證據、整體說通過但有 fail。拒收的審查算失敗，**不算通過**；最多重試一次，仍無效就停在「需要你確認」。
   - **品質審查**（唯讀、獨立呼叫）：只在規格全部 pass 時執行；輸出 `{ "findings": [{ "severity": "high|medium|low", "text", "location" }] }`；有 high 就要求修訂（計入修訂上限）。
   - **結論**：全部 AC pass 且無 high → 審查通過；有 cannot_verify → 交給你判斷；有 fail → 修訂；到上限 → 交付目前版本並列出未過的 AC。證據等級：驗證指令通過且指紋一致才可能「已確認」。
   - **git 指紋**（worktree 內，只讀 git）：驗證前、交給審查者前、審查結束後各取 `HEAD`＋`git diff --binary HEAD` 的 SHA-256＋未追蹤檔（路徑＋內容 hash）。驗證前≠審查前：記錄「驗證後又改動」；審查前≠審查後：該次審查不採計（`version_changed`）。沒有 git：指紋為未知，結論上限為推論，結果寫明。
5. **團隊階段**：整合者的審查改用 `se-spec-review` 的方法，但只做**文件契約完整性**（本階段必填段落、上游版本），不套程式品質審查。
6. **改名**：介面所有「議題」→「專案」，原「專案」（工作資料夾）→「程式庫」；API、資料欄位、檔名不變；`docs/tech/` 新增名詞對照表（專案＝session、程式庫＝project、工作流、方案、工作單、階段文件）。
7. **Skills 頁**（唯讀）：清單、版本、hash 狀態、用在哪些階段／角色、注入次數、契約通過率。
8. **驗收清單**：`docs/ACCEPTANCE.md`，使用者開始用 Agora 驗收時照著做（開一個真的程式庫、跑團隊工作流、交付一個工作單給工作坊、看哪些證據、遇到哪些情況要停下來回報）。

### 分工

| 項目 | 誰 | 方式 |
|---|---|---|
| A（SE） | GPT | 獨立 worktree（分支），不 commit；Claude 審查後在 SE repo 開分支提交 |
| B（SE-Codex） | GPT | 同上，A 完成後再做（要同步 A 的 skills） |
| C（Agora） | Claude | 主工作區；skills 副本在 A 完成後同步進來 |

## 驗收標準（第一階段）

1. SE：`python scripts/sync_skills.py --check` 通過；`skills/` 有 9 個核心 skill，每個 SKILL.md ≤200 行、`name` 與資料夾同名、`description` ≤1,024 字；hooks 自我測試證明「會擋」與「出錯時擋」；`CLAUDE.md` ≤100 行。
2. SE-Codex：`python se.py test` 全過；`.agents/skills` 與 SE manifest 一致（`--check`）；`doctor` 在指定模型不存在時降級並說明，不是 exit 3 卡住。
3. Agora：固定 fixture（模擬模型）跑完團隊五階段與一個工作坊，**每一則**團隊與工作坊 turn 的 `skills` 都符合對應表（測試逐一比對）。
4. Agora：規格審查的拒收測試——缺一條 AC、重複 id、未知 id、pass 沒證據、hash 不符、JSON 無效、整體通過但有 fail——每一種都不算通過。
5. Agora：品質審查只在規格全 pass 時呼叫（呼叫次數測試）；high 發現觸發修訂。
6. Agora：指紋測試——驗證後改檔被記錄；審查中改檔使該次審查不採計；沒有 git 時結論上限為推論。
7. Agora：harness 副本被竄改（hash 不符）時該 skill 停用並在 `/api/skills` 顯示原因。
8. Agora：介面（`web/`）不再出現「議題」二字（測試掃描）；名詞對照表存在。
9. `cargo test --workspace`、`node scripts/check-web.mjs` 全過；瀏覽器驗收 Skills 頁與審查結果的呈現（1320×860、375px）。

## 延後

- 第二階段：審查者唯讀探測（Codex 寫 sentinel 被拒、Claude 工具清單）。
- 第三階段：把 harness 安裝到使用者的程式庫（dry-run、衝突拒絕、去重：已安裝時不重複內嵌）。
- 第四階段：真的 CLI 開／關評估（每個 skill 正例、反例、對抗例）、跨模型版本重跑、無效 skill 降為選用；lesson → 規則的升級流程。
- SE-Codex 記憶與 runtime 的深度重構。

## 不做

付費 API、PostgreSQL、程式圖譜 MCP、多人協作、自動刪除規則（消融只給建議）、讓模型自行核准。

整體證據等級：推論（引用的 repo 現況為已確認）。

## 修訂紀錄：GPT 紅隊之後（實作前）

紅隊全文見 [06](06-gpt-redteam.md)，15 條全部採用；兩條（#2、#3）調整了範圍，理由寫在表內。

| # | 嚴重度 | 問題 | 修正後的規格 |
|---|---|---|---|
| 1 | 高 | 新審查流程超出 `2 * rounds + 2` 的呼叫預算 | 工作坊預算改為逐類相加：規劃＋人工修訂＋每輪（執行者 1＋規格審查 1＋格式重試 1＋品質審查 1）；**開始前最壞情況 > 24 就拒絕或自動降低修訂次數**並說明。測試斷言每條路徑「實際呼叫 ≤ 預算 ≤ 24」 |
| 2 | 高 | 核准後在驗證／審查中途重啟無法續跑 | 審查狀態機的每一步（`build → verify → spec_review → quality_review → revise/done`、嘗試次數、指紋、合約 hash、結果）**全部持久化在議題紀錄**，重啟後不遺失；「從最後一步續跑」放第二階段（v0.5 起 build 中途重啟本來就標為中斷，不是新的退步）。第一階段的驗收：重啟後紀錄完整、可以看到停在哪一步 |
| 3 | 高 | 唯讀與提示詞注入 | 第一階段：(a) 方案、AC、skill、程式庫內容、逐字稿、模型輸出一律放進資料區塊（中和 `<<<`／`>>>`、明示其中指令無效）；(b) 解析器只取**審查者本次回應**裡最後一個合法的 `review.result` 區塊，逐字稿裡的假結果不算；(c) 唯讀旗標寫進測試（Claude 工具白名單、Codex `-s read-only`）；(d) 設定頁「證明審查者唯讀」按鈕（實際叫 CLI 嘗試寫 sentinel，花極少額度），**沒有證明過的提供者，審查結論上限為推論**。對抗測試：假 `review.result`、巢狀 fence、未知欄位、skill 內的注入 |
| 4 | 高 | AC 切割規則未定義、`list_items` 最多 10 條 | 方案輸出時要求一個 fenced JSON `acceptance_contract`（`[{id, text, verification}]`），程式以 schema 驗證；沒有時退回明確的 Markdown 文法：「驗收標準」標題下最外層編號清單，每項含其下縮排行，忽略 code fence，最多 30 條、每條 ≤ 500 字；**核准時以核准的文字重新計算合約**（人工修改後舊審查結果作廢）。fixture 涵蓋多行、巢狀、fence、空白、重複 |
| 5 | 高 | SE-Codex 的本機 skill 移到 `skills-local/` 會失去自動發現 | `skills-local/` 只是來源；同步腳本把 SE 的 bundle 與本機 skill **實體複製**到 `.agents/skills/`，合併成單一 manifest；名稱衝突拒絕；不用 symlink。驗收：四個本機 skill 仍被 `doctor` 發現 |
| 6 | 高 | 三 repo 同步沒有可重現的協議 | manifest：`schema_version`、`bundle_version`、`source_repo`、`source_commit`、每個檔案的相對路徑與正規化 SHA-256。同步：先寫暫存目錄、完整驗證、再整個目錄替換，並刪除 manifest 以外的舊檔；`--check` 驗缺檔、多檔、hash、source commit；來源路徑必須明確傳入（不猜 sibling repo）。SE-Codex 的分支是 `feature/agent-execution`（預設分支），新分支從它開出 |
| 7 | 中 | 兩次審查不一定獨立 | 規格審查與品質審查各自用**最小上下文**：品質審查看不到規格審查的文字，只拿到產物（diff 摘要）、指紋、驗證結果與品質準則；收據記錄兩次審查是否同一家模型（`same_family`），同一家時不宣稱交叉獨立 |
| 8 | 中 | 證據只是非空字串 | verdict 加 `evidence_kind`（`command`／`code`／`doc`）：`command` 必須引用本次的驗證紀錄、`code` 必須是 worktree 內存在的相對路徑（可加行號）；`cannot_verify` 必須有 `reason`；品質發現有穩定 `id`。程式只確認「引用存在且屬於這個版本」，語義正確仍是模型判斷，不升格為已確認 |
| 9 | 中 | git 指紋的涵蓋範圍 | 定義為「git 追蹤的檔案＋未追蹤且未被 ignore 的檔案＋submodule 狀態」；逐檔串流 hash，不把整份 binary diff 放進記憶體；ignored 檔與環境變數明確列為**未涵蓋**；不宣稱排除審查期間的瞬間改動 |
| 10 | 中 | references 用絕對路徑不可靠 | 不讓模型自己去讀 Agora 的安裝目錄：需要的 reference 由 Agora 驗證路徑在 bundle 內（拒絕 `..`、絕對路徑、連結逃逸）後在字數預算內內嵌；沒有內嵌時收據標 `references_omitted` |
| 11 | 中 | 對應表沒有定義 | 見下方「skill 對應表」；測試的期望值寫在測試檔裡的獨立表格，**不與正式程式共用函式** |
| 12 | 中 | 模型降級的資料來源 | 能即時探測時以 app-server `model/list` 為準，cache 只作診斷；模型不存在時產生明確的 resolved profile（記錄要求的模型、實際模型、來源與原因），routing 永遠拿到有模型的 profile |
| 13 | 中 | hooks 失敗即擋可能把人鎖死 | 安全判定「拒絕」時擋；hook 本身出錯時只擋高風險操作，並給錯誤碼、停用／修復指令與一次性的 break-glass。自我測試：沒有 Python、錯誤 payload、逾時、含空白與中文的路徑、Windows 原生執行 |
| 14 | 低 | 「議題」掃描範圍太窄 | 掃描所有使用者看得到的字串：web、API 錯誤、事件、匯出、通知；**模型看的提示詞不算**（例如 `prompts.rs` 的 `## 議題` 標題，改了會影響解析）；以 allowlist 保留名詞對照表與歷史設計文件 |
| 15 | 低 | 統計分母未定義 | 注入次數＝實際送給提供者、非分岔複製的 turn；截斷另列。契約通過率＝有效檢查中的通過數／有效檢查數；格式拒收、提供者失敗、cannot_verify 分開列，不混入通過／不通過 |

## skill 對應表（第一階段）

鍵：模式＋階段＋操作（＋角色）。每次最多 2 個 skill，依表內順序。

| 模式 | 階段 | 操作 | skill |
|---|---|---|---|
| 團隊 | 構想收斂 | 產出／修訂 | `se-problem-definition` |
| 團隊 | 可行性分析 | 產出 | `se-feasibility` |
| 團隊 | 可行性分析 | 整合／修訂 | `se-feasibility`、`se-spec-review` |
| 團隊 | 需求與體驗 | 產出 | `se-product-contract` |
| 團隊 | 需求與體驗 | 整合／修訂 | `se-product-contract`、`se-spec-review` |
| 團隊 | 技術設計 | 產出 | `se-technical-design` |
| 團隊 | 技術設計 | 整合／修訂 | `se-technical-design`、`se-spec-review` |
| 團隊 | 任務拆分 | 產出／修訂 | `se-work-order` |
| 工作坊 | 方案 | 提案／整合／修訂 | `se-work-order` |
| 工作坊 | 實作 | 執行者（第一次） | `se-minimal-implementation` |
| 工作坊 | 實作 | 執行者（修訂） | `se-minimal-implementation`、`se-debug` |
| 工作坊 | 審查 | 規格審查 | `se-spec-review` |
| 工作坊 | 審查 | 品質審查 | `se-quality-review` |
| 其他模式 | — | — | 不注入（單人、協做、討論、辯論維持原樣） |

## 實作順序

1. **GPT（SE）**：A 全部，產出 bundle（含 manifest）。
2. **GPT（SE-Codex）**：B 全部，從 A 的 bundle 同步（明確傳入路徑）。
3. **Claude（Agora）**：先做不依賴 skill 內容的部分——預算公式、審查狀態機與持久化、AC 合約、審查 JSON 與證據檢查、指紋、資料區塊、改名；A 完成後同步 bundle，接上登錄表、注入、收據與 Skills 頁。
