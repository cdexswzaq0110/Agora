# AGENTS.md

給非 Claude Code 的 agent（Codex、Gemini CLI……）的入口指標。規則與 [CLAUDE.md](CLAUDE.md) 相同，請先讀它。

最重要的三條：

1. 絕不加入使用 API key 的路徑（見 `docs/adr/0001-cli-only-never-paid-api.md`）。
2. 改完之後執行 `cargo test --workspace`，並用 `node scripts/check-web.mjs` 做前端檢查（module 語法、import、離線快取清單）。
3. 手動測試用 `cargo run -p agora-server -- --demo --data <暫存資料夾>`，不要動到使用者真正的資料夾；開議題前確認流程摘要裡的參與者都是「模擬」（lessons/0009）。
