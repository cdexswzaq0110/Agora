# 0009 — 示範模式的預設參與者用了真的 CLI

- **Date**: 2026-11-16
- **Grade**: 已確認

## 發生了什麼

設定頁寫著「示範模式：加入兩個離線模擬模型，不呼叫 CLI、不使用額度」。但示範模式只是**加入**模擬模型；`pickProvider()` 選預設參與者時優先挑「健康的提供者」。Codex 已登入、狀態健康，所以 GPT 的預設位置是真的 `codex`。流程摘要寫的是「GPT」而不是「模擬 GPT」，很容易看漏。

v0.3 驗收時用 Ctrl+Enter 開一個測試議題，結果真的呼叫了 Codex：提案 47,105 tokens、互評 71,456 tokens，第三次呼叫在發現後取消。花的是 ChatGPT 訂閱額度（不是 API key），但違反了「示範模式不花額度」的承諾。

## 證據

- `S.providers`：`claude` 不健康、`codex` 健康、`mock-claude`、`mock-gpt` 健康。
- 修正前的測試議題 `spec.participants`：`["mock-claude", "codex"]`，兩則 `gpt-5.6-sol` 發言帶有 usage。
- 修正後清空 localStorage 重新載入：五種模式的預設參與者全是 `mock-claude`／`mock-gpt`，流程摘要顯示「模擬 Claude、模擬 GPT」。

## 規則

- 示範模式下 `pickProvider()` 優先選該家族的模擬模型；真的 CLI 仍可以在「調整」裡手動選。
- 在介面上開議題做測試之前，先確認流程摘要裡每個參與者都是「模擬」。
- 一個功能的說明文字（這裡是設定頁的標籤）也是規格：測試要驗證它，而不只是驗證程式碼的意圖。
