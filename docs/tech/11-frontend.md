# 11 · 前端

原始碼：`web/`。原生 HTML、CSS、JS（ES modules），**沒有建置步驟**。release 版由 `rust-embed` 內嵌進執行檔；debug 版直接從磁碟讀取，改完重新整理就看得到。

v0.3 的介面改版是先讓 Claude 與 GPT 各自提案、互評、整合，再由 GPT 紅隊審視後才實作；完整過程在 [`docs/design/2026-11-16-ui-v0.3/`](../design/2026-11-16-ui-v0.3/05-final-plan.md)。

## 模組

| 檔案 | 責任 |
|---|---|
| `js/main.js` | 路由（`#/`、`#/s/:id`、`#/x/:id`、`#/digest`、`#/usage`、`#/settings`）、啟動、全域 SSE、通知、主題切換、換頁後聚焦 `h1`、量測頂欄高度（`--topbar-h`） |
| `js/state.js` | 全域狀態 `S`、詞彙（模式、角色、狀態）、提供者與額度的輔助函式、HP 條、頂部用量表、下拉選單 |
| `js/util.js` | `esc`、`api`、`toast`、時間格式化、`copyText`、`localStorage` 包裝 |
| `js/md.js` | 安全的 Markdown 轉換（先跳脫再轉換）、證據等級標籤、VERDICT 標記、✅／❌、情報的 `[n]` 引用連結 |
| `js/attention.js` | 「待你處理」：全專案的等待核准／進行中清單、頂欄徽章、分頁標題 `(N) Agora`、去重的螢幕閱讀器宣告 `announce(text, key)` |
| `js/home.js` | 首頁工作台：待辦列、輸入區（專案、範本、模式分段按鈕、一行流程摘要、可收合的「調整」）、議題紀錄搜尋 |
| `js/session.js` | 議題頁：SSE、只在內容改變時重繪的 `swap()`、核准面板（方案文件＋sticky 決策欄）、依輪收合的討論、階段目錄、精簡標題列、「更多」選單、工作區與 diff、分岔 |
| `js/digest.js` | 情報頁（v0.4）：精簡標題＋「和昨天相比」、章節目錄、預設收合的「全部 N 項」（來源／只看新上榜／只看關注主題）、趨勢徽章、畫出之後才標記已讀 |
| `js/usage.js` | 用量頁：真的 CLI 用大卡片、模擬模型收成「離線示範」一列；SVG 圖表（走勢小圖、堆疊柱狀圖、提示框、表格檢視） |
| `js/settings.js` | 設定頁（v0.4）：章節目錄、`data-k` 草稿與「未儲存的變更」列、手機連線（QR、網卡選擇、倒數換碼；GPT 實作）、推理強度預設值、快捷鍵開關 |
| `js/shortcuts.js` | 鍵盤快捷鍵（v0.4）：`?` 說明（原生 `<dialog>`）、`g h/d/u/s` 換頁；可關閉（WCAG 2.1.4） |
| `js/experiment.js` | 同題比較的結果表（執行中每 2.5 秒輪詢一次） |
| `js/pair.js` | 手機配對畫面（從 `#pair=` 預先填入配對碼） |
| `js/voice.js` | 語音輸入（Web Speech API，`zh-TW`）；每個 root 只綁一次 |
| `widget.html` / `widget.js` | 置頂用量小工具 |
| `sw.js` / `manifest.webmanifest` | PWA：網路優先，離線時用快取的外殼；API 一律不快取。**新增 module 時要加進 `SHELL`**（`scripts/check-web.mjs` 會檢查） |

## 首頁：工作台

由上而下：精簡標題 → CLI 狀態 → **待辦列**（最多顯示 1 筆，等待核准優先；「還有 N 項」就地展開全專案清單）→ **輸入區** → 議題紀錄。

- 輸入區的契約是那一行**流程摘要**：它描述伺服器實際會執行的流程，包括自動核准、只產出方案、同題比較（全部唯讀；工作坊只產出方案並自動核准，與 `Engine::start_experiment` 一致）。
- 參與者、選項、同題比較、專案記憶收在「調整」裡，預設收合。
- 示範模式下，每個家族預設選模擬模型，即使真的 CLI 可用（[lessons/0009](../lessons/0009-demo-mode-defaulted-to-real-cli.md)）。
- Ctrl/Cmd+Enter 開始；有 `starting` 鎖，輸入法組字中的 Enter 不算。

## 議題頁：先看成果

順序：頁首 → 工作坊進度條 → **核准面板**（只在等待核准時）→ 結論 → 工作區 → 方案 → 討論過程（左）＋側欄（右；手機排在討論之前）。

- **核准面板**：左邊是方案文件（預覽與編輯都沒有內層捲動，編輯框隨內容長高），右邊是 sticky 的決策欄（驗證指令、核准、要求修訂、只保留方案）。預覽／編輯是 ARIA tabs，可用 ←→、Home、End 切換；目前分頁存在 `S.planTab`，修改內容存在 `S.planEdit`，兩者分開。窄螢幕上決策欄排在方案下面，所以另有「跳到核准決定 ↓」。
- **討論過程**：每一輪是 `<h3><button aria-expanded aria-controls>` 的揭露區塊；收合時下方顯示每則發言一行摘要（角色、第一句、模型、耗時、tokens），點摘要會展開並捲到那則發言。預設：進行中只展開最新一輪；等待核准或完成時全部收合。使用者手動改過的才記在 `S.phaseOpen`。
- **側欄**：階段目錄（點擊會展開並聚焦該輪）→ 概況 → 過程紀錄（手機預設收合）。
- **精簡標題列**：標題捲出畫面後出現（`position: fixed`，不會讓內容跳動）；等待核准時有「前往核准」。
- 次要操作（複製結論、下載 Markdown、寫成 ADR、lesson 草稿、以此為範本）收在原生 `<details>` 的「更多」裡；Esc 關閉並還原焦點；手機上是貼底的面板。

## v0.4 的頁面規則

- **推理強度**：`state.js` 的 `effortsFor(provider, model)` 和伺服器的 `supported_efforts` 用同一條規則（找不到模型就用「CLI 預設」那一項，也就是所有模型的交集）。選項第一個是「模型預設」；只有指定了模型、而且 CLI 有回報預設值時才寫出是哪一級。換模型時，不再支援的值會改回模型預設並提示。
- **段落留言**：伺服器在方案上附 `blocks`（`##`／`###`，跳過程式碼區塊）。前端依順序比對「同層級、正規化後文字相同」的標題，只在對得上的標題後面放「留言」按鈕；對不上就不放，絕不掛錯段落。留言記下寫的是第幾版，方案改版後舊留言標成「原段落已變更」，改當一般意見送出。
- **記憶建議**：結論下方是可編輯的一行，附上實際會寫入的預覽；永遠由使用者按下才寫入。
- **設定頁草稿**：要按儲存的欄位帶 `data-k`。畫面畫好後先記下伺服器的值（baseline），再把 `S.setDraft` 套回去；比對兩者決定「未儲存的變更」列是否出現。立即生效的操作只重畫自己的區塊（手機連線），或重畫後再套回草稿，所以不會吃掉還沒儲存的修改。有草稿時離開頁面會先詢問，重新整理或關閉視窗有 `beforeunload`。
- **手機（≤ 760px）**：同一個 `.nav` 在手機上固定到底部，徽章與情報點照常顯示。所有貼底的元素用 `--bottomnav-h` 計算位置，層級固定為：底部導覽 25 < 選單／未儲存列 30 < toast 50 < `<dialog>`。虛擬鍵盤出現時（有可輸入元素聚焦，而且 `visualViewport` 高度少於視窗的 75%）隱藏底部導覽。
- **快捷鍵**：在輸入框、輸入法組字中、按著 Ctrl／Alt／Meta、或有對話框開著時都不觸發；`?` 用 `event.key` 判斷（不同鍵盤配置都可以）。說明對話框是原生 `showModal()`：背景自動不可操作，關閉後焦點回到原本的元素，那個元素不在了就回到 h1。

## 即時串流的處理

1. 打開 `EventSource('/api/sessions/{id}/events')`，第一個事件是 `snapshot`。
2. 後續事件更新 `S.rec`：
   - `delta` 依 `seq` 去重：伺服器先訂閱再取快照，所以兩者之間的事件可能重複收到。
   - 結構性的變化（發言開始或結束、階段、方案、結論……）排一次整頁重繪。
   - 文字片段只重繪那一則發言和它的一行摘要。
3. 重繪排程用 `nextFrame()`（v0.5 起在 `util.js`，議題頁與工作流頁共用）：`requestAnimationFrame` 加上 250 ms 計時器備援，頁面在背景也會更新（[lessons/0005](../lessons/0005-raf-does-not-fire-in-hidden-pages.md)）。
4. 議題結束時關閉串流；連線中斷時改成抓一次完整紀錄。
5. 全域串流收到 `hello`（每次連線都會送）、`lagged`，或分頁回到前景時，重新同步待辦與徽章；`refreshAttention` 和 `loadSessions` 都只接受最新一次請求的回應。
6. 偵錯用：`window.__agora` 可以看到狀態，`window.__agora.evlog` 是最近收到的事件類型。

## 重繪、焦點與宣告（v0.3）

- `swap(el, html)`：markup 跟上次一樣就不動 DOM；要換的時候，用 id（或 summary 的父 id、第一個 data 屬性）把焦點還原到新節點，並保留打開的「更多」選單。
- 核准面板只有在「還在等待核准，而且焦點在面板裡」時才跳過重繪；草稿、修訂文字、驗證指令都存在 `S`，所以重繪也不會遺失。
- 換頁後把焦點移到 `h1`；SSE 更新不移動焦點。核准或送出修訂後，等**伺服器的新狀態畫出來**才聚焦狀態列（`S.pendingFocus`）。
- 長期存在的節點上的綁定要小心：`#view` 跨頁面存在，事件委派只能綁一次；觀察器要跟著被替換的節點重新觀察（[lessons/0010](../lessons/0010-long-lived-bindings-outlive-rerendered-nodes.md)）。
- `announce(text, key)` 寫進 `aria-live="polite"`，同一個 key 15 秒內只說一次；全域串流與議題串流用同一個 key（`session_id:status`），階段用 `session_id:phase:名稱:發言數`。
- `prefers-reduced-motion` 也套用到 JS 的 `scrollIntoView`（`motion()`）。

## 安全

- 所有動態值都先 `esc()` 才放進 `innerHTML`；圖表提示框用 `textContent`。
- CSP：`script-src 'self'`，所以不能寫 inline script；`style-src` 允許 inline，因為 SVG 用到 inline style。
- Markdown 的連結只轉換 `http(s)://`，並加上 `rel="noopener noreferrer"`。
- 頁內錨點（`#phase-3`、`#planActions`）一律攔截點擊：hash 是路由，直接跳會被當成換頁。

## 設計語言（取自 github.io）

暖灰中性色、全大寫字距的編號標籤（`01 / PROJECT`）、細線網格的卡片、直角按鈕、實心黑底的選中狀態、脈動的狀態點。另外提供深色主題，可以跟隨系統或手動切換。

- 手機（≤ 760px）：按鈕、summary、輸入框、頂欄導覽至少 44px 高；模式按鈕是三欄 grid；不可有整頁水平捲動（diff、程式碼、表格可各自捲動）；頂欄、頁尾、插話列考慮 `env(safe-area-inset-*)`。
- 徽章用 `#a3501f` 配白字（5.64:1）；原本的 `#c8742e` 只有 3.50:1。

## 圖表規則（dataviz）

- 線寬 2px，末端圓點 r=4，外圍加一圈 2px 的畫布色。
- 格線是細實線、顏色低調。
- 兩個系列以上才放圖例；末端數值標籤會互相重疊時就不顯示。
- 提示框顯示所有系列的數值，數值粗體、名稱淡色。
- 每張圖都有「表格檢視」。
- 配色經 `validate_palette.js` 驗證（見 [06](06-quota-and-usage.md)）。

## 檢查

```bash
node scripts/check-web.mjs
```

## v0.6 審查、Skills 與唯讀證明

- 專案頁由 `js/review.js` 呈現封存的驗收合約、最新採用的規格審查判定、品質發現與可收合審查紀錄；`session.js` 只提供容器與重繪掛勾，並在發言標頭顯示該次注入的 skill。
- `#/skills` 由 `js/skills.js` 讀取 `/api/skills`，列出 bundle 來源、核心與延伸 skills、hash、注入及審查統計。入口位於命令面板、團隊首頁與設定頁，主導覽仍維持五格。
- 設定頁的模型區顯示每個提供者的唯讀證明。本機可呼叫 `POST /api/skills/prove-readonly` 進行一次小型 CLI 證明；未證明的審查結論最高只到「推論」。

把 `web/js/*.js` 當 ES module、`web/*.js` 當一般 script 解析；確認每個具名 import 都存在於目標 module 的 export；確認 `sw.js` 快取了每個 module。不要用 `node --check web/js/x.js`：Node 24 上它對 module 裡的語法錯誤也回傳 0（[lessons/0008](../lessons/0008-node-check-misses-esm-syntax-errors.md)）。

## v0.5 專案團隊介面

- `#/team` 由 `js/team.js` 繪製新構想表單、專案工作流清單與 18 個角色名冊；專案選擇與首頁共用 `S.projectId`。
- `#/w/:id` 顯示五階段進度、唯一下一步、目前角色分工、角色檢查紀錄、核准文件與可交付工作坊的任務。文件內容一律交給 `md()`，其他動態文字一律經 `esc()`。
- 全域 SSE 的 `workflow` 事件交給 `onWorkflowEvent()`；團隊頁只重載清單，工作流頁以 `nextFrame()` 合併連續事件後重抓資料（v0.5 驗收時原本用裸的 `requestAnimationFrame`：瀏覽器面板裡 rAF 不觸發，第一個畫格永遠排著，之後的事件全被丟掉；改用 `nextFrame()` 後即時更新正常）。即時重繪保留捲動位置及文件 `<details>` 開合狀態，也不主動移動焦點。
- 團隊階段仍使用一般議題頁核准。`session.js` 只補工作流麵包屑、發言角色標籤，以及「構想收斂」核准前的問題回答欄位，不改既有 SSE 重繪排程。

## 本機擴充（v0.5）

個人內容（例如自己的桌寵角色、助理人設）放在 `web/local/`，被 `.gitignore` 排除，公開的 repo 與 Release 都沒有。

- 伺服器在 `/api/whoami` 回傳 `extensions`：`web/local/<name>.js` 存在於這個建置裡才會列出（`rust-embed` 在編譯時嵌入磁碟上的檔案）。
- `main.js` 開機後對每個擴充載入 `/local/<name>.css` 與 `import('/local/<name>.js')`，存在 `S.extensions[name]`，呼叫它的 `init()`；設定頁「其他」呼叫 `settingsHtml()` 與 `bindSettings()`。
- 公開程式碼不 import、不引用任何擴充的檔名；`sw.js` 不快取擴充（擴充需要連線才會載入）；`check-web.mjs` 只檢查 `web/js/`。
- 助理聊天的人設：`web/local/persona.txt` 存在時替換角色段落，規則與資料邊界固定由伺服器附上。
