# ADR-0002: Rust 引擎 + 內嵌 HTTP 伺服器 + Tauri 外殼

- **Status**: accepted
- **Date**: 2026-11-16
- **Evidence grade of the problem statement**: Confirmed（使用者選了 Tauri；需求是網頁、桌面 exe、手機三種入口）

## Context

需求是三種入口，而且背後是同一個 agent：網頁、桌面 exe、手機。使用者選了 Tauri（Rust）。Agent Kernel 是 Python 寫的，但 Agora 開在新 repo，不需要重用它的程式碼，只沿用它的設計原則。

## Options considered

1. **Python 後端當 Tauri sidecar**：可以重用 Agent Kernel，但得把 Python 打包成第二個 exe，並且同時維護兩種語言的建置流程。
2. **Tauri IPC（`invoke`）當 API**：桌面很順，但手機和瀏覽器用不了 IPC，得再寫一套 HTTP API。
3. **Rust 引擎 + 內嵌 axum 伺服器，Tauri 視窗只載入 `http://127.0.0.1:<port>`（選這個）**。

## Decision

選 3。決定性的事實是：**手機一定得走 HTTP**。既然非要有 HTTP API 不可，桌面也用同一套，就只需要維護一份前端（`web/`，原生 JS，沒有建置步驟），也只有一個信任模型要測。Tauri 不對頁面開放任何 IPC，所以頁面的權限和瀏覽器分頁一樣。

`agora-server` 也能單獨編譯成無視窗的執行檔，給「常駐在一台電腦、從手機連」的情境用。

## Consequences

- 一個行程就包含引擎、伺服器和視窗，打包結果是單一 exe（另需 WebView2，Windows 10/11 內建）。
- 頁面是 `http://` 的 loopback 來源，所以桌面版的通知改由 Rust 端用 `tauri-plugin-notification` 發送；網頁版只在 HTTPS／localhost 時使用 Web Notification。
- 伺服器的信任模型（見 ARCHITECTURE.md）成為安全邊界，因此有獨立的測試（`crates/agora-server/tests/api.rs`）。
- 關閉主視窗只是縮到系統匣，讓手機連線和進行中的議題繼續執行。
