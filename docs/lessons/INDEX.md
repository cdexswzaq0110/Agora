# Lessons Index

格式沿用 [Serendipity-Epiphany](https://github.com/cdexswzaq0110/Serendipity-Epiphany) 的 `docs/lessons/`。每一則都要附**失敗證據**，例如重現步驟，或修正前會失敗、修正後會通過的測試。「聽起來很對」的經驗法則不算。

| # | 標題 | 失敗證據等級 | 改變了哪條規則 |
|---|---|---|---|
| 0001 | [Claude CLI 的錯誤會包成 `subtype: "success"` 的 result](0001-claude-is-error-hides-in-success-subtype.md) | 已確認 | `ClaudeStreamParser` 一律檢查 `is_error` |
| 0002 | [CLI 設定檔的預設模型不一定在你的方案裡](0002-codex-configured-model-not-on-plan.md) | 已確認 | 被拒絕的模型要記下並標示，名稱從錯誤訊息解析 |
| 0003 | [JSON-RPC 的 `params` 是否必填，每個方法各自不同](0003-app-server-params-are-per-method.md) | 已確認 | `app_server_call` 改成逐方法帶 `params` |
| 0004 | [作者寫的 `display` 會蓋掉 `hidden` 屬性](0004-hidden-attribute-vs-author-display.md) | 已確認 | 全域 `[hidden] { display: none !important }` |
| 0005 | [頁面在背景時，`requestAnimationFrame` 不會觸發](0005-raf-does-not-fire-in-hidden-pages.md) | 候選 | 重繪一律用 `nextFrame()`（rAF 加計時器備援） |
| 0006 | [Reddit 的匿名存取只能盡力而為](0006-reddit-anonymous-access-is-best-effort.md) | 現象已確認／根因候選 | 每個來源獨立失敗；Reddit 用 curl 重試 |
| 0007 | [CLI 會把你的個人設定帶進 agent 執行](0007-cli-personal-config-leaks-into-agent-runs.md) | 已確認 | `isolate_cli_config` 預設開啟 |
| 0008 | [`node --check x.js` 沒抓到 ES module 裡的語法錯誤](0008-node-check-misses-esm-syntax-errors.md) | 已確認 | 前端檢查改用 `scripts/check-web.mjs`，並在瀏覽器確認 module 載入 |
| 0009 | [示範模式的預設參與者用了真的 CLI](0009-demo-mode-defaulted-to-real-cli.md) | 已確認 | 示範模式預設選模擬模型；測試前先看流程摘要 |
| 0010 | [長期存在的綁定，會比被重繪的節點活得更久](0010-long-lived-bindings-outlive-rerendered-nodes.md) | 已確認 | 跨頁節點的委派只綁一次；觀察器跟著節點重新觀察 |
