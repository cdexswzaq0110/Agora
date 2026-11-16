# CLAUDE.md — Agora

這是在這個 repo 工作的入口，沿用 [Serendipity-Epiphany](https://github.com/cdexswzaq0110/Serendipity-Epiphany) 的 bootstrap 慣例。這裡只寫跟環境有關的事。

## 要找什麼、去哪裡

| 需要 | 去哪裡 |
|---|---|
| 專案是什麼、怎麼跑 | [README.md](README.md) |
| 為什麼這樣設計 | [ARCHITECTURE.md](ARCHITECTURE.md)、[docs/adr/](docs/adr/) |
| 每個子系統怎麼運作、API、資料模型 | [docs/tech/](docs/tech/README.md) |
| 做的過程與證據 | [docs/devlog/](docs/devlog/README.md) |
| 設計討論（Claude 與 GPT 的提案、互評、最終方案） | [docs/design/](docs/design/) |
| 共用詞彙 | [CONTEXT.md](CONTEXT.md) |
| 工作規則 | [.claude/rules/core-rules.md](.claude/rules/core-rules.md) |
| 踩過的坑（附證據） | [docs/lessons/INDEX.md](docs/lessons/INDEX.md) |
| 接下來要做什麼 | [ROADMAP.md](ROADMAP.md) |

## 指令

```bash
cargo test --workspace                          # 全部測試，離線，不呼叫 CLI、不花額度
cargo run -p agora-server -- --demo --port 4799 --data <暫存資料夾>   # 網頁版＋模擬模型（預設選模擬；真的 CLI 也在清單裡），不碰 %APPDATA%\Agora
cargo build -p agora-desktop -p agora-server --release                 # 桌面 exe、無視窗版、agora-mcp.exe
cargo test -p agora-core -- --ignored reddit_probe --nocapture         # 需要網路的 Reddit 探測（手動執行）
python -I scripts/make_icons.py                 # 重新產生圖示（只用標準函式庫）
node scripts/check-web.mjs                      # 前端：module 語法、具名 import、sw.js 快取清單（不要用 node --check x.js，見 lessons/0008）
```

## 這個 repo 特有的限制

- **絕不加入任何 API key 路徑**（ADR 0001）。新的提供者必須做到三件事，而且都要有測試：
  1. 清除 `proc::PAID_API_ENV` 裡的環境變數；
  2. 能從 CLI 本身確認這次用的是訂閱；
  3. 無法確認時不執行（fail closed）。
- **新的子行程一律用 `provider::proc::command()`**，或至少呼叫 `no_window()` 並清除環境變數。
- **CLI 要帶 `clean_config`**：`TurnRequest.clean_config` 對應設定 `isolate_cli_config`，不能在新的程式路徑裡漏掉（lessons/0007）。
- **成本一定有上限**：新的模式要算進 `Engine::turn_budget`；不能新增沒有上限的迴圈。核准關卡要放在實作之前（ADR 0003）。
- **寫入只發生在工作區**：用 `Runner::ensure_workspace()`；Agora 不替使用者 commit（ADR 0004）。
- **證據等級由程式決定上限**：沒有驗證指令通過就不是「已確認」；只靠模型判斷的結論，最高「推論」。
- **不默默丟資料**：壓縮、拒絕、防護、改派、來源失敗，都要寫成 `Note` 或來源狀態。
- **外部格式以實測和 schema 為準**（lessons/0003）：解析器寫成純函式，用錄下來的真實格式做測試。
- **前端**：
  - 每個動態值都要先 `esc()` 才放進 `innerHTML`；
  - 不寫 inline script（CSP）；
  - 重繪排程用 `nextFrame()`，不要只靠 rAF（lessons/0005）；
  - 圖表配色要先用 dataviz 的 `validate_palette.js` 驗證。
- **手動測試用暫存資料夾**（`--data` 或 `AGORA_DATA_DIR`），不要碰使用者的 `%APPDATA%\Agora`。
- **爬蟲**：每個來源獨立失敗，不能拖垮整份情報；不要連續重打同一個站（lessons/0006）。
- **每一輪工作都寫 devlog**（`docs/devlog/`），附上證據；重大決策另寫 ADR，踩到的坑寫 lesson。
- 使用者偏好**繁體中文**：介面文字和使用者會讀的文件用繁體中文；程式碼的識別名稱和註解用英文。
