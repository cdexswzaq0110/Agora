# 12 · 桌面、手機與 MCP

## 桌面（`src-tauri/src/main.rs`）

- 啟動時，先用 `std::net::TcpListener` **同步綁定** `127.0.0.1:4717`，再在 Tauri 的 async runtime 裡啟動 axum。這樣視窗打開時伺服器一定已經在聽，不會有競爭。
- 連接埠被占用時（例如已經開了另一個 Agora），就把現有的那個當成伺服器，自己只當用戶端。
- 主視窗載入 `http://127.0.0.1:4717/?shell=desktop`。網頁看到 `shell=desktop` 就不發網頁通知，改由原生通知負責。
- **不對頁面開放任何 Tauri IPC**，頁面的權限和一般瀏覽器分頁相同。
- 關閉主視窗只會縮到系統匣；第一次縮到系統匣時，會發一則通知說明「仍在執行」。
- 系統匣選單：開啟 Agora、今日技術情報、用量小工具（置頂）、在瀏覽器開啟、結束。
- 原生通知：議題完成、失敗、取消，方案等待核准，每日情報整理完成。主視窗在前景時不通知。
- release 版設定 `windows_subsystem = "windows"`，沒有主控台；每個子行程都有 `CREATE_NO_WINDOW`。

建置：

```bash
cargo build -p agora-desktop --release
```

產出 `target/release/agora-desktop.exe`。要做成安裝檔，先 `cargo install tauri-cli --version "^2"`，再在 `src-tauri/` 執行 `cargo tauri build`（NSIS）。

## 無視窗版（`crates/agora-server/src/main.rs`）

```bash
agora-server [--port 4717] [--lan] [--demo] [--data <資料夾>]
```

適合常駐在一台電腦上，從手機連進來使用。啟動時會印出網址和配對碼。

## 手機

| 方式 | 步驟 | HTTPS | 安裝成 App、通知、語音 |
|---|---|---|---|
| 同一個 Wi-Fi | 設定 → 勾選「允許區網連線」→ 手機掃 QR code 或打開 `http://<區網IP>:4718/` | 否 | 否（瀏覽器限制） |
| Tailscale（建議） | 電腦執行 `tailscale serve --bg 4717` → 手機打開 `https://<電腦>.<tailnet>.ts.net/` | 是 | 是 |

兩種方式都要輸入 6 位數配對碼。QR code 會把配對碼放在網址片段 `#pair=`，手機端自動帶入。

## MCP（`crates/agora-server/src/bin/agora-mcp.rs`）

`agora-mcp.exe` 是一個 stdio MCP server，透過 Agora 的本機 HTTP API 工作，所以 Agora 必須開著。

| 工具 | 作用 |
|---|---|
| `agora_discuss` | 讓 Claude 和 GPT 回答、討論、辯論或產出方案（solo、roundtable、debate、workshop）；**一律唯讀**，工作坊只產出方案；等到結論出來，回傳結論、證據等級、待驗證事項和網址 |
| `agora_session` | 讀取一個議題的 Markdown |
| `agora_digest` | 讀取每日情報（`latest` 或指定日期） |
| `agora_usage` | 讀取剩餘額度 |

註冊方式（設定頁會顯示這台電腦上的實際路徑）：

```bash
claude mcp add agora -- "D:\path\to\agora-mcp.exe"
```

```toml
# ~/.codex/config.toml
[mcp_servers.agora]
command = "D:\\path\\to\\agora-mcp.exe"
```

用途舉例：你正在 Claude Code 裡工作，想聽聽 GPT 的意見時，直接說「用 agora_discuss 問 codex 這個設計有什麼風險」。

協定：一行一個 JSON-RPC 2.0 訊息；`initialize` 會回傳用戶端要求的 `protocolVersion`；Agora 沒開時，工具會回傳 `isError: true` 和說明，不會讓整個 server 當掉。
