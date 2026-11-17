# 05 · 每日技術情報

原始碼：`crates/agora-core/src/digest/`（`sources.rs` 抓取與解析、`rank.rs` 排序與去重、`mod.rs` 提示詞與備援）、`engine/ops.rs`（`run_digest`、`compose_digest`）、`scheduler.rs`、`web/js/digest.js`。

## 原則

- **只讀列表頁**：標題、分數、留言數、簡介、標籤，不抓文章全文。
- **一次模型呼叫**：把排好序的項目交給一個 CLI 模型整理成繁體中文摘要，成本可預期（實測：輸入約 2.7 萬 tokens，其中 1.6 萬是快取；輸出約 3 千）。
- **每個主張都要有來源**：要求模型用 `[n]` 標出依據，從標題推測的內容要標【推論】；介面把 `[n]` 變成連到該項目卡片的連結。
- **永遠有東西可讀**：沒有可用的模型、額度不足或模型失敗時，改用固定格式的清單。
- **禮貌與隱私**：使用通用的 User-Agent、短逾時，每個來源只發一次請求（Reddit 每個看板一次，間隔 3 秒），請求裡沒有任何個人資料。

## 來源

| id | 來源 | 端點 | 解析 |
|---|---|---|---|
| `hn` | Hacker News | Algolia `search?tags=story&numericFilters=created_at_i>24h前,points>30` | JSON `hits`，依分數排序 |
| `github` | GitHub Trending | `github.com/trending?since=daily`（HTML） | `<article class="Box-row">`：repo、簡介、語言、總星數、今日新增星數 |
| ↳ 備援 | GitHub Search API | `search/repositories?q=created:>7天前&sort=stars` | JSON `items` |
| `lobsters` | Lobsters | `hottest.json` | JSON 陣列 |
| `reddit` | Reddit | `/r/<看板>/top/.rss?t=day&limit=8`（Atom） | 解析 entry；外部連結取自 `[link]`；被 403 或 429 擋時改用 curl 重試 |
| `devto` | dev.to | `api/articles?top=1` | JSON 陣列 |
| `hf` | Hugging Face Papers | `api/daily_papers` | JSON 陣列，依讚數排序 |

每個解析器都是純函式，用實測的回應格式做單元測試（`digest::sources::tests`）。

## 排序與去重（`rank.rs`）

```
分數 = 在該來源內的名次分（第 1 名 1.0，往後遞減）× 來源權重
     + 0.35 × 命中你關注的主題數（最多 2 個）
     + 0.30 × 同一連結還出現在幾個其他來源（跨社群都在討論）

來源權重：HN、GitHub 1.0；Lobsters、HF 0.85；Reddit 0.75；dev.to 0.55
```

- 去重：網址正規化（去掉 scheme、`www`、`utm_*`、`#`、結尾的 `/`），或長標題完全相同；重複的項目合併成一筆，在 `also_on` 記下其他來源。
- 挑選：每個有資料的來源至少保留 3 則、最多 10 則，其餘依分數補滿 `max_items`（預設 36），最後依分數重新編號 `[1]…[n]`。

## 摘要提示詞（`llm_prompt`）

段落依序是：今日三大重點、技術趨勢觀察、GitHub 熱門專案、論壇熱議、AI 與論文、建議深入閱讀（依你的關注主題）、名詞小辭典。規則是「只根據提供的資料」「不要編造數字、版本號或引言」。

## 和昨天相比（v0.4，`trend.rs`）

不多花任何模型呼叫，只用已存的情報計算：

- 只和**日期更早**的情報比較（同一天重新整理不會和自己比），最多往回 7 天。
- 先比對正規化後的網址，沒有網址才比「同來源＋正規化標題」。
- **新上榜**：前一份情報沒有這一項，而且前一份**那個來源有抓成功**；來源失敗或沒有來源紀錄時就是「未知」，不顯示徽章。用「新上榜」而不是「新出現」，因為前一份只存了排名後的項目。
- **升溫**：同來源、同一個指標（`stars_today`、`score`、`comments` 依序取第一個雙方都有的），增加 30% 以上而且至少 +10。
- **連續 N 天**：依曆日往回數，每一天都要有已存的情報並且包含這一項；缺一天就中斷。
- 關注主題（`keywords`）的出現次數：今天 vs 前一份。
- 提示詞裡的項目會帶【新上榜】【升溫 +N】【連續 N 天】標記，並要求一節「## 和昨天相比」；沒有模型時，備援摘要也會由程式產生同一節。
- v0.4 以前存的檔案沒有這些欄位，讀取時補算，不回寫。

## 已讀（v0.4）

- 伺服器存一份 `digest-read.json`：日期 → 讀過的那一版的 `generated_at`。所有裝置共用，在手機讀過，電腦上也算讀過。
- 「未讀」＝最新一份沒有紀錄，或紀錄的版本比較舊（同一天重新整理過）。
- 前端只在**成功畫出**那一版之後才呼叫 `POST /api/digests/{date}/read`；伺服器會檢查 `generated_at` 和存檔相符，游標只增不減，並和寫入情報共用同一把鎖。

## 排程（`scheduler.rs`）

- Agora 開著時每分鐘檢查一次：今天還沒有情報、時間已經到了（預設 08:00），而且一小時內沒有試過，就開始整理。
- 電腦整天關機時，下次開啟 Agora 會補做；同一天只做一份。
- 手動：情報頁的「立即更新今天的情報」→ `POST /api/digests/run`。
- 完成時送出 `digest` 事件 → 桌面版發通知，網頁重新載入。

## 延伸討論

每張項目卡片都有「延伸討論 ↗」按鈕：帶入「技術情報延伸討論」範本（辯論模式），專案裡預先填好標題、網址、討論串和簡介，你只要補上「我們的情境」。

## 設定（`settings.digest`）

`enabled`、`time`、`sources`、`subreddits`、`keywords`、`provider`（空白＝第一個就緒的 CLI）、`model`、`max_items`、`use_llm`。

## 證據

- 單元測試：每個解析器、去重、權重、來源保底、提示詞內容。【已確認】
- 端到端（不連網）：`digest_is_summarized_by_a_ready_provider_or_falls_back`。【已確認】
- 實機（2026-11-16）：HN 29、GitHub 12、Lobsters 25、dev.to 20、HF 25，Reddit 0（403）→ 保留 36 則 → Codex 約 60 秒整理完，摘要裡有 74 個引用連結。【已確認】
