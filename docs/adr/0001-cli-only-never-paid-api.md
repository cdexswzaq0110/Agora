# ADR-0001: 只透過 CLI 使用訂閱額度，絕不使用付費 API key

- **Status**: accepted
- **Date**: 2026-11-16
- **Evidence grade of the problem statement**: Confirmed（使用者明確要求：「只使用 CLI，絕對不可使用額外付費的 api key」）

## Context

使用者已經有 Claude 與 ChatGPT 的訂閱，並且已經在本機安裝 `claude`（2.1.252）與 `codex`（0.147.0）兩個 CLI。API key 是按 token 計費的另一個帳單來源。而且兩個 CLI 都會**自動**改用環境變數裡的 API key：例如 `ANTHROPIC_API_KEY` 存在時，Claude Code 會優先用它。換句話說，就算程式碼裡完全沒有「API 路徑」，只要使用者的環境裡剛好有一把 key，帳單還是會發生。

## Options considered

1. **提供 API 與 CLI 兩種 provider，預設 CLI**：彈性最大，但任何設定錯誤都會直接變成帳單。
2. **只用 CLI，靠文件提醒不要設 API key**：不夠，因為環境變數是外部因素。
3. **只用 CLI，並用程式碼主動阻擋所有 API key 路徑（選這個）**。

## Decision

選 3。三層防護，每層都有測試：

1. **清環境變數**：`provider/proc.rs` 的 `PAID_API_ENV` 列出的變數，在每個子行程啟動前都會被移除。測試：`command_scrubs_every_paid_api_variable`。
2. **Claude**：`system/init` 事件回報的 `apiKeySource` 必須是 `"none"`（訂閱登入時的實測值），否則在模型回答之前終止。測試：`api_key_billing_source_aborts_before_any_answer`。
3. **Codex**：每輪之前用 `codex app-server` 的 `account/read` 確認 `account.type == "chatgpt"`。是 `apiKey`、`amazonBedrock`，或無法確認時，**不執行**（fail closed）。測試：`account_parsing_distinguishes_api_key_from_chatgpt`，以及 `run()` 裡的防護分支。

健康檢查也會把 `apiProvider != firstParty`（Bedrock／Vertex）和 API key 登入方式標成「未就緒」。

## Consequences

- 使用者的額度就是唯一的成本，可以在用量儀表上看到。
- 無法使用只有 API 才有的功能，例如精確計費數字、批次處理。這是可以接受的代價。
- Claude 的剩餘額度沒有免費的主動查詢方式，只能從執行中被動擷取（見 ROADMAP 已知限制）。
- 未來要加入其他模型家族時，必須先找到對應的「訂閱登入 + 計費來源可驗證」機制，才能加。
