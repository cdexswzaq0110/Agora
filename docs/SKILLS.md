# Skill 使用說明

Agora 用的 skill 來自 [Serendipity-Epiphany](https://github.com/cdexswzaq0110/Serendipity-Epiphany) v2.0.0（Agent Skills 標準格式）。同一份 skill 用在三個地方：

| 在哪裡 | 怎麼用 | 你要做什麼 |
|---|---|---|
| **Agora** | 每次呼叫時，Agora 自己把對應的 skill 放進提示詞 | 不用做任何事 |
| **Claude Code** | 讀程式庫裡的 `.claude/skills/` | 打 `/se-spec-review` 這類指令，或讓它依說明自動觸發 |
| **Codex** | 讀程式庫裡的 `.agents/skills/` | 打 `$se-spec-review`，或用 `/skills` 選 |

內部怎麼運作（hash 驗證、收據、統計方式）見 [技術說明 15](tech/15-review-and-skills.md)。

## 一、在 Agora 裡

**不用手動選，也沒有開關。** Agora 依階段自動帶入 skill，每次呼叫最多 2 個：

| 你在做什麼 | 自動帶入的 skill |
|---|---|
| 團隊：構想收斂 | `se-problem-definition` |
| 團隊：可行性分析 | `se-feasibility` |
| 團隊：需求與體驗 | `se-product-contract` |
| 團隊：技術設計 | `se-technical-design` |
| 團隊：上面三個階段的整合、修訂 | 該階段的 skill＋`se-spec-review` |
| 團隊：任務拆分；工作坊：提案、整合方案 | `se-work-order` |
| 工作坊：實作、修訂 | `se-minimal-implementation`（修訂時加 `se-debug`） |
| 工作坊：規格審查／品質審查 | `se-spec-review`／`se-quality-review` |
| 單人、協作、討論、辯論 | 不帶 skill |

**在哪裡看**

- 每則發言上有 skill 標籤，標出這次用了哪個 skill。
- **Skills 頁**（命令面板 Ctrl／⌘＋K →「Skills」）：
  - bundle 版本與來源 commit；
  - 每個 skill 是「可用」還是「已停用」；
  - 注入次數；
  - 審查被採用、不採用的次數。

**你要做的事：把「驗收標準」寫清楚**

- 審查 skill 會照方案的驗收標準逐條判定通過、未通過或無法確認，這一步最影響結果。
- 核准方案前，確認每一條都能被別人或程式判定。例如「`/api/x` 回 200」可以判定；「體驗良好」沒辦法判定。
- 核准時直接改方案就行，合約會依你改後的文字重新封存。
- 格式是「驗收標準」標題底下的編號清單（`1.`、`1)`、`1、` 都可以），每條一項。

**限制**

- 每個 skill 最多放 4,000 字進提示詞，`references/` 這一版不放。
- 延伸 skill（`extended/` 裡的 18 個）**不會自動帶入**。

**出問題時**：Skills 頁出現「已停用」，代表那個 skill 的檔案和 manifest 的 hash 對不上。其他 skill 照常運作，但請截圖回報。

## 二、9 個核心 skill

| Skill | 什麼時候用 |
|---|---|
| `se-problem-definition` | 構想還模糊時：釐清問題、使用者、範圍、成功指標、待確認的假設 |
| `se-feasibility` | 投入之前：評估技術、資料、要不要用 AI、成本、風險、安全，給出有前提的做或不做 |
| `se-product-contract` | 寫出使用者故事、可逐條檢查的驗收條件、UX 流程、非功能需求 |
| `se-technical-design` | 架構、元件與 API、資料模型、取捨、回滾、ADR |
| `se-work-order` | 工作單：AC 編號固定不變、每條怎麼驗證、改哪裡、不做什麼、相依關係 |
| `se-minimal-implementation` | 先找既有實作，再做最小變更；不准留 stub，沒有新的驗證證據就不算完成 |
| `se-spec-review` | 唯讀，逐條判定驗收條件；判定「通過」一定要附證據 |
| `se-quality-review` | 規格全部通過後才做；每個發現標高／中／低、位置和理由 |
| `se-debug` | 先重現，再提出可量測的假設，一次只改一個變因，修好後做回歸驗證 |

延伸 skill 的清單在 [`harness/se/INDEX.md`](../harness/se/INDEX.md)，例如 `se-ml-lifecycle`、`se-sdlc`、`se-branch-lifecycle`。不透過 Agora 時，在 Claude Code 或 Codex 裡可以手動叫用。

## 三、不透過 Agora，直接在你的程式庫用

Agora 還沒有「把 harness 裝進你的程式庫」的功能（列在 [ROADMAP](../ROADMAP.md)），目前要手動安裝。

**Claude Code**：在 Serendipity-Epiphany 的 repo 裡執行。bundle 會包含 extended。

```bash
python scripts/sync_skills.py build --out <bundle 資料夾>
python scripts/sync_skills.py install --bundle <bundle 資料夾> --dest <你的程式庫>/.claude/skills
```

**Codex**：在 [Serendipity-Epiphany-Codex](https://github.com/cdexswzaq0110/Serendipity-Epiphany-Codex) 的 repo 裡執行。會裝 9 個核心 skill，加上 Codex 專屬的 4 個（kernel、memory、recover、git）；要 extended 時加 `--with-extended`。

```bash
python scripts/sync_skills.py install --bundle <bundle 資料夾> --dest <你的程式庫>/.agents/skills
```

裝完可以用 `check --dest <同一個路徑>` 驗證 hash。

## 四、要修改 skill 時

**只改 Serendipity-Epiphany 的 `skills/`。** 不要直接改 Agora 的 `harness/se/`，也不要改任何 `.claude/skills`、`.agents/skills`：這些都是同步產生的副本，改了 hash 會對不上，Agora 會停用那個 skill。

1. 在 Serendipity-Epiphany 改 `skills/` 並 commit。
2. `build` 產生新的 bundle。
3. 執行 `install`，並寫明 `--dest`：
   - Agora 用 `--dest <Agora>/harness/se`；
   - Serendipity-Epiphany-Codex 用 `--dest .agents/skills`。
4. Agora 要重新編譯，因為 skill 是編譯時嵌進程式裡的。
