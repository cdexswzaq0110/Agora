# ADR-0004: 用 git worktree 隔離寫入，而且不在 worktree 裡 commit

- **Status**: accepted
- **Date**: 2026-11-16
- **Evidence grade of the problem statement**: 已確認（v0.1 的「可修改檔案」會直接改使用者的工作目錄，ROADMAP 把它列為最大風險）

## Context

允許模型改檔時，v0.1 直接在專案資料夾裡改。如果模型改壞了、改到一半被取消，或者兩個議題同時寫入，使用者的工作目錄就會被弄亂，而且分不清哪些是自己的修改、哪些是模型的修改。

## Options considered

1. **複製整個資料夾**：簡單，但大型 repo 很慢，也會複製 `node_modules`、`target` 這類目錄。
2. **在 worktree 的分支上 commit，套用時 merge 或 cherry-pick**：需要作者身分，會觸發專案的 commit hooks，還會在使用者的 repo 留下 Agora 的 commit。
3. **worktree，但不 commit；用 `git add -A` 加 `git diff --cached <base>` 算差異，套用時用 `git apply --3way`（選這個）**。

## Decision

選 3：

- `git worktree add -b agora/<session> <資料夾>/worktrees/<session> HEAD`。如果專案是 repo 的子資料夾，agent 的工作目錄要對應到 worktree 裡的同一個子資料夾。
- diff：`git add -A` 之後跑 `git diff --cached --numstat <base>` 和 `git diff --cached <base>`。這樣新增的檔案也會算進去。
- 套用：在 repo 根目錄執行 `git apply --3way -`，把變更變成**使用者工作目錄裡尚未 commit 的修改**，由使用者自己檢查、自己 commit。
- 套用或丟棄之後，用 `git worktree remove --force` 加 `git branch -D` 清乾淨。這個操作是冪等的，重複執行不會出錯。
- 不是 git repo、還沒有任何 commit，或使用者關閉隔離時，改成直接寫入專案資料夾，並在過程紀錄裡清楚說明。

決定性的事實：**Agora 不應該替使用者 commit**。commit 代表「我認可這個變更」，這個判斷屬於使用者。

## Consequences

- worktree 是從 `HEAD` 建立的，所以使用者**未 commit 的修改不會出現在 worktree 裡**。這一點在過程紀錄裡寫明。
- worktree 不包含被 gitignore 的目錄（`node_modules` 等），有些驗證指令可能需要先安裝相依套件；這種情況可以關閉隔離。
- 套用時如果和使用者的未 commit 修改衝突，`git apply --3way` 會失敗並回報原因，worktree 會保留下來，可以之後再試。
- 測試：`git::tests::worktree_roundtrip_diff_apply_remove`（子資料夾專案）、`workshop_waits_for_approval_then_builds_in_an_isolated_worktree`（端到端）。
