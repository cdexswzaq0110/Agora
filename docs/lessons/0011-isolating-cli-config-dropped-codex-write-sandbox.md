# 0011 — 隔離 CLI 個人設定時，連 Codex 的 Windows 寫入沙盒也一起丟了

- **Date**: 2026-11-16
- **Grade**: 已確認（同一個提示詞、只差一個設定的 A/B 實驗）

## 發生了什麼

v0.4 開發時，Claude 把「手機 QR 連線」交給 GPT（Codex CLI）在獨立的 git worktree 實作，參數是 `codex exec -s workspace-write --ignore-user-config`。Codex 讀完程式、寫好修改，但每次寫入都被拒絕：`patch rejected: writing is blocked by read-only sandbox`。一個檔案也沒改到，卻已經花了 313,274 輸入 tokens。

原因：使用者的 `~/.codex/config.toml` 有：

```toml
[windows]
sandbox = "elevated"
```

`--ignore-user-config` 是 [lessons/0007](0007-cli-personal-config-leaks-into-agent-runs.md) 為了不帶入個人 MCP 工具而加的。它把整個檔案略過，這個 Windows 沙盒設定也跟著消失。Codex 在 Windows 上沒有可用的寫入沙盒時，**不會報錯**，而是把 `workspace-write` 當成唯讀。

**這也影響 Agora 本身**：`isolate_cli_config` 預設開啟，所以工作坊裡「Codex 當實作者」的回合一樣寫不進 worktree。v0.2 實測 Codex 寫程式成功，是在加入隔離之前做的。

## 證據

在暫存資料夾，同一個提示詞（「建立 ok.txt，內容 ok」），`-s workspace-write --ignore-user-config`：

| 實驗 | 額外參數 | 結果 |
|---|---|---|
| A | 無 | `read-only sandbox; rejected by user approval settings`，檔案不存在 |
| B | `-c windows.sandbox="elevated"` | `ok.txt` 內容 `ok` |

兩次各約 4.3 萬輸入 tokens（大多快取）。

## 規則

- `codex::build_args`：隔離個人設定、而且需要寫入時，**只**把 config.toml `[windows]` 表裡的 `sandbox` 值用 `-c windows.sandbox="<值>"` 帶回去（`parse_windows_sandbox`；只接受 `[A-Za-z0-9_-]`）。其他設定（MCP、外掛、預設模型）照樣不帶。測試：`windows_sandbox_is_the_only_key_read_from_config`、`args_pick_sandbox_from_access_and_read_prompt_from_stdin`。
- 「隔離」要列出**功能上必需**的設定，而不是全部丟掉。新增隔離旗標時，要用一次真的寫入確認寫得進去。
- 尚未處理【候選】：使用者的 config.toml 完全沒有 `[windows] sandbox` 時，Codex 仍會是唯讀。之後可以偵測 Codex 輸出裡的 `read-only sandbox`，把它變成一則看得見的錯誤，而不是「實作者什麼都沒改」。
