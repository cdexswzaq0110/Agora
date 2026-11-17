# 0015 — 在這個環境用 Bash heredoc 跑 Python，反斜線會被減半

- **Date**: 2026-11-17
- **Grade**: 已確認

## 發生了什麼

v0.6 要在 `prompts.rs` 的 Rust 字串常值後面插入文字，錨點是 `…的行為）\n\`（反斜線 n、反斜線，換行）。用 `python - <<'PYEOF'` 的 heredoc 跑一段取代腳本，腳本裡寫的是 `'...\\n\\'`。

結果 Python 看到的原始碼變成 `'...\n\'`——每個 `\\` 都變成 `\`，於是 `\'` 吃掉了結尾引號：`SyntaxError: unterminated string literal`。前一次的版本沒有語法錯誤，但錨點因為少了反斜線而找不到（`AssertionError`）。同一個腳本存成檔案、用 `python 檔案.py` 執行就完全正常。

這和 [0008](0008-node-check-misses-esm-syntax-errors.md)、[0014](0014-python-text-mode-writes-crlf-on-windows.md) 是同一類問題：**改檔的工具本身改了內容**。這次是 shell 把 heredoc 裡的反斜線處理掉了（即使用了加引號的 `'PYEOF'`）。

另外，這台繁中 Windows 上 Python 的 `sys.stdin.encoding` 是 `cp950`、`utf8_mode` 是 0。

## 證據

- 失敗的輸出：`print('anchor found:', '…\n\' in s)` → `SyntaxError: unterminated string literal (detected at line 3)`。
- 同樣內容的 `scratchpad/v06/prompts_v06.py` 以檔案執行：錨點找到、取代成功、`cargo build` 通過。

## 規則

- 含反斜線（尤其是 Rust／JSON 字串裡的 `\n`、`\"`）的改檔腳本，一律**用編輯工具寫成檔案再執行**，不用 inline heredoc。
- 腳本改檔照 0014：`encoding='utf-8'`、`newline=''`；改完用 `git diff --stat` 看行數是否合理。
- 執行 Python 時加 `PYTHONUTF8=1`，避免主控台編碼影響。
