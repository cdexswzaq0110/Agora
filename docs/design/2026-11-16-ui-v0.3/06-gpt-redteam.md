# 06 · GPT 紅隊審視（第三輪）

> 模型：Codex CLI `gpt-5.6-sol`（ChatGPT Plus 訂閱，非 API key）· 輸入 3,563,790 tokens（快取 3,318,144）· 輸出 27,789（推理 19,308）· 2026-11-16 10:48–10:58
> 輸入：`05-final-plan.md` ＋ 唯讀存取 `web/`。GPT 讀檔期間 Claude 仍在改 `session.js`，所以部分問題在它寫完前已修正 —— 處理結果見 `05-final-plan.md` 文末的「修訂紀錄」。

## 【品味評分】

🟡

方向合理，但目前仍有數個會直接造成焦點遺失、狀態過期、手機溢出或驗收結果不可重現的高風險缺口，還不能安全視為「最終方案」。

> 讀檔期間 v0.3 程式仍有更新；以下程式碼判斷以最後讀到的版本為準。（連結的行號是 GPT 讀檔當下的位置，之後的修改可能讓行號偏移。）

## 【致命問題】

1. **高｜SSE 重繪與焦點策略互相衝突**

   - 換頁時先聚焦「載入中…」的 `h1`，snapshot 後它會被替換，真正標題不會重新取得焦點。
   - `paintSession()` 為保留焦點而跳過 `head/gate` 重繪，可能讓「等待核准」、停止按鈕或舊核准面板永久停留；另一方面，timeline 每次完整重畫又會直接刪掉其中的鍵盤焦點。
   - 核准後立刻聚焦 `#sStatus`，不代表狀態 SSE 已經抵達，因此不符合「更新後的狀態列」。相關位置：[main.js](../../../web/js/main.js#L37)、[session.js](../../../web/js/session.js#L418)。

   **修正：** 不要以「區域內有焦點就完全不更新」作為保護。拆成穩定 DOM 節點，只更新文字與必要屬性；將 textarea 草稿存入 state。核准 API 後等待對應 `status/phase` 事件完成重繪，再聚焦新的狀態列。snapshot 首次完成後再聚焦真正的 `h1`。

2. **高｜即時宣告會重複，且漏掉階段變化**

   同一個 `awaiting_approval` 可能被：

   - 全域 SSE 宣告一次；
   - 議題 SSE 再宣告一次；
   - `refreshAttention()` 發現數量增加後再宣告一次。

   反而 `phase` 事件只重繪、不宣告，與方案 E 不符。[main.js](../../../web/js/main.js#L55)、[session.js](../../../web/js/session.js#L144)、[attention.js](../../../web/js/attention.js#L10)。

   **修正：** 建立單一 announcer，以 `session_id + event type + value` 去重；議題頁開啟時只由 session stream 宣告，其他頁由 global stream 宣告。明確列出哪些 phase 要朗讀，串流 delta 不朗讀。

3. **高｜「還有 N 項」計算與目的地資料集不同**

   待辦數量來自所有專案，但連結只捲到目前專案的議題紀錄。其他專案的待辦可能完全找不到，而且同名議題沒有專案名稱可辨識。[attention.js](../../../web/js/attention.js#L48)、[home.js](../../../web/js/home.js#L203)。

   **修正：** 「還有 N 項」應開啟全專案待辦清單，或自動切換到該項專案；每筆至少顯示專案名稱。不要導向單一專案 archive。

4. **高｜徽章與待辦可能在 SSE 漏訊後永久過期**

   `status` payload 只有 `session_id/title/status/error`，沒有專案、模式、階段、時間，因此前端必須重新抓列表。現在若 global stream 發生 `lagged` 或重新連線，中間狀態不會補回；多個並行 GET 也可能由較舊回應覆蓋新狀態。事件契約見 [event.rs](../../../crates/agora-core/src/event.rs#L12)。

   **修正：**

   - 在 `hello`、`lagged`、重新連線及 `pageshow` 時強制同步。
   - 為 refresh 加 request generation 或 `AbortController`，只接受最新回應。
   - 資料量增大後應加入 `status` 篩選或專用 attention endpoint，避免每次狀態變更讀取全部歷史。

5. **高｜「核准面板只有一層捲動」只處理了預覽，沒有處理編輯**

   `.plan-preview` 已取消內層捲動，但長方案切到 textarea 後仍會在固定高度內捲動；這仍是兩層垂直捲動。[app.css](../../../web/app.css#L625)、[app.css](../../../web/app.css#L457)。

   此外，開啟修訂框、200% zoom 或較矮螢幕時，決策欄可能高於 viewport，「一直看得到」不可能同時成立。

   **修正：** 編輯 textarea 自動依內容增高，或明文把文字編輯器列為唯一例外。驗收標準改成「決策欄頂端 sticky，且主要核准按鈕在指定 viewport、100% zoom、修訂框收合時可見」。

6. **高｜輪次 `<details>` 的摘要結構對無障礙不安全**

   目前把 `<ul>` 放在 `<summary>` 裡；這不符合 summary 的內容模型，也會讓 disclosure button 的 accessible name 包含所有發言摘要。長發言又保留第二層「展開全文」，所以「全部展開」並不代表全文全部展開。[session.js](../../../web/js/session.js#L351)。

   另外目前摘要顯示家族與角色，沒有 `t.model`；「第一句」實際上是第一個非空行，不是第一句。

   **修正：** `<summary>` 只放階段名稱、數量與狀態；一行摘要清單放在 summary 後方，利用 CSS 讓它在 closed 時顯示。明確決定是否移除 turn 級第二層收合。摘要應顯示模型名稱、中文字句號切分、長度截斷，以及無 usage/失敗/進行中的 placeholder。

7. **高｜語音與 Ctrl/Cmd+Enter 有實際退化風險**

   `bindVoice(view())` 每次 render 都在同一個 `#view` 加 delegated listener，議題頁重畫 gate 又再對子節點綁一次。導覽或切換預覽／編輯後，一次語音點擊可能被多個 listener 處理，剛開始便被另一個 handler 停止。[home.js](../../../web/js/home.js#L287)、[session.js](../../../web/js/session.js#L468)。

   Ctrl/Cmd+Enter 直接呼叫 `startSession()`，沒有 pending guard；快速按兩次可能建立兩個議題。[home.js](../../../web/js/home.js#L454)。

   **修正：** `bindVoice` 改成全站單例或以 `WeakSet` 保證每個 root 只綁一次；移除 gate 的重複 binding。新增 `starting` 鎖並忽略 `event.isComposing`。

8. **高｜流程摘要在兩種情況下會說錯話**

   - `autoApprove=true` 時仍顯示「你核准」。
   - 開啟同題比較時，摘要仍只描述目前單一模式，但實際會執行多種模式且工作坊會被改為只產出方案。

   這會讓使用者依錯誤摘要估計風險與成本。[home.js](../../../web/js/home.js#L124)。

   **修正：** 摘要必須從最終送出的 spec／experiment spec 產生，而不是平行讀取 composer state。為五種模式、auto-approve、plan-only、compare、直接改檔建立資料驅動測試表。

9. **高｜驗收標準目前無法重現**

   截圖沒有指定瀏覽器、DPR、字型、mobile 高度、fixture、CLI 健康狀態、localStorage、動態時間、baseline 路徑與像素容差。`adjustOpen` 還會從 localStorage 恢復，直接影響首屏高度。[home.js](../../../web/js/home.js#L225)。

   「import 檢查」也沒有定義；`node --check` 只驗證語法，不解析瀏覽器 import 或執行模組。

   **修正：** 固定 demo fixture、清空 storage、淺色、100% zoom、指定瀏覽器與字型；首屏改用 selector 的 `getBoundingClientRect().bottom <= 860` 驗證。定義截圖 baseline 與容差，並以瀏覽器載入後「console 無 error、所有 module 回傳 200」作 import 檢查。

## 【為什麼這是問題】

以下仍是中低風險，但會造成邊界失敗或驗收爭議：

- **中｜「375px 沒有水平捲動」定義過廣。** diff、程式碼與 Markdown 表格本來就需要局部水平捲動。[app.css](../../../web/app.css#L470)  
  **修正：** 改為 `document.documentElement.scrollWidth === clientWidth`，允許 `.patch`、`.md pre`、表格容器局部捲動。

- **中｜進行中與等待核准的同級排序未定義。** 現況採 `updated_at` 新到舊，但方案只寫「最高優先」。  
  **修正：** 明定同狀態是最舊等待優先或最新更新優先，並加入相同 timestamp 的穩定次排序。

- **中｜「更多」誤用了 ARIA menu pattern。** 現況宣告 `role="menu/menuitem"`，但沒有方向鍵、Escape、roving tabindex 等 menu 行為。[session.js](../../../web/js/session.js#L392)  
  **修正：** 最簡單是移除 menu roles，保留原生 `<details>` disclosure 與一般按鈕／連結。

- **中｜降低動態效果仍會平滑捲動。** CSS 停止動畫不會影響 JavaScript `scrollIntoView({behavior:'smooth'})`。[session.js](../../../web/js/session.js#L465)  
  **修正：** 用 `matchMedia('(prefers-reduced-motion: reduce)')` 決定 `auto` 或 `smooth`。

- **中｜sticky offset 是未同步的常數。** `--topbar-h` 沒有實際設定；topbar 因 meters、字型或縮放換行後，sticky 決策欄和階段錨點會被遮住。[app.css](../../../web/app.css#L607)  
  **修正：** 量測 topbar 高度寫入 CSS variable，或移除方案未要求的第二條 sticky session bar。

- **中｜手機仍有溢出與安全區風險。** 「更多」選單在手機改成 `left:0`，若按鈕靠右可能伸出 viewport；PWA 又使用 `viewport-fit=cover`，但 topbar 沒有 safe-area padding。  
  **修正：** 選單使用 viewport 內的 `max-width` 並靠右／clamp；topbar 與 footer 加 `env(safe-area-inset-*)`。

- **中｜首頁 archive 存在非同步競態。** 切換專案或快速搜尋時，舊請求只檢查 `#sessions` 是否存在，可能把 A 專案或舊查詢結果寫進目前畫面。[home.js](../../../web/js/home.js#L196)  
  **修正：** 每次請求記錄 project、query、sequence，回應時三者仍一致才套用；更新標記也只由最新請求清除。

- **中｜「正在更新」與空狀態的適用範圍不清。** archive 有標記，但 attention refresh、diff 載入、session snapshot、SSE 重連沒有一致規則。  
  **修正：** 列出必須顯示 updating 的非同步操作，以及失敗、無資料、初始載入三種空狀態文案。

- **中｜首屏條件沒有定義資料邊界。** 多個 CLI 未就緒、長專案名、範本提示、分岔 banner、待辦列和已展開調整都可能把按鈕推出 860px。  
  **修正：** 指定首屏驗收 fixture；另增加長文字、兩個 CLI 失敗、分岔及 200% zoom 的壓力測試，但不要要求所有壓力狀態仍符合 860px。

- **低｜徽章小字對比可能不足。** 橘色底配白色約 10px 文字不宜只靠目測。  
  **修正：** 實測 WCAG contrast，必要時改深色底或深色文字；同時測試 3 位數徽章不擠壓導覽。

- **低｜手機分段按鈕換行後邊框會不連續。** `button + button` 永遠加左邊框，第二行首項仍有左框但沒有上框。  
  **修正：** 手機改為明確 grid 版面或每個按鈕都有完整邊框。

- **低｜相對時間不會自行更新。** 待辦列若長時間開著，仍可能顯示「剛剛」。  
  **修正：** 每分鐘只更新時間文字，或改顯示固定時間。

## 【改進方向】

建議在開始正式驗收前先完成四件事：

1. 先修正 SSE 重繪、焦點與草稿保存，否則後續 a11y 測試會反覆失敗。
2. 將 attention 建成真正的全專案資料集，補上 reconnect／lagged 同步與請求競態保護。
3. 收斂 disclosure、tab、更多選單的原生語意，避免半套 ARIA pattern。
4. 把驗收條件改成可執行的固定 fixture、DOM assertion 與明確截圖基準。

## 【已檢查且方向良好的部分】

- `GET /api/sessions` 不帶 `project` 確實能取得所有專案，不需為徽章另改既有 API。
- 首頁 `budgetEstimate()` 與後端 `turn_budget()` 的五種模式公式目前一致。
- 原生專案／範本 select、模式 roving radio、核准 tabs 的方向正確。
- 完成頁將結論、工作區放在討論前，以及手機 DOM 中 rail 位於 timeline 前，方向符合需求。
- 核准預覽已取消 `max-height/overflow`，且手機決策區回到普通文件流。
- 動態字串大多仍經 `esc()` 或既有 Markdown renderer，未看到新增 API key 或付費 API 路徑。

本次只做靜態紅隊檢查，未修改檔案、未執行完整測試。由於工作區內容在檢查期間仍有更新，正式實作完成後應再做一次穩定版本 review。