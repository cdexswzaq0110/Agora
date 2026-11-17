# 04 · 工作區隔離（git worktree）

原始碼：`crates/agora-core/src/git.rs`、`engine/runner.rs::ensure_workspace`、`engine/ops.rs`（diff／套用／丟棄）。設計理由見 [ADR 0004](../adr/0004-worktree-without-commits.md)。

## 什麼時候建立

需要寫入檔案的時候：

- 方案工作坊在**核准之後**才建立。
- 協做或單人選了「可修改檔案」時，一開始就建立。

接著判斷：

```
spec.isolate == false        → 直接寫入（Warn：已關閉隔離）
程式庫不是 git repo            → 直接寫入（Warn）
create_worktree 失敗（例如還沒有 commit） → 直接寫入（Warn，附原因）
否則                          → worktree（Info：分支、基準 commit、未 commit 的修改不包含在內）
```

## 流程

```mermaid
sequenceDiagram
    participant A as Agora
    participant G as git
    participant W as worktree（<資料夾>/worktrees/<session>）
    participant P as 你的專案
    A->>G: worktree add -b agora/<session> <W> HEAD
    Note over W: CLI 的工作目錄＝W（若專案是子資料夾，則是 W/<子資料夾>）
    A->>W: 執行者改檔、驗證指令
    A->>G: (in W) add -A; diff --cached --numstat <base>; diff --cached <base>
    A-->>A: 介面顯示檔案清單與逐行差異
    alt 套用到程式庫
        A->>G: (in W) diff --cached --binary <base>
        A->>P: (repo 根目錄) git apply --3way -
        A->>G: worktree remove --force; branch -D
    else 丟棄
        A->>G: worktree remove --force; branch -D
    end
```

## 狀態

`Workspace { kind: worktree|direct, path, root, branch, base, state: active|applied|discarded, message }`

- 專案還在進行時不能套用或丟棄（`finished_record`）。
- 套用或丟棄只能做一次，重複操作會回錯誤。
- `direct` 模式下，`/diff` 會改成顯示專案相對於 `HEAD` 的變更，可能包含你自己的未 commit 修改，介面上會註明。

## 已知限制

- worktree 不包含被 gitignore 的目錄（`node_modules`、`target`……），驗證指令可能需要先安裝相依套件。
- 你未 commit 的修改不在 worktree 裡；套用時如果和它們衝突，`git apply --3way` 會失敗並回報原因，worktree 會保留下來。

## 證據

- `git::tests::worktree_roundtrip_diff_apply_remove`：子資料夾專案、新增與修改檔案、套用、清理分支。【已確認】
- 端到端測試 `workshop_waits_for_approval_then_builds_in_an_isolated_worktree`。【已確認】
- 實機：真的 Codex 在 worktree 新增 12 行，套用前 demo repo 沒有變動，套用後出現暫存的變更。【已確認，見 devlog v0.2】
