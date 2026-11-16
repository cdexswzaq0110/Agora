# UI v0.3 · GPT 的獨立提案

> Codex CLI · gpt-5.6-sol（reasoning effort high）· 唯讀 · 讀了 33 次 repo 檔案 · 輸入 798,475 tokens（快取 728,960）· 輸出 12,571 tokens。以下為原文。

## 我看到的問題

1. **P0：首頁把核心動作埋在 2,300px 之後。**  
   議題輸入框位於 y≈2,301，使用者必須先經過品牌介紹、專案、9 張範本、5 種模式與參與者設定，才可以描述需求。依據：截圖 1–3；`web/js/home.js` 的 `renderHome()` 依序輸出 Hero → Project → Playbook → Mode → Cast → Brief。這與「先提出問題、取得更好方案」的核心任務相反。

2. **P0：等待核准沒有形成明確待辦。**  
   待核准議題只在首頁最底部的議題紀錄卡片加上左側色條；使用者留在頁面時沒有頂層提醒。依據：截圖 4；`home.js` 的 `sessionCard()`、`loadSessions()`；`main.js` 的 `openGlobal()` 只更新底部列表，瀏覽器通知還受頁面隱藏狀態限制。

3. **P0：核准面板有雙重捲動，決策按鈕與方案內容分離。**  
   方案預覽自己捲動，核准／修訂按鈕又位於預覽框下方；閱讀長方案時容易失去操作上下文。依據：截圖 5–6；`session.js` 的 `planHtml()`；`app.css` 的 `.plan-preview { max-height: 34rem; overflow: auto; }`。

4. **P1：議題頁以完整逐字稿為主，而不是以成果與下一步為主。**  
   7 則發言就形成 5,013px 頁面。`paintSession()` 永遠展開所有 round；`turnHtml()` 只有文字超過 2,400 字才折疊，而且折疊後仍可高達 22rem。使用者真正要判斷的是方案、驗收標準、驗證結果與 diff。依據：截圖 6；`session.js` 的 `paintSession()`、`turnHtml()`；`app.css` 的 `.turn.collapsed .body`。

5. **P1：設定散落在多個畫面區段，開始前沒有完整摘要。**  
   專案、模式、模型、執行者、隔離、驗證指令分散於不同區塊；到「開始」按鈕時，使用者無法快速確認這次將如何執行。依據：截圖 2–4；`home.js` 的 `castHtml()`、`optionsHtml()`、`buildSpec()`。

6. **P1：核心「方案工作坊」和次要模式的視覺權重過於接近。**  
   五種模式都是同尺寸卡片，工作坊只有上方 3px 漸層與「核心」標籤。加上前面 9 張範本，首次使用者必須先理解大量選項。依據：截圖 2；`state.js` 的 `MODE_ORDER`；`app.css` 的 `.mode-grid`、`.mode-card.core`。

7. **P2：手機版主要是把桌面內容全部堆成單欄，沒有重新安排任務優先順序。**  
   375px 下首頁會更長；議題頁的 `.rail` 取消 sticky 後仍位於時間軸之後，使用者要讀完逐字稿才看到概況；頁首次要操作也會大量換行。依據：`app.css` 的 980px／760px media queries，以及 `renderSession()` 中 Timeline 先於 Rail 的 DOM 順序。

8. **P2：核准區的互動語意與即時狀態通知不完整。**  
   預覽／編輯容器宣告 `role="tablist"`，但按鈕沒有 `role="tab"`、`aria-selected`、`aria-controls`，內容也不是 `tabpanel`；SSE 更新的階段與待核准狀態沒有 `aria-live`。依據：`session.js` 的 `planHtml()`、`paintSession()`。

## 我的方案

1. **建立頁首「待你處理」工作列｜工作量：中**  
   - 要改什麼：任何等待核准或需要處理的議題，都顯示在首頁最上方，優先於建立新議題。  
   - 為什麼：核准是工作坊的阻塞點，不能依賴使用者到 2,785px 的紀錄區尋找。  
   - 怎麼做：在 `home.js` 新增 `attentionQueueHtml()`；`loadSessions()` 取得既有列表後，篩出 `awaiting_approval` 與 `running`，同時填入 `#attentionQueue` 和原本的 `#sessions`。每張卡只顯示狀態、標題、目前階段與「檢視並核准」。`main.js → openGlobal()` 收到 status event 時刷新此區。可在 `index.html` 的「議題」導覽旁加入數量徽章。現有 API 已包含 status，不必先改後端。

2. **把首頁改成 Action-first Composer｜工作量：中**  
   - 要改什麼：縮短 Hero，將專案、議題輸入、當前流程摘要與「開始」放進第一個畫面。  
   - 為什麼：核心任務是描述問題並啟動協作，不是先閱讀產品介紹。  
   - 怎麼做：重排 `home.js → renderHome()`：
     ```html
     <section class="hero compact">一句價值主張＋CLI 狀態</section>
     <section id="attentionQueue"></section>
     <section class="composer-shell">
       <header>專案選擇＋目前流程摘要</header>
       <textarea id="prompt"></textarea>
       <footer>開始方案工作坊＋呼叫預估</footer>
       <details class="composer-settings">調整範本、模式、參與者與進階設定</details>
     </section>
     ```
     在 `app.css` 新增 `.hero.compact`、`.composer-shell`、`.composer-summary`；保留既有暖灰、細線、直角控制項。

3. **把核准區改成「方案文件＋決策欄」｜工作量：中**  
   - 要改什麼：取消方案內層垂直捲動，讓核准操作持續可見。  
   - 為什麼：閱讀與決策應使用同一個頁面捲動上下文。  
   - 怎麼做：修改 `session.js → planHtml()`：
     ```html
     <section class="plan-gate">
       <header>版本、狀態、預覽／編輯切換</header>
       <div class="plan-layout">
         <article class="plan-document">完整方案</article>
         <aside class="plan-actions">驗證指令、核准、要求修訂、只保留方案</aside>
       </div>
     </section>
     ```
     `app.css` 移除 `.plan-preview` 的 `max-height/overflow`；桌面版讓 `.plan-actions` sticky，手機版改成單欄，主要決策列在方案區域內 sticky 到底部。

4. **把逐字稿改為依階段折疊的證據區｜工作量：大**  
   - 要改什麼：方案、結論、驗證、diff 保持優先；提案與互評預設按階段折疊。  
   - 為什麼：逐字稿是可追溯證據，不應主導整頁。  
   - 怎麼做：調整 `session.js → paintSession()`：
     - 等待核准：先顯示核准面板；「獨立提案」「互評」「紅隊」以關閉的 `<details>` 呈現。
     - 進行中：只自動展開目前階段與最新發言。
     - 完成：先顯示 Verdict、Workspace、驗證結果；完整討論放在「查看討論過程」。
     - 增加「全部展開／全部收合」，但不刪除任何發言。  
     `turnHtml()` 可降低長文折疊門檻，並讓總結、驗證和錯誤訊息保持展開。

5. **降低模式與範本的選擇成本｜工作量：中**  
   - 要改什麼：將方案工作坊作為明確預設；其他四種模式與範本改為次級調整。  
   - 為什麼：目前所有選項權重相近，使用者必須先理解產品架構才能開始。  
   - 怎麼做：在 `home.js → renderHome()`、`templatesHtml()` 中，讓摘要固定顯示「方案工作坊 · Claude + GPT · 人工核准 · 隔離實作」；其他模式放入「其他協作方式」細線網格，範本先顯示名稱式快速選擇，描述放在展開區。保留 `MODES`、`buildSpec()` 與目前資料結構，不改執行邏輯。

6. **為桌面與 375px 手機分別安排操作優先級｜工作量：中**  
   - 要改什麼：不只切成單欄，而是重新排序概況、主要動作和次要工具。  
   - 為什麼：手機堆疊後，重要內容可能落在數千像素之後。  
   - 怎麼做：
     - `session.js → renderSession()` 將概況 Rail 在 DOM 上放到 Timeline 前，桌面再用 CSS grid areas 放右側。
     - 頁首只保留狀態與當前主要操作；下載、ADR、lesson、重用等放進「更多」`<details>`。
     - 375px 下按鈕最小高度 44px、全寬排列、無水平溢出。
     - `app.css` 新增明確的 `.session-summary`、`.session-tools` 和手機 grid-area 規則。

7. **補齊核准流程的鍵盤與即時狀態語意｜工作量：小**  
   - 要改什麼：修正 tabs、焦點與 live status。  
   - 為什麼：待核准是關鍵操作，鍵盤與輔助技術不能只看到視覺變化。  
   - 怎麼做：在 `planHtml()` 加上完整 tab／tabpanel ARIA；在待辦數量、階段狀態加入 `aria-live="polite"`；路由完成後將焦點移到頁面標題；核准或送出修訂後把焦點移到更新後的狀態訊息。保留 `prefers-reduced-motion` 現有處理。

## 我刻意不建議的事

- 不導入 React、Vue、Tailwind、打包器或新的建置步驟。
- 不使用 inline script；維持目前 CSP。
- 不把核准流程做成 modal。長方案、手機鍵盤與深連結都不適合 modal。
- 不再增加另一個固定高度的捲動容器。
- 不刪除逐字稿；只降低預設視覺權重，保留完整稽核能力。
- 不預設自動核准，也不把它做成醒目捷徑；這會破壞核心人工關卡。
- 不全面改成圓角卡片、漸層背景或大型陰影；沿用現有網站的暖灰、細線網格、字距標籤與直角按鈕。
- 第一輪不重整 `digest.js`、`usage.js`、`settings.js`；它們不是目前核心流程的主要阻塞點。
- 不新增 API key 或雲端模型路徑。

## 驗收標準

- 在 1320×860、淺色主題下，專案、完整議題輸入框、目前流程摘要與主要「開始」按鈕全部位於第一個畫面內。
- 有待核准議題時，首頁頂部 360px 內必須看到標題、狀態與「檢視並核准」；從首頁最多一次點擊進入核准面板。
- SSE 收到 `awaiting_approval` 後，頁面中的待辦區和數量徽章會更新，不必重新整理。
- 核准頁不存在內層垂直捲動；方案閱讀只使用文件捲動。
- 在 1320×860 閱讀長方案時，核准、要求修訂與驗證指令仍可見或可由 sticky 決策欄直接操作。
- 7 則發言的工作坊預設只展開目前階段；提案與互評可逐階段或全部展開，內容不能遺失。
- 完成的工作坊進入頁面後，Verdict、驗證結果與 Workspace/diff 必須先於完整逐字稿。
- 375px 寬度下無水平捲動；主要按鈕觸控高度至少 44px；概況位於逐字稿之前。
- 預覽／編輯可用鍵盤切換，具備正確 `tab`、`tabpanel`、`aria-selected`；狀態更新可被輔助技術宣告。
- 深色主題、降低動態效果、現有五種模式、範本、語音輸入與 Ctrl/Cmd+Enter 均不得退化。
- 實作後執行 `cargo test --workspace`，並對所有改動的 ES module 執行 `node --check`，再以 1320×860 與 375px 做截圖比對。

本輪僅做唯讀檢查，未修改檔案，也未執行測試。