# 0002 — CLI 設定檔的預設模型不一定在你的方案裡

- **Date**: 2026-11-16
- **Grade**: 已確認

## 發生了什麼

`~/.codex/config.toml` 設定的是 `model = "gpt-6.1-sol"`，`models_cache.json` 也把它列為 `visibility: "list"`。但用 ChatGPT Plus 帳號執行 `codex exec` 時：

```text
The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account.  (code: model_not_found)
```

在 Agora 裡實測，`gpt-6-sol` 一樣被拒絕，`gpt-5.6-sol` 可以用。也就是說，**「模型清單裡有」不代表「你的方案能用」**。而 CLI 用的是設定檔裡的預設模型時，請求本身根本沒有模型名稱，所以引擎一開始無法記下是哪個模型被拒絕。

## 證據

- 實測：codex-cli 0.147.0，Plus 方案，2026-11-16。分別用預設模型、`gpt-6-sol`、`gpt-5.6-sol` 各跑一次單人議題；前兩次被拒絕，第三次完成（輸入 24,926 tokens，輸出 84 tokens）。
- 測試：`model_not_found_is_flagged_and_readable`、`rejected_model_is_named_even_when_none_was_requested`。

## 規則

- `model_not_found` 一律轉成可以照做的訊息，並把模型記到 `unsupported_models`，模型選單會標示「方案不支援」。
- 被拒絕的模型名稱**從錯誤訊息解析**（`The '<name>' model…`），不能只依賴請求裡帶的參數。
- 被拒絕的如果是 CLI 預設模型，就提示使用者到「設定 → 預設模型」選一個能用的。
