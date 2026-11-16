<div align="center">

<img src="web/icon.svg" alt="Agora" width="96">

# Agora

**One question. Two minds. Clear answers.**

讓 Claude 與 GPT 單獨作答、協做、討論或辯論的多模型協作平台。
桌面 exe、網頁、手機是同一套介面；**只用你自己的 CLI 訂閱額度，絕不使用付費 API key。**

</div>

---

## v0.4 重點

- 一樣先讓 Claude 與 GPT 提案、互評、紅隊審視再實作（[docs/design/2026-11-16-v0.4](docs/design/2026-11-16-v0.4/05-final-plan.md)）；手機 QR 連線則直接**分派給 GPT 實作**，Claude 審查後合併。
- **推理強度可調**：每位參與者都能選 low…max（依模型實際支援的值；Codex 讀它自己的模型清單、Claude 讀 `--help`），伺服器會擋下不支援的值。
- **情報對照昨天**：新上榜、升溫（同來源同指標 +30% 且 +10 以上）、連續上榜天數、關注主題的變化；不多花任何模型呼叫。導覽列有未讀點，手機讀過電腦也算讀過。
- **方案段落留言**：核准前可以對方案的某一段留言，修訂時逐條送給總結者。結論會建議一句「存入專案記憶」，你可以改了再存。
- 手機底部導覽、鍵盤快捷鍵（`?` 看全部，可關閉）、情報／用量／設定三頁重整（目錄、未儲存提醒）。
- 修正：隔離 CLI 個人設定後，Codex 實作者其實寫不進檔案（[lessons/0011](docs/lessons/0011-isolating-cli-config-dropped-codex-write-sandbox.md)）。

## v0.3 重點（介面）

- 先讓 Claude 與 GPT 各自提案、互評、整合，GPT 再紅隊審視，然後才實作：過程全文在 [docs/design/2026-11-16-ui-v0.3](docs/design/2026-11-16-ui-v0.3/05-final-plan.md)。
- **首頁工作台**：第一個畫面就能寫議題；頂部的待辦列與導覽徽章告訴你哪個方案在等你核准；一行流程摘要說清楚會發生什麼。
- **議題頁先看成果**：核准面板是「方案文件＋固定的決策欄」；討論依輪收合，每則發言留一行摘要；側欄有階段目錄。
- 鍵盤操作、ARIA 語意與 `aria-live` 宣告、手機 44px 觸控、深色主題都在瀏覽器量測過（還沒用實際的螢幕閱讀器測）；示範模式現在預設只用模擬模型。

## v0.2 重點

- **方案工作坊**（預設模式）：獨立提案 → 互評 → 整合成有驗收標準的方案 →（紅隊）→ **你核准** → 在 git worktree 實作 → 驗證 → 依驗收標準審查 → 你看 diff 再決定套用。
- **每日技術情報**：每天整理 Hacker News、GitHub Trending、Lobsters、Reddit、dev.to、Hugging Face Papers 的熱門項目，產出附來源編號的繁體中文摘要。
- 另有議題範本、ADR 與 lesson 匯出、額度不足時自動改派、分岔、盲審、檢索式記憶、模式比較、排程議題、QR 配對、語音輸入、用量圖表、MCP server。
- 文件：[技術文件](docs/tech/README.md)、[開發日誌](docs/devlog/README.md)、[講解指南](docs/tech/presentation-guide.md)。

## 這個專案在解什麼問題

做專案時，常常想讓兩個模型各看一次、互相挑錯，或者正反方吵一架再下結論。但手動在兩個 CLI 之間複製貼上很累，也看不出還剩多少額度。

Agora 把 **Claude Code CLI** 與 **Codex CLI** 當成兩位成員，用四種固定的協作方式排程它們。每個議題都有成本上限，每個結論都附上[證據等級](#證據等級)。

```mermaid
flowchart LR
    U["你的議題"] --> E["Agora 引擎"]
    E -->|claude -p| C["Claude Code CLI<br/>（Claude 訂閱）"]
    E -->|codex exec| G["Codex CLI<br/>（ChatGPT 訂閱）"]
    C --> V["結論 + 證據等級"]
    G --> V
    E -.->|免費讀取| Q["剩餘額度"]
```

## 四種協作方式

| 模式 | 流程 | 適合 |
|---|---|---|
| **單人** | 一個模型直接回答或動手做 | 快速問答、小修改 |
| **協做** | 執行者 →（驗證指令）→ 審查者 → 不通過就修訂，修訂次數有上限 | 寫程式、修 bug；可以讓 Claude 寫、GPT 審，或反過來 |
| **討論** | 第一輪各自**獨立**作答（避免互相帶風向）→ 互相回應 → 總結共識與分歧 | 技術選型、架構方向 |
| **辯論** | 正反方攻防 → 雙方模型**交叉裁判**；裁判意見不一致就直接標示分歧 | 需要逼出風險和反例的決策 |

執行中可以隨時**插話**（下一則發言起生效），也可以**停止**。

## 絕不使用付費 API key

這是硬性規則，不是設定選項（[ADR 0001](docs/adr/0001-cli-only-never-paid-api.md)）。下面三層防護都有測試：

1. 啟動 CLI 前，清除 `ANTHROPIC_API_KEY`、`OPENAI_API_KEY`、`CODEX_API_KEY`、Bedrock／Vertex 等所有會改走付費 API 的環境變數。
2. **Claude**：`claude -p` 的第一行回報 `apiKeySource`。只要不是 `none`，就在模型回答之前終止程序。
3. **Codex**：每一輪開始前，先透過 `codex app-server` 確認登入方式是 `chatgpt`。是 `apiKey`，或者無法確認，就不執行（fail closed）。

## 用量儀表

| | 來源 | 花費 |
|---|---|---|
| **GPT（Codex）** | `codex app-server` 的 `account/rateLimits/read`：5 小時窗、每週窗的已用比例與重置時間 | 免費，每 5 分鐘自動更新 |
| **Claude** | 每次 `claude -p` 執行時附帶的 `rate_limit_event`（5 小時窗、每週窗） | 免費，跑過一次就有數字；也可以手動「主動更新」（用極少額度） |

頂部列、用量頁，以及桌面版可置頂的**用量小工具**，都用 5 格 HP 條顯示剩餘額度。任一時段用到門檻（預設 97%）時，**額度保護**會拒絕再呼叫該模型。

## 記憶與驗證

- **記憶壓縮**：每則發言都會帶入前面的討論。超過門檻（預設 24,000 字）時，最近的發言保留原文，較早的發言改成擷取式摘要，並加上看得見的標記。全文一律保留在紀錄裡。
- **專案記憶**：議題結束後按「存入專案記憶」，結論會變成一行決策紀錄，之後每個議題都會帶入（超過 3,000 字自動摘要）。
- **驗證流程**：單人和協做模式可以設定驗證指令，例如 `cargo test`。驗證失敗時，結果會回饋給審查者和執行者。

### 證據等級

沿用 [Serendipity-Epiphany](https://github.com/cdexswzaq0110/Serendipity-Epiphany) 的五級制，由程式強制套用，不是交給模型自己評：

| 結論 | 等級 |
|---|---|
| 協做：審查通過**且**驗證指令通過 | 已確認 |
| 協做：審查通過，但沒有設定驗證指令 | 推論 |
| 協做：到達修訂上限仍有問題 | 候選 |
| 驗證指令失敗 | 未驗證 |
| 討論／辯論的結論 | 最高「推論」（沒有直接觀察，就不能是「已確認」） |
| 辯論裁判意見分歧 | 候選 |
| 單人、沒有驗證 | 未驗證 |

## 快速開始

**需求**：Windows 10/11（內建 WebView2）、Rust 1.80+，以及已經用**訂閱帳號**登入的兩個 CLI：

```bash
claude auth login
```

```bash
codex login
```

**網頁版**（不開視窗，適合常駐）：

```bash
cargo run -p agora-server --release
```

打開 <http://127.0.0.1:4717/>。想先不花任何額度看看介面，可以加 `-- --demo`，會多出兩個離線模擬模型。

**桌面版**：不想自己編譯的話，到 [Releases](https://github.com/cdexswzaq0110/Agora/releases) 下載 `Agora_<版本>_x64-setup.exe` 安裝（沒有程式碼簽章，Windows 會跳出 SmartScreen：「其他資訊」→「仍要執行」）。自己編譯：

```bash
cargo build -p agora-desktop --release
```

產出 `target/release/agora-desktop.exe`。要做成安裝檔（NSIS），先 `cargo install tauri-cli --version "^2"`，再在 `src-tauri/` 執行 `cargo tauri build`。

**手機**：桌面版的「設定 → 手機連線」會顯示 6 位數配對碼。

- 同一個 Wi-Fi：勾選「允許區網連線」，手機打開畫面上的區網網址。
- 出門在外：在電腦執行 `tailscale serve --bg 4717`，手機打開 `https://<電腦名稱>.<tailnet>.ts.net/`。走 HTTPS 才能安裝成 App，也才能收到通知。

## 結構

```text
crates/agora-core/     引擎：CLI 提供者、四種模式、用量、記憶壓縮、驗證、儲存（無 UI 相依）
  src/provider/          claude.rs · codex.rs · mock.rs · proc.rs（環境清理）· resolve.rs · stream.rs
  src/engine.rs          議題排程、成本上限、額度保護、插話、取消
  tests/engine_modes.rs  四種模式的端到端測試（模擬提供者，不花額度）
crates/agora-server/   HTTP + SSE 伺服器與信任模型；內嵌 web/；也是無視窗版的執行檔
src-tauri/             Tauri 桌面殼：主視窗、系統匣、置頂用量小工具、原生通知
web/                   原生 HTML/CSS/JS（無建置步驟），github.io 風格，PWA
docs/adr/              架構決策
docs/lessons/          實際踩過的坑（附失敗證據）
ROADMAP.md             後續發想與優先順序
```

## 開發

```bash
cargo test --workspace
```

設計理由與模組對照見 [ARCHITECTURE.md](ARCHITECTURE.md)；共用詞彙見 [CONTEXT.md](CONTEXT.md)；接下來要做什麼見 [ROADMAP.md](ROADMAP.md)。

## 與其他作品的關係

- **[Serendipity-Epiphany](https://github.com/cdexswzaq0110/Serendipity-Epiphany)**：證據等級、lessons／ADR 的寫法、核心規則。
- **[Agent Kernel](https://github.com/cdexswzaq0110/Agent-System-Design)**：「審查機制必須限制自身成本」（其 ADR 0002）、上下文壓縮與「不默默丟資料」的原則。
- **Codex Usage Companion**：5 格 HP 條的用量呈現方式，以及從 Codex app-server 讀額度的作法。

## License

MIT — 見 [LICENSE](LICENSE)。
