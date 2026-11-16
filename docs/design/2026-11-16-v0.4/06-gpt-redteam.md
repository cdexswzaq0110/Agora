# v0.4 · GPT 紅隊審視（實作前）

> Codex CLI `gpt-5.6-sol` · 輸入 183,685 tokens（快取 146,944）· 輸出 3,253。輸入：`05-final-plan.md`＋唯讀存取 repo。這次在實作前審視；處理結果見 `05-final-plan.md` 文末的「修訂紀錄」。

## 【品味評分】

🟡

方向合理、範圍也有控制，但目前仍有數個會造成資料誤判、相容性破壞或權限過大的缺口。以下依現有 `agora-core`、`agora-server`、`web/js` 的實際結構審視。

## 【致命問題】

1. **高｜Effort 的「模型預設（low）」語意矛盾**

   「模型預設」不等於 `low`。若 CLI 真正預設是 `medium` 或隨模型改變，UI 顯示 low、實際卻不傳參數，稽核紀錄也無法知道真正強度。

   **修正：**選項顯示「模型預設（由 CLI 決定）」；能力資料將 `default` 設為 `Option<Effort>`。`Turn` 同時記錄 `requested_effort` 與可取得時的 `effective_effort`，否則明確標為未知。

2. **高｜封閉 enum 會被新版 CLI 值擊穿**

   Codex `model/list` 或 Claude 未來可能回傳新值；直接反序列化成封閉 `Effort`，可能讓整份 provider 清單失敗。Claude `--help` 的括號也可能是說明文字、換行或不同版本格式。

   **修正：**外部探測先解析為字串，逐值映射已知 enum，未知值忽略並記錄診斷；加入未知值、缺欄位、格式改變、逾時測試。Claude 不應只靠「有 `--effort`」便判定各值皆可用。

3. **高｜「舊情報讀取時補算」無法可靠還原 `new`**

   舊 Digest 若未保存「每個來源當次抓取成功／失敗」，便無法知道前一份缺項是沒上榜還是抓取失敗。因此不能依方案正確補算 `new` 或 `unknown`。

   **修正：**舊檔僅補算 `continuing/rising` 等可證明資訊；缺來源狀態時不得宣稱 `new`。新檔新增版本與逐來源結果。驗收應加入舊檔來源狀態缺失案例。

4. **高｜配對裝置權限未定義**

   現有 guard 除配對／網路／設定 PATCH 外，多數 API 對任何已配對裝置開放。新增的 remember、read、核准／留言等寫入操作因此可能由所有配對手機執行。這不是單純 UI 問題。

   **修正：**明確列出每個新 API 的 local-only／paired-read／paired-write 權限。至少「寫入專案記憶、核准、修訂」應要求本機或具寫入權限的裝置；補未配對、已撤銷、一般配對裝置的 401/403 測試。

## 【為什麼這是問題】

5. **高｜已讀 API 存在競態與偽造時間**

   Client 傳入任意 `generated_at`，可能把尚未載入或未來版本標為已讀。兩台裝置同時 read，以及重新產生與 read 交錯時，舊請求可能覆蓋新游標。

   **修正：**伺服器以 `{date, generated_at}` 查證該版本確實存在，再用單調更新 `max(existing, requested)`；重新產生與 read 共用同一 store 鎖。定義時間型別，禁止字串字典序比較。

6. **中｜已讀其實不是「跨裝置各自狀態」**

   單一 `digests/read.json = date → generated_at` 是全域狀態；手機讀過後桌面也變已讀。方案稱「跨裝置理由」但未說這是同步已讀，還是每裝置游標。

   **修正：**產品語意二選一並寫進驗收。若每裝置獨立，key 必須包含 device id；本機桌面也需穩定識別。

7. **高｜段落 `block_id` 不穩定且可碰撞**

   標題改字、重新排序或插入同名標題都會改變序號。`heading + quote` 也可能包含模型輸出的惡意 HTML。現有前端廣泛使用 `innerHTML`，任何漏掉 `esc()` 的 data attribute、按鈕或預覽都會成為 stored XSS。

   **修正：**`block_id` 只作該方案版本內定位，留言提交時一併帶 `plan_revision`；伺服器驗證段落確實屬於該版本。所有 heading、quote、text 經文字節點或 `esc()` 輸出，禁止放入未跳脫 `id`。加 `<img onerror>`、引號、重複標題測試。

8. **中｜留言長度與提示詞注入無限制**

   `comments` 沒有限制數量、單則長度、quote 長度；配對裝置可製造巨大 session JSON 或把指令塞進修訂提示詞。

   **修正：**伺服器限制留言數與 UTF-8 字元數，將留言包在清楚的「不可信使用者資料」區塊；拒絕控制字元，保留換行規則要明定。

9. **中｜`remember` 向後相容定義不完整**

   現有 handler 完全不解析 body。Axum 若改為必要 `Json<Body>`，真正的無 body POST 會在進入 handler 前成為 415/422，無法「照舊」。

   **修正：**使用 optional JSON extractor，明確測試空 body、`{}`、合法 line、過長 Unicode、只有換行。自訂 line 仍應沿用既有專案記憶格式，避免資料格式分裂。

10. **中｜設定 dirty state 容易被重繪破壞**

    現有設定頁的立即操作大量呼叫 `renderSettings()`，會整頁重抓並重畫。使用者先改未儲存欄位，再換配對碼或切 LAN，草稿可能消失，即使 dirty bar 不出現也不代表安全。

    **修正：**草稿與 server snapshot 分離；立即操作不得覆蓋草稿。驗收加入「先改欄位未存，再換配對碼，草稿仍在」。

11. **中｜手機三個固定層可能互相覆蓋**

    底部導覽、插話列、更多面板、dirty bar 都宣稱位於導覽上方，但沒有統一 stacking/offset 模型。`visualViewport` 事件在 iOS 可能頻繁變動，僅靠「變矮」會把網址列收合誤判成鍵盤。

    **修正：**集中使用 CSS 變數計算各層高度，定義唯一 z-index 次序；鍵盤判斷需同時要求可輸入元素聚焦及明顯高度差。測試 landscape、200% 縮放、動態字級與 safe area。

12. **中｜h1 驗收會鼓勵移除可見焦點**

    只驗 `outline-style: none` 無法證明滑鼠與鍵盤來源，也可能讓實作者對 h1 永久移除 outline。換頁非同步 render 後的焦點時機亦未定義。

    **修正：**h1 用 `tabindex="-1"`；僅 `:focus-visible` 顯示不造成位移的 box-shadow／偽元素。自動化分別模擬滑鼠與鍵盤導航，驗證可見指示，而非只讀 outline。

13. **低｜快捷鍵鍵盤配置與焦點陷阱未涵蓋**

    `?` 在不同鍵盤配置常需 Shift；dialog 只測 Esc，不足以證明焦點不會跑到背景。

    **修正：**依 `event.key === '?'` 判斷，dialog 使用原生 `<dialog>` 或完整 focus trap，測 Tab 循環、背景 inert、關閉後原控制項已被刪除時的 fallback。

## 【改進方向】

已檢查且整體良好的部分包括：CSP 現況仍禁止 inline script、外部連結已有 `noopener noreferrer`、寫入 API 有 same-origin JSON 防護；趨勢的同來源／同指標門檻、非顏色單一表達、舊 JSON 的 `serde(default)`、不自動寫入記憶，以及 `cargo test`／前端 module 檢查方向都合理。

但驗收還應增加：API 權限矩陣、stored XSS payload、未知 effort 值、跨裝置競態、設定草稿保留、200% 縮放及橫向手機。另應把「歷史最多 7 份」與「中間缺一天」定義清楚：streak 究竟依曆日連續，還是依已存檔案連續；目前兩句可產生相反實作。

## 【建議修改版本】

不必重寫方案；實作前優先補四項規格：effort 的真實預設與未知值策略、Digest 來源狀態版本化、配對裝置權限矩陣、read 游標的伺服器驗證與單調更新。其餘可在原章節直接追加上述邊界測試。

本次只做針對性讀檔與紅隊審查，未修改檔案，也未執行測試。