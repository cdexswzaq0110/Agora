# v0.4 · GPT 的獨立提案

> Codex CLI `gpt-5.6-sol`（ChatGPT 訂閱，非 API key）· 輸入 333,081 tokens（快取 277,632）· 輸出 4,242 · 寫在看到 Claude 提案之前。輸入：實測事實＋4 張截圖＋唯讀存取 repo。

## 我理解的需求

本輪目標是為 Agora v0.4 提出可實作、可驗證的獨立方案，涵蓋 R3–R6 與 U1–U4。以下依 repo 現況與實測資料評估；本輪完全唯讀，未修改檔案、未執行測試。

## 最重要的問題

1. **資訊密度已影響核心任務。** 情報頁把篩選、摘要、完整清單串成 6,575px；設定頁則把 10 種不同責任與兩種儲存語意放在同一頁。這不是純視覺問題，而是使用者難以找到「今天有何變化」與「哪些設定尚未儲存」。

2. **資料模型不足以支援趨勢與精細互動。** `Digest` 只有當日 `items`、`markdown`、來源狀態，沒有跨日比較欄位（`crates/agora-core/src/digest/mod.rs:65`）；`GateDecision::Revise` 只有一段 `feedback`（`engine/mod.rs:62`）。

3. **推理強度若只做成文字輸入，容易產生無效 CLI 呼叫。** `TurnRequest` 目前只有 `model`，兩個 provider 的 `build_args()` 也沒有 effort（`provider/mod.rs:27`、`provider/codex.rs:174`、`provider/claude.rs:40`）。不同模型支援集合不同，必須由後端驗證，不能只依賴 UI。

4. **路由焦點管理正確，但焦點樣式套錯層級。** `focusHeading()` 每次換頁聚焦 h1（`web/js/main.js:37`）；情報、用量的大型框線標題因此顯示整圈焦點框。應保留焦點移動，只調整程式性聚焦的視覺規則，不能移除無障礙行為。

5. **「記憶」目前不可審核。** `Engine::remember` 直接組合日期、標題、模式與結論（`engine/mod.rs:373`）；使用者按下按鈕前看不到將寫入什麼，也不能修正過長或失真的內容。

## 優先順序與具體方案

### P0：R4 推理強度

資料結構新增：

```rust
Participant {
    provider: String,
    model: Option<String>,
    effort: Option<ReasoningEffort>,
}
```

`ReasoningEffort` 使用封閉 enum：`low/medium/high/xhigh/max/ultra`，加 `#[serde(default)]` 保持舊 session 相容。`TurnRequest` 同步加入 effort。

Codex 產生 `-c model_reasoning_effort=<值>`；Claude 產生 `--effort <值>`。provider/model capability 應由 `/api/providers` 回傳 `supported_efforts`、`default_effort`；後端收到不支援值時回傳 400，不可靜默降級。模擬 provider 接受並記錄 effort，但不呼叫 CLI。

介面在首頁每位參與者的模型選擇旁提供 `<select>`，預設顯示「模型預設（low）」而非自行固定值。手機一列一欄、label 完整，切換模型後保留仍合法的值，否則回到模型預設。Claude 一併支援，但先標示「需 Claude CLI 2.1.252+」；舊 CLI 執行失敗時給可操作錯誤，不自動重試消耗額度。

### P0：U4 重整三頁

- **情報：** 首屏只保留日期、更新按鈕、來源健康、趨勢摘要與 5 個重點；完整「今日項目」放入明確的「全部 N 項」區域。桌面首個項目應在 860px 高度內出現。
- **用量：** 真實 CLI 放前面；模擬模型合併成一個「離線示範」區塊，預設收合，不再與真實額度同權重。
- **設定：** 頂部加入 sticky 區段目錄；拆成「連線、情報與排程、模型、保護、進階」。可批次儲存的欄位使用 sticky「有未儲存變更／儲存」列；立即生效操作各自標示並不混入該表單。

同時修正 h1：保留 `tabindex=-1` 與 focus，僅對程式性頁標題使用不改變版面的焦點提示，例如底線／外框偏移；鍵盤可操作控制項仍保留明顯 focus ring。

### P1：R3 昨日趨勢

不要再花一次模型額度。後端建立純函式：

```text
今日項目 + 最近一份較早 digest
→ URL 正規化比對
→ 無 URL 命中時以來源 + 穩定識別／標題正規化比對
→ new / continuing / rising
```

在 `DigestItem` 加 `trend: Option<Trend>`，包含 `kind`、`previous_date`、`previous_metric`、`delta`；舊 JSON 因 `serde(default)` 仍可讀。`rising` 僅在同來源具有可比較指標時成立，例如 score、comments、stars_today；不可跨來源比較不同單位。昨天缺檔、來源失敗或指標缺失時，只能標成「新出現／持續」，不能宣稱升溫。

`GET /api/digests/{date}` 回傳已儲存結果；若要讓既有檔案也顯示，可在讀取時動態補算，不回寫歷史檔。介面用文字加圖示呈現「新出現」「持續 2 天」「升溫 +34」，不能只靠顏色。

### P1：U1 手機底部導覽、U3 未讀

375px 以下將四個主入口改成固定底部導覽，支援 `safe-area-inset-bottom`、每項至少 44×44px；桌面維持頂欄。當鍵盤或文字輸入聚焦時不可遮住送出控制項，主內容需保留底部 padding。

已讀狀態應存**伺服器端設定資料**，而非 localStorage，否則手機與桌面不同步：

```json
"digest_read": {
  "last_opened_date": "2026-11-16",
  "last_opened_generated_at": 1794800000000
}
```

提供 `GET /api/digests` 的 `unread_count/latest_generated_at`，以及 `POST /api/digests/read`。只有成功載入某日情報後才標已讀；更新同一天但 `generated_at` 較新時重新變未讀。導覽顯示文字可讀的 badge，並以 `aria-label="情報，2 則未讀"` 暴露狀態；示範模式亦使用暫存 data dir。

### P2：R5 方案逐段留言

先縮小為**核准關卡中的段落級修訂意見**，不做多人即時評論系統。方案解析後為 heading、paragraph、list 等 block 產生穩定 `block_id`，例如 `section-slug + type + ordinal`。

```rust
RevisionComment {
    block_id: String,
    quote: String,
    comment: String,
}
GateDecision::Revise {
    feedback: String,              // 保留舊客戶端
    comments: Vec<RevisionComment>
}
```

預覽中每個 block 提供「留言」按鈕；手機點擊後使用該段下方 editor，不用狹窄側欄。送出時把全域意見與逐段意見按方案順序整理給 synthesizer。方案修訂後找不到原 block 時，以 `quote` 作上下文並標示「原段落已變更」，不得把留言錯掛到其他段落。所有動態內容繼續經 `esc()`。

### P2：R6 記憶建議

讓既有結論產生流程同時輸出一個 `memory_suggestion`，限制為單行、例如 160 字；模擬 provider 使用確定性內容。結論完成後，在「存入專案記憶」前顯示可編輯文字框與實際預覽。

新增 `POST /api/sessions/{id}/remember` body：

```json
{ "line": "- [2026-11-16] 決定……" }
```

後端移除換行、限制長度、拒絕空值；未提供 body 時沿用目前組句，保持相容。這是「建議」而非自動寫入，仍須使用者確認。

### P3：U2 全域快捷鍵

本版只做三個低衝突操作：

- `Ctrl/Cmd+K`：聚焦全域導覽／命令列
- `g` 再按 `i/u/s`：前往情報／用量／設定
- `?`：開啟快捷鍵說明

在 input、textarea、select、contenteditable 或 IME composing 時全部停用；不攔截瀏覽器既有儲存、重新整理、分頁快捷鍵。這項可在核心功能完成後實作。

## 主要風險與取捨

- 跨日標題比對會有誤判，因此 URL／來源識別優先，模糊標題只能保守使用。
- effort 會影響耗時與訂閱額度；UI 必須說明「更高不保證更好」，且不自動把所有角色升到 high。
- 段落 ID 在模型重寫後不可能完全穩定；保留 quote 是必要降級機制。
- 伺服器端已讀會增加一次寫入，但換來桌面與手機一致；這比 localStorage 更符合產品模型。
- 大幅壓縮頁面可能隱藏診斷資訊，因此採漸進展開，不刪除來源錯誤與額度細節。

## 可量測驗收標準

1. Rust 測試驗證每個 Codex/Claude effort 產生正確參數；不支援值回 400；`None` 不產生參數。
2. 舊版不含 `effort`、`trend`、`comments` 的 JSON 反序列化成功。
3. 趨勢單元測試涵蓋 URL 正規化、新出現、同項持續、指標上升、缺昨日、昨日來源失敗及不可比較指標。
4. 1320×860 時，情報第一個重點的 `getBoundingClientRect().top < 860`；頁高不超過 3,500px，完整清單仍可展開。
5. 375px 時底部導覽固定可見、每項 bounding box 至少 44×44px，最後一個頁面控制項不被遮擋。
6. 同日重新生成且 `generated_at` 增加後未讀數恢復；桌面標已讀後，手機重新請求得到 0。
7. 設定任一批次欄位改動後顯示 dirty bar；儲存成功後消失；配對碼更新不使表單誤判 dirty。
8. 逐段留言送出的 block 順序、quote、comment 可由 Rust API 測試完整反序列化；舊 `{feedback}` 仍有效。
9. h1 換頁後仍是 `document.activeElement`，且焦點提示不改變其尺寸；鍵盤按鈕 focus ring 仍可見。
10. 全部驗證最終需離線通過 `cargo test --workspace` 與 `node scripts/check-web.mjs`。

## 這一輪不該做

- 不引入前端框架、打包器或第三方評論編輯器。
- 不新增資料庫；沿用 JSON store 與 atomic write。
- 不為趨勢額外呼叫模型或抓文章全文。
- 不自動把情報標成已讀，也不只存在 localStorage。
- 不做即時多人留言、留言討論串或協同游標。
- 不自動寫入專案記憶；必須保留人工確認。
- 不加入 API key、第三家模型或 CLI session resume。
- 不順便重構全部設定 API、Markdown renderer 或導覽架構。

本輪僅完成唯讀評估與 v0.4 方案；未修改程式、未執行測試。建議實作順序為：**R4 → U4 → R3 → U1/U3 → R5 → R6 → U2**。