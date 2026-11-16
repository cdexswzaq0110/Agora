//! 議題範本 (Playbooks) — one-click session presets distilled from the user's skill
//! libraries (Serendipity-Epiphany, mattpocock `grill-me`/`tdd`/`to-spec`, milktea, …).

use serde::{Deserialize, Serialize};

use crate::model::{Access, Mode};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub mode: Mode,
    #[serde(default)]
    pub rounds: Option<u32>,
    #[serde(default)]
    pub access: Access,
    /// Pre-filled prompt with headings for the user to complete.
    pub scaffold: String,
    #[serde(default)]
    pub verify_hint: Option<String>,
    #[serde(default)]
    pub blind: bool,
    #[serde(default)]
    pub red_team: bool,
    #[serde(default)]
    pub plan_only: bool,
    /// Offer "寫成 ADR" prominently when done.
    #[serde(default)]
    pub adr: bool,
    /// Where the idea comes from (shown in the UI).
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub builtin: bool,
}

fn t(id: &str, name: &str, summary: &str, mode: Mode, scaffold: &str) -> Template {
    Template {
        id: id.into(),
        name: name.into(),
        summary: summary.into(),
        mode,
        rounds: None,
        access: Access::ReadOnly,
        scaffold: scaffold.into(),
        verify_hint: None,
        blind: false,
        red_team: false,
        plan_only: false,
        adr: false,
        source: None,
        builtin: true,
    }
}

pub fn builtins() -> Vec<Template> {
    let mut v = Vec::new();

    let mut w = t(
        "workshop-feature",
        "新功能：先討論方案再實作",
        "兩個模型各自提案、互評，整合成一份附驗收標準的方案；你核准後才動手，在隔離的 git 分支實作並驗證。",
        Mode::Workshop,
        "## 想做的功能\n\n## 限制與偏好（技術、時間、不能動的地方）\n\n## 怎樣算完成\n",
    );
    w.access = Access::Write;
    w.red_team = true;
    w.verify_hint = Some("cargo test／npm test／python -m pytest".into());
    w.source = Some("Serendipity-Epiphany se-design + se-sdlc".into());
    v.push(w);

    let mut b = t(
        "bug-hunt",
        "Bug 調查與修復",
        "先由兩個模型獨立診斷根因並互相挑錯，再依核准的修法實作，驗證指令通過才算修好。",
        Mode::Workshop,
        "## 症狀\n\n## 重現步驟\n\n## 預期 vs 實際\n\n## 相關檔案／錯誤訊息\n",
    );
    b.access = Access::Write;
    b.verify_hint = Some("能重現這個 bug 的測試指令".into());
    b.source = Some("se-debug + milktea-skills-debug".into());
    v.push(b);

    let mut d = t(
        "tech-choice-adr",
        "技術選型辯論 → ADR",
        "正反方攻防、交叉裁判，結論一鍵寫成專案裡的 ADR。",
        Mode::Debate,
        "要在下列選項之間做決定：\n- 選項 A：\n- 選項 B：\n\n背景：\n\n最在意的考量（依重要性）：\n",
    );
    d.rounds = Some(2);
    d.adr = true;
    d.source = Some("Serendipity-Epiphany templates/ADR.md".into());
    v.push(d);

    let mut g = t(
        "grill-me",
        "需求拷問",
        "兩個模型盡可能找出想法裡的模糊、矛盾與遺漏，各提出最關鍵的問題並排序。適合動手前先跑一次。",
        Mode::Roundtable,
        "我的想法：\n\n請找出模糊、矛盾、遺漏與隱藏假設，提出最關鍵的 10 個問題並依重要性排序。\n",
    );
    g.rounds = Some(1);
    g.source = Some("mattpocock grill-me / milktea grill-check-needs".into());
    v.push(g);

    let mut r = t(
        "pr-review",
        "雙模型程式碼審查（盲審）",
        "兩個模型各自審查，再互相回應；盲審隱藏作者身分，降低偏袒。",
        Mode::Roundtable,
        "請審查下列變更（可以直接讀檔或執行 git diff）：\n範圍：例如 `git diff main...HEAD`、某個資料夾\n\n重點：正確性、安全、邊界條件、可維護性、測試是否足夠\n",
    );
    r.rounds = Some(2);
    r.blind = true;
    r.source = Some("se-two-axis-review + open-code-review".into());
    v.push(r);

    let mut tdd = t(
        "tdd-relay",
        "TDD 協做",
        "執行者先寫會失敗的測試，再實作到通過；審查者確認測試真的在測需求。",
        Mode::Relay,
        "需求：\n\n請先寫會失敗的測試（先執行確認它失敗），再實作到測試通過。不要修改既有測試來讓它通過。\n",
    );
    tdd.access = Access::Write;
    tdd.rounds = Some(2);
    tdd.verify_hint = Some("測試指令，例如 cargo test".into());
    tdd.source = Some("mattpocock tdd / milktea-skills-tdd".into());
    v.push(tdd);

    let mut a = t(
        "arch-review",
        "架構健檢",
        "讀整個專案，評估模組邊界、耦合、測試與技術債，提出優先順序最高的改進。",
        Mode::Roundtable,
        "請閱讀這個專案並評估架構：模組邊界、耦合、測試覆蓋、技術債。\n各自提出優先順序最高的 5 個改進，附上理由與預估成本。\n",
    );
    a.rounds = Some(2);
    a.source = Some("improve-codebase-architecture / sunnydata-architecture-review".into());
    v.push(a);

    let mut s = t(
        "to-spec",
        "想法 → 規格書",
        "把一段模糊的想法整理成有範圍、驗收標準與非目標的規格（只產出方案，不實作）。",
        Mode::Workshop,
        "想法：\n\n請產出：目標、使用者情境、範圍、非目標、驗收標準、風險、開放問題。\n",
    );
    s.plan_only = true;
    s.source = Some("mattpocock to-spec / milktea-skills-to-spec".into());
    v.push(s);

    let mut x = t(
        "digest-discuss",
        "技術情報延伸討論",
        "從每日技術情報挑一則，辯論它值不值得我們採用。",
        Mode::Debate,
        "這項技術／專案值得我們採用嗎？\n\n來源：\n\n我們的情境：\n",
    );
    x.rounds = Some(1);
    v.push(x);

    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_ids_are_unique_and_core_flow_is_first() {
        let b = builtins();
        let mut ids: Vec<_> = b.iter().map(|t| t.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), b.len());
        assert_eq!(b[0].mode, Mode::Workshop);
    }
}
