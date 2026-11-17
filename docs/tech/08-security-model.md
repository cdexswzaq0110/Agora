# 08 · 安全模型

原始碼：`crates/agora-server/src/auth.rs`、`lib.rs::guard`、`v2.rs`；CLI 隔離見 [03](03-providers-and-billing-guard.md)。

## 威脅與對策

| 威脅 | 對策 | 測試 |
|---|---|---|
| 被花到 API key 的錢 | 三層計費防護（ADR 0001） | `command_scrubs_every_paid_api_variable` 等 |
| 同網路的其他人操作你的 Agora | 預設只監聽 `127.0.0.1`；區網連線要手動開，而且要配對 | `local_is_trusted_and_phone_must_pair` |
| 惡意網站對 `127.0.0.1:4717` 發請求（CSRF） | 會改變狀態的請求必須是 `application/json`；`Origin` 要和 `Host` 完全一致；`Sec-Fetch-Site: cross-site` 一律拒絕 | `rebinding_and_csrf_are_refused` |
| DNS rebinding（惡意網域解析到 127.0.0.1） | 「本機」的條件是 loopback、`Host` 是 127.0.0.1 或 localhost，而且沒有代理標頭；其他一律需要配對 | 同上 |
| 透過 `tailscale serve` 進來的請求 | 它們有代理標頭，Host 也不是 localhost，所以視為遠端，需要配對 | `local_requires_loopback_host_and_no_proxy` |
| 猜配對碼 | 6 位數、10 分鐘過期、用過一次就換；10 分鐘內錯 8 次就鎖定；比對時間固定 | `pairing_code_is_single_use_and_rate_limited` |
| 手機被偷 | 配對過的裝置不能改 CLI 路徑、配對、網路；不能把 ADR 或 lesson 寫進專案；可以隨時撤銷 | `v2_endpoints_…_local_only_exports` |
| 模型輸出裡夾帶惡意 HTML | Markdown 先跳脫再轉換；HTML 回應帶 CSP `script-src 'self'`；連結只允許 http(s) | 程式審閱 |
| 模型改壞你的專案 | 寫入在 git worktree 進行，套用要你按按鈕（ADR 0004） | `worktree_roundtrip…` |
| agent 用到你的個人工具（例如電腦操控 MCP） | `isolate_cli_config` 預設開啟 | lessons/0007 |
| 專案 id 或日期被拿來做路徑穿越 | id 只允許英數和 `-`；日期必須是 `YYYY-MM-DD` | `rejects_path_traversal_ids`、`digest_dates_are_validated` |

## 呼叫者分級

```
本機（Caller.local）       ：全部 API
已配對裝置（cookie）        ：一般 API；不包括 /api/pairing*、/api/network、PATCH /api/settings、寫專案檔的匯出
未配對                       ：靜態檔、/api/whoami、/api/pair
```

## Cookie

`agora_token=<32 bytes hex>; Path=/; Max-Age=1 年; HttpOnly; SameSite=Strict`；經由 HTTPS（例如 `tailscale serve`）進來時再加上 `Secure`。token 存在 `settings.json`（本機檔案），比對時間固定。

## QR code

QR 的網址把配對碼放在 `#pair=123456`。網址片段不會被瀏覽器送到伺服器，只在手機端預先填入配對碼。產生 QR 的端點只開放本機呼叫。
