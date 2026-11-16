# Core Rules — Agora

> 改編自 [Serendipity-Epiphany](https://github.com/cdexswzaq0110/Serendipity-Epiphany) 的 `.claude/rules/core-rules.md`，濃縮成管這個 repo 的六條。完整原文在來源 repo。

1. **來源優先、說明出處**：提出設計前，先看這個 repo 自己的程式碼、測試和文件。資訊不足的地方，標上[證據等級](../../CONTEXT.md)，不要把猜測寫成事實。
2. **計費紅線**：任何改動都不能新增 API key 計費路徑（[ADR 0001](../../docs/adr/0001-cli-only-never-paid-api.md)）。碰到 provider 的改動，一定要附上計費防護的測試。
3. **保護既有工作**：不碰無關的進行中修改。破壞性的 git 操作，先看 `git status`／`git log`；有任何意外狀況，就停下來問。手動測試用暫存資料夾，不碰使用者的 `%APPDATA%\Agora`。
4. **完成要有證據**：功能「完成」的標準是：測試通過，而且有實際執行的輸出（API 回應、截圖、議題紀錄）。只是編譯得過，不算完成。沒跑過的測試標成「未驗證」，不能寫成「已確認」。
5. **最小必要改動**：先找現有模組（額度看 `quota.rs`、子行程看 `provider/proc.rs`、上下文看 `compact.rs`），再考慮寫新的。
6. **可搜尋的教訓**：實際踩到的坑，要在 [`docs/lessons/`](../../docs/lessons/INDEX.md) 留一則有日期、有失敗證據的紀錄，不能只藏在 commit diff 裡。
