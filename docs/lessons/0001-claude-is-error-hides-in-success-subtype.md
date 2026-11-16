# 0001 — Claude CLI 的錯誤會包成 `subtype: "success"` 的 result

- **Date**: 2026-11-16
- **Grade**: 已確認

## 發生了什麼

開發 Agora 前，先用 `claude -p "Reply with exactly: OK" --output-format stream-json --verbose` 探測輸出格式。這台電腦上 Claude CLI 的 OAuth 登入已過期，最後一行是：

```json
{"type":"result","subtype":"success","is_error":true,
 "result":"Failed to authenticate: OAuth session expired and could not be refreshed", ...}
```

`subtype` 是 `success`，`result` 卻是錯誤訊息。如果解析器只看 `subtype`，這段錯誤訊息就會被當成模型的「回答」：顯示在議題裡、傳給下一位模型，甚至被審查者「審查通過」。

## 證據

- 實測輸出：claude 2.1.252，2026-11-16。
- 測試：`crates/agora-core/src/provider/claude.rs` 的 `is_error_with_success_subtype_is_an_error_not_an_answer`，用的就是上面的 JSON。

## 規則

判斷一則 `result` 成功與否，**一律看 `is_error`**，`subtype` 只當作輔助：`subtype` 以 `error` 開頭也算失敗。認證類的錯誤要轉成可以照做的修復指令（`claude auth login`），不要把原文直接丟給使用者。
