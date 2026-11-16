# 0003 — JSON-RPC 的 `params` 是否必填，每個方法各自不同

- **Date**: 2026-11-16
- **Grade**: 已確認

## 發生了什麼

開發前，我先探測了 `codex app-server` 的 `account/rateLimits/read`：沒帶 `params` 也成功。於是 `app_server_call` 寫成「所有方法都不帶 `params`」。

實際跑起來，健康檢查回報：

```text
無法讀取 Codex 帳號狀態：Invalid request: missing field `params`
```

查 schema（`codex app-server generate-json-schema`）才看到：`account/rateLimits/read` 的 `params` 型別是 `null`，而 `account/read` 的 `params` 是**必填物件**（`GetAccountParams`，可以是 `{}`）。

一個方法的探測結果，被我當成了整個協定的證據。這和 Agent Kernel lessons/0005 是同一類錯誤：檢查了 helper，卻沒有檢查要保證的行為本身。

## 證據

- 修正前：在瀏覽器裡看到健康檢查顯示上面的錯誤（2026-11-16）。
- 修正後：`/api/providers/refresh` 回報 `已登入（ChatGPT 訂閱）`、`ChatGPT plus`，額度 5 小時窗 0%、每週窗 32%。

## 規則

- `app_server_call` 的介面改成 `&[(method, params)]`，每個方法明確帶自己 schema 要求的 `params`；`Value::Null` 表示不帶這個欄位。
- 對外部協定的假設，要以 **schema** 為準，不能以「某一次呼叫成功」為準。
