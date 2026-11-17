//! v0.6 auditable review (after review-bridge): the acceptance contract is cut from the approved
//! plan by code and sealed with SHA-256; the spec reviewer answers every condition once, by id,
//! in a fenced `review.result` JSON block; code — never a model — decides whether that answer is
//! acceptable. Pure functions; the build loop in `engine/modes.rs` drives them.
//! Design: docs/design/2026-11-17-v0.6/05-final-plan.md (C.4 and red-team #3, #4, #7, #8).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_CRITERIA: usize = 30;
pub const MAX_CRITERION_CHARS: usize = 500;

/// One acceptance condition of the approved plan.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Criterion {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub verification: String,
}

/// The sealed acceptance contract of one approved plan revision.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Contract {
    pub sha256: String,
    pub criteria: Vec<Criterion>,
    /// `json` (an `acceptance_contract` block) or `markdown` (the 驗收標準 list).
    pub source: String,
    /// Conditions cut short or dropped to fit the limits (never silently).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn truncate(s: &str, max: usize) -> (String, bool) {
    if s.chars().count() <= max {
        (s.to_string(), false)
    } else {
        (s.chars().take(max).collect::<String>() + "…", true)
    }
}

/// Fenced blocks (```info … ```) outside of other fences: (info string, body).
fn fenced_blocks(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut cur: Option<(String, String, String)> = None; // (fence, info, body)
    for line in text.lines() {
        let t = line.trim_start();
        match &mut cur {
            Some((fence, _, body)) => {
                if t.starts_with(fence.as_str()) && t.trim_start_matches(fence.chars().next().unwrap()).trim().is_empty() {
                    let (_, info, body) = cur.take().unwrap();
                    out.push((info, body));
                } else {
                    body.push_str(line);
                    body.push('\n');
                }
            }
            None => {
                for f in ["```", "~~~"] {
                    if let Some(rest) = t.strip_prefix(f) {
                        let fence: String = t.chars().take_while(|c| *c == f.chars().next().unwrap()).collect();
                        let info = rest.trim_start_matches(f.chars().next().unwrap()).trim().to_string();
                        cur = Some((fence, info, String::new()));
                        break;
                    }
                }
            }
        }
    }
    out
}

/// Cut the contract from an approved plan: a fenced `acceptance_contract` block (JSON array of
/// `{id, text, verification}`, or `{"acceptance_contract": [...]}` in a json block) wins;
/// otherwise the outermost numbered list under the first heading containing 「驗收標準」.
pub fn parse_contract(plan: &str) -> Option<Contract> {
    let mut notes = Vec::new();
    let (criteria, source) = match contract_from_json(plan) {
        Some(c) => (c, "json"),
        None => (contract_from_markdown(plan), "markdown"),
    };
    if criteria.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for (i, mut c) in criteria.into_iter().enumerate() {
        if i >= MAX_CRITERIA {
            notes.push(format!("驗收條件超過 {MAX_CRITERIA} 條，第 {} 條以後沒有納入合約", MAX_CRITERIA + 1));
            break;
        }
        let (text, cut) = truncate(c.text.trim(), MAX_CRITERION_CHARS);
        if cut {
            notes.push(format!("{} 超過 {MAX_CRITERION_CHARS} 字，已截斷", c.id));
        }
        c.text = text;
        out.push(c);
    }
    let canonical = serde_json::to_string(&serde_json::json!({ "plan": plan.trim(), "criteria": out })).unwrap_or_default();
    Some(Contract { sha256: sha256_hex(canonical.as_bytes()), criteria: out, source: source.into(), notes })
}

fn contract_from_json(plan: &str) -> Option<Vec<Criterion>> {
    for (info, body) in fenced_blocks(plan).into_iter().rev() {
        let value: serde_json::Value = match info.as_str() {
            "acceptance_contract" => serde_json::from_str(&body).ok()?,
            "json" => match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(v) if v.get("acceptance_contract").is_some() => v["acceptance_contract"].clone(),
                _ => continue,
            },
            _ => continue,
        };
        let items: Vec<Criterion> = serde_json::from_value(value).ok()?;
        let mut seen = std::collections::HashSet::new();
        let valid = items.iter().all(|c| is_ac_id(&c.id) && seen.insert(c.id.clone()) && !c.text.trim().is_empty());
        return valid.then_some(items);
    }
    None
}

fn is_ac_id(id: &str) -> bool {
    id.strip_prefix("AC").is_some_and(|n| !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit()))
}

/// The documented Markdown grammar (red-team #4): the first heading containing 「驗收標準」, up to
/// the next heading; code fences ignored; each outermost ordered-list item (`1.`, `1)`, `1、`
/// at indent ≤ 3) is one condition, with the indented or wrapped lines under it.
fn contract_from_markdown(plan: &str) -> Vec<Criterion> {
    let mut items: Vec<String> = Vec::new();
    let mut inside = false;
    let mut fence = false;
    for line in plan.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        if t.starts_with('#') {
            if inside {
                break;
            }
            inside = t.contains("驗收標準");
            continue;
        }
        if !inside || t.is_empty() {
            continue;
        }
        let indent = line.len() - t.len();
        let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rest = &t[digits.len()..];
        let marker = !digits.is_empty() && (rest.starts_with(". ") || rest.starts_with(") ") || rest.starts_with('、'));
        if marker && indent <= 3 {
            let body = rest.trim_start_matches(['.', ')', '、']).trim();
            items.push(body.to_string());
        } else if let Some(last) = items.last_mut() {
            last.push(' ');
            last.push_str(t);
        }
    }
    items
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .enumerate()
        .map(|(i, text)| Criterion { id: format!("AC{}", i + 1), text, verification: String::new() })
        .collect()
}

// ------------------------------------------------------------------ the reviewer's answer

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcVerdict {
    Pass,
    Fail,
    CannotVerify,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcResult {
    pub id: String,
    pub verdict: AcVerdict,
    /// `command` (cites `verify_round`), `code` (cites `path`), `doc`.
    #[serde(default)]
    pub evidence_kind: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_round: Option<u32>,
    /// Required for `cannot_verify`.
    #[serde(default)]
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualityFinding {
    pub id: String,
    pub severity: Severity,
    pub text: String,
    #[serde(default)]
    pub location: String,
}

/// What the program can check about the evidence; meaning stays the model's judgement.
pub struct EvidenceContext<'a> {
    /// Rounds of verify runs recorded in this session.
    pub verify_rounds: &'a [u32],
    /// Does this worktree-relative path exist?
    pub path_exists: &'a dyn Fn(&str) -> bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpecParse {
    pub verdicts: Vec<AcResult>,
    /// Every reason the answer is not acceptable. Non-empty → the review does not count.
    pub problems: Vec<String>,
}

/// The last fenced `review.result` block of the reviewer's **own** answer (red-team #3b).
fn last_result_block(answer: &str) -> Option<String> {
    fenced_blocks(answer).into_iter().rev().find(|(info, _)| info == "review.result").map(|(_, b)| b)
}

fn safe_relative(path: &str) -> bool {
    let p = path.replace('\\', "/");
    !p.is_empty() && !p.starts_with('/') && !p.contains(':') && !p.split('/').any(|seg| seg == "..")
}

/// Validate a spec reviewer's answer against the sealed contract.
pub fn parse_spec_review(answer: &str, contract: &Contract, ev: &EvidenceContext) -> SpecParse {
    let mut out = SpecParse::default();
    let Some(block) = last_result_block(answer) else {
        out.problems.push("沒有 `review.result` 區塊".into());
        return out;
    };
    let v: serde_json::Value = match serde_json::from_str(&block) {
        Ok(v) => v,
        Err(e) => {
            out.problems.push(format!("review.result 不是有效的 JSON：{e}"));
            return out;
        }
    };
    if v.get("contract_sha256").and_then(|x| x.as_str()) != Some(contract.sha256.as_str()) {
        out.problems.push("contract_sha256 與核准的合約不符".into());
    }
    let verdicts: Vec<AcResult> = match v.get("verdicts").cloned().map(serde_json::from_value) {
        Some(Ok(x)) => x,
        _ => {
            out.problems.push("verdicts 缺少或格式錯誤".into());
            return out;
        }
    };
    let mut seen = std::collections::HashSet::new();
    for r in &verdicts {
        if !contract.criteria.iter().any(|c| c.id == r.id) {
            out.problems.push(format!("未知的條件 {}", r.id));
        }
        if !seen.insert(r.id.clone()) {
            out.problems.push(format!("{} 重複判定", r.id));
        }
        match r.verdict {
            AcVerdict::Pass => {
                if r.evidence.trim().is_empty() {
                    out.problems.push(format!("{} 判定通過但沒有證據", r.id));
                }
                match r.evidence_kind.as_str() {
                    "command" => {
                        if !r.verify_round.is_some_and(|n| ev.verify_rounds.contains(&n)) {
                            out.problems.push(format!("{} 引用的驗證紀錄不存在", r.id));
                        }
                    }
                    "code" => match r.path.as_deref() {
                        Some(p) if safe_relative(p) && (ev.path_exists)(p) => {}
                        Some(p) => out.problems.push(format!("{} 引用的檔案 {p} 不存在或不在工作區內", r.id)),
                        None => out.problems.push(format!("{} 的程式證據沒有 path", r.id)),
                    },
                    "doc" => {}
                    other => out.problems.push(format!("{} 的 evidence_kind「{other}」不合法", r.id)),
                }
            }
            AcVerdict::CannotVerify if r.reason.trim().is_empty() => out.problems.push(format!("{} 無法確認但沒有原因", r.id)),
            _ => {}
        }
    }
    for c in &contract.criteria {
        if !seen.contains(&c.id) {
            out.problems.push(format!("缺少 {} 的判定", c.id));
        }
    }
    if v.get("approved").and_then(|x| x.as_bool()) == Some(true) && verdicts.iter().any(|r| r.verdict != AcVerdict::Pass) {
        out.problems.push("整體標為通過，但有條件不是 pass".into());
    }
    out.verdicts = verdicts;
    out
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QualityParse {
    pub findings: Vec<QualityFinding>,
    pub problems: Vec<String>,
}

pub fn parse_quality_review(answer: &str) -> QualityParse {
    let mut out = QualityParse::default();
    let Some(block) = last_result_block(answer) else {
        out.problems.push("沒有 `review.result` 區塊".into());
        return out;
    };
    let v: serde_json::Value = match serde_json::from_str(&block) {
        Ok(v) => v,
        Err(e) => {
            out.problems.push(format!("review.result 不是有效的 JSON：{e}"));
            return out;
        }
    };
    match v.get("findings").cloned().map(serde_json::from_value::<Vec<QualityFinding>>) {
        Some(Ok(f)) => {
            let mut seen = std::collections::HashSet::new();
            for x in &f {
                if x.id.trim().is_empty() || !seen.insert(x.id.clone()) {
                    out.problems.push(format!("發現的 id「{}」空白或重複", x.id));
                }
            }
            out.findings = f;
        }
        _ => out.problems.push("findings 缺少或格式錯誤".into()),
    }
    out
}

/// What the spec verdicts mean for the build loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpecOutcome {
    AllPass,
    /// At least one fail: back to the builder with these ids.
    Failed(Vec<String>),
    /// No fail, but some could not be verified: the human decides.
    Unverified(Vec<String>),
}

pub fn spec_outcome(verdicts: &[AcResult]) -> SpecOutcome {
    let ids = |v: AcVerdict| verdicts.iter().filter(|r| r.verdict == v).map(|r| r.id.clone()).collect::<Vec<_>>();
    let failed = ids(AcVerdict::Fail);
    if !failed.is_empty() {
        return SpecOutcome::Failed(failed);
    }
    let unverified = ids(AcVerdict::CannotVerify);
    if !unverified.is_empty() {
        return SpecOutcome::Unverified(unverified);
    }
    SpecOutcome::AllPass
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN_MD: &str = "### 目標\n做 X\n### 驗收標準\n1. 匯出 CSV 有標題列\n   且欄位用逗號分隔\n2) 空資料時匯出只有標題\n```\n3. 這不是條件\n```\n3、舊測試全部通過\n   - 子項目屬於第 3 條\n### 不做的事\n4. 不是條件\n";

    fn contract() -> Contract {
        parse_contract(PLAN_MD).unwrap()
    }

    #[test]
    fn markdown_grammar_merges_continuations_and_skips_fences() {
        let c = contract();
        assert_eq!(c.source, "markdown");
        let ids: Vec<&str> = c.criteria.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["AC1", "AC2", "AC3"]);
        assert_eq!(c.criteria[0].text, "匯出 CSV 有標題列 且欄位用逗號分隔");
        assert!(c.criteria[2].text.contains("子項目屬於第 3 條"));
        assert!(parse_contract("沒有驗收標準").is_none());
    }

    #[test]
    fn json_contract_wins_and_is_validated() {
        let plan = format!("{PLAN_MD}\n```acceptance_contract\n[{{\"id\":\"AC1\",\"text\":\"一\",\"verification\":\"cargo test\"}},{{\"id\":\"AC2\",\"text\":\"二\"}}]\n```\n");
        let c = parse_contract(&plan).unwrap();
        assert_eq!((c.source.as_str(), c.criteria.len()), ("json", 2));
        assert_eq!(c.criteria[0].verification, "cargo test");
        // A duplicate id makes the JSON invalid → the Markdown list is used instead.
        let dup = format!("{PLAN_MD}\n```acceptance_contract\n[{{\"id\":\"AC1\",\"text\":\"一\"}},{{\"id\":\"AC1\",\"text\":\"二\"}}]\n```\n");
        assert_eq!(parse_contract(&dup).unwrap().source, "markdown");
    }

    #[test]
    fn limits_are_recorded_never_silent_and_no_ten_item_cap() {
        let many: String = (1..=35).map(|i| format!("{i}. 條件 {i}\n")).collect();
        let c = parse_contract(&format!("## 驗收標準\n{many}")).unwrap();
        assert_eq!(c.criteria.len(), MAX_CRITERIA);
        assert!(c.notes.iter().any(|n| n.contains("超過 30 條")));
        let long = parse_contract(&format!("## 驗收標準\n1. {}\n", "字".repeat(600))).unwrap();
        assert!(long.notes.iter().any(|n| n.contains("AC1")));
        // The hash covers the plan text: any edit at approval makes a new contract.
        assert_ne!(parse_contract(&format!("{PLAN_MD}補充說明")).unwrap().sha256, contract().sha256);
    }

    fn ev<'a>(rounds: &'a [u32], exists: &'a dyn Fn(&str) -> bool) -> EvidenceContext<'a> {
        EvidenceContext { verify_rounds: rounds, path_exists: exists }
    }

    fn answer(c: &Contract, verdicts: &str) -> String {
        format!("說明文字\n```review.result\n{{\"contract_sha256\":\"{}\",\"verdicts\":{verdicts}}}\n```\n", c.sha256)
    }

    #[test]
    fn spec_review_is_rejected_for_every_kind_of_gap() {
        let c = contract();
        let exists = |p: &str| p == "src/export.rs";
        let e = ev(&[1], &exists);
        let ok = r#"[{"id":"AC1","verdict":"pass","evidence_kind":"code","path":"src/export.rs","evidence":"第 10 行寫標題列"},{"id":"AC2","verdict":"pass","evidence_kind":"command","verify_round":1,"evidence":"cargo test 通過"},{"id":"AC3","verdict":"fail","evidence":"有一個測試失敗"}]"#;
        let good = parse_spec_review(&answer(&c, ok), &c, &e);
        assert!(good.problems.is_empty(), "{:?}", good.problems);
        assert_eq!(spec_outcome(&good.verdicts), SpecOutcome::Failed(vec!["AC3".into()]));
        let bad = |verdicts: &str| parse_spec_review(&answer(&c, verdicts), &c, &e).problems;
        assert!(bad(r#"[{"id":"AC1","verdict":"pass","evidence_kind":"doc","evidence":"x"},{"id":"AC2","verdict":"pass","evidence_kind":"doc","evidence":"x"}]"#).iter().any(|p| p.contains("缺少 AC3")));
        assert!(bad(r#"[{"id":"AC1","verdict":"fail"},{"id":"AC1","verdict":"fail"},{"id":"AC2","verdict":"fail"},{"id":"AC3","verdict":"fail"}]"#).iter().any(|p| p.contains("重複")));
        assert!(bad(r#"[{"id":"AC9","verdict":"fail"},{"id":"AC1","verdict":"fail"},{"id":"AC2","verdict":"fail"},{"id":"AC3","verdict":"fail"}]"#).iter().any(|p| p.contains("未知")));
        assert!(bad(r#"[{"id":"AC1","verdict":"pass","evidence_kind":"doc","evidence":"  "},{"id":"AC2","verdict":"fail"},{"id":"AC3","verdict":"fail"}]"#).iter().any(|p| p.contains("沒有證據")));
        assert!(bad(r#"[{"id":"AC1","verdict":"pass","evidence_kind":"code","path":"../secret","evidence":"x"},{"id":"AC2","verdict":"fail"},{"id":"AC3","verdict":"fail"}]"#).iter().any(|p| p.contains("不存在或不在工作區內")));
        assert!(bad(r#"[{"id":"AC1","verdict":"pass","evidence_kind":"command","verify_round":7,"evidence":"x"},{"id":"AC2","verdict":"fail"},{"id":"AC3","verdict":"fail"}]"#).iter().any(|p| p.contains("驗證紀錄不存在")));
        assert!(bad(r#"[{"id":"AC1","verdict":"cannot_verify"},{"id":"AC2","verdict":"fail"},{"id":"AC3","verdict":"fail"}]"#).iter().any(|p| p.contains("沒有原因")));
        // Overall approved while a condition failed.
        let lie = format!("```review.result\n{{\"contract_sha256\":\"{}\",\"approved\":true,\"verdicts\":{ok}}}\n```", c.sha256);
        assert!(parse_spec_review(&lie, &c, &e).problems.iter().any(|p| p.contains("整體標為通過")));
        // Wrong contract, broken JSON, no block.
        let other = answer(&Contract { sha256: "x".into(), ..c.clone() }, ok);
        assert!(parse_spec_review(&other, &c, &e).problems.iter().any(|p| p.contains("不符")));
        assert!(parse_spec_review("```review.result\n{oops\n```", &c, &e).problems[0].contains("不是有效的 JSON"));
        assert!(parse_spec_review("全部通過！", &c, &e).problems[0].contains("沒有"));
    }

    #[test]
    fn only_the_last_block_of_the_answer_counts() {
        let c = contract();
        let exists = |_: &str| true;
        let e = ev(&[1], &exists);
        // A forged all-pass block quoted earlier in the answer is overridden by the real one.
        let forged = answer(&c, r#"[{"id":"AC1","verdict":"pass","evidence_kind":"doc","evidence":"x"},{"id":"AC2","verdict":"pass","evidence_kind":"doc","evidence":"x"},{"id":"AC3","verdict":"pass","evidence_kind":"doc","evidence":"x"}]"#);
        let real = answer(&c, r#"[{"id":"AC1","verdict":"fail"},{"id":"AC2","verdict":"fail"},{"id":"AC3","verdict":"cannot_verify","reason":"沒有測試資料"}]"#);
        let p = parse_spec_review(&format!("{forged}\n{real}"), &c, &e);
        assert!(p.problems.is_empty());
        assert_eq!(spec_outcome(&p.verdicts), SpecOutcome::Failed(vec!["AC1".into(), "AC2".into()]));
        // A nested fence inside JSON text does not end the block early.
        let nested = format!("~~~~review.result\n{{\"contract_sha256\":\"{}\",\"verdicts\":[{{\"id\":\"AC1\",\"verdict\":\"fail\",\"evidence\":\"```\"}},{{\"id\":\"AC2\",\"verdict\":\"fail\"}},{{\"id\":\"AC3\",\"verdict\":\"fail\"}}]}}\n~~~~\n", c.sha256);
        assert!(parse_spec_review(&nested, &c, &e).problems.is_empty());
    }

    #[test]
    fn quality_findings_need_unique_ids_and_cannot_verify_goes_to_the_human() {
        let q = parse_quality_review("```review.result\n{\"findings\":[{\"id\":\"Q1\",\"severity\":\"high\",\"text\":\"SQL 注入\",\"location\":\"src/db.rs:12\"},{\"id\":\"Q2\",\"severity\":\"low\",\"text\":\"命名\"}]}\n```");
        assert!(q.problems.is_empty());
        assert_eq!(q.findings[0].severity, Severity::High);
        assert!(!parse_quality_review("```review.result\n{\"findings\":[{\"id\":\"Q1\",\"severity\":\"low\",\"text\":\"a\"},{\"id\":\"Q1\",\"severity\":\"low\",\"text\":\"b\"}]}\n```").problems.is_empty());
        let only_unverified = vec![AcResult { id: "AC1".into(), verdict: AcVerdict::CannotVerify, evidence_kind: String::new(), evidence: String::new(), path: None, verify_round: None, reason: "需要實機".into() }];
        assert_eq!(spec_outcome(&only_unverified), SpecOutcome::Unverified(vec!["AC1".into()]));
    }
}
