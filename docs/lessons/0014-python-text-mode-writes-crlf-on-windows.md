# 0014 — 在 Windows 用 Python 文字模式改檔，會把 LF 換成 CRLF

- **Date**: 2026-11-16
- **Grade**: 已確認

## 發生了什麼

v0.5 實作團隊工作流時，為了一次改好幾個 Rust 檔，用了 `python - <<'EOF'` 的小腳本：`open(p, encoding='utf-8').read()` → 取代 → `open(p, 'w', encoding='utf-8').write(s)`。

這個 repo 的 `.gitattributes` 是 `* text=auto eol=lf`，檔案原本都是 LF。Python 在 Windows 的文字模式會在**讀取時**把 `\r\n` 轉成 `\n`、在**寫入時**把每個 `\n` 轉成 `\r\n`，結果被腳本碰過的 7 個檔案整份變成 CRLF。編譯與測試都不受影響，所以沒有任何測試會抓到。

## 證據

- `git add -A` 時出現 `warning: in the working copy of 'crates/agora-core/src/engine/modes.rs', CRLF will be replaced by LF the next time Git touches it`，同樣的警告共 10 個檔案。
- `grep -lU $'\r'` 在變更清單裡找到 `modes.rs`、`runner.rs`、`event.rs`、`model.rs`、`prompts.rs`、`mock.rs`、`store.rs`（以及兩個新檔）；用 `newline=''` 讀寫並把 `\r\n` 換回 `\n` 後，同一個指令沒有輸出。

## 規則

- 腳本改檔一律 `open(p, encoding='utf-8', newline='')`（讀寫都要），保留原本的換行；能用編輯工具（Edit）就用編輯工具。
- 提交前看到 `CRLF will be replaced by LF` 的警告不能略過：它代表工作區的檔案和 repo 的換行規則不一致，diff 會變成整份檔案。
- 和 [0008](0008-node-check-misses-esm-syntax-errors.md) 記錄的跳脫字元問題同一類：**改檔的工具本身會改內容**。改完要用 `git diff --stat` 確認行數變化合理。
