//! Turn a session into durable artifacts: a Markdown transcript, an ADR in the project's
//! `docs/adr/`, or a lesson draft in `docs/lessons/` (Serendipity-Epiphany formats). Files are
//! only written when the user presses the button — never automatically.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::compact::truncate_chars;
use crate::engine::format_date;
use crate::model::*;
use crate::prompts;

fn turn_label(t: &Turn) -> String {
    format!(
        "第 {} 輪｜{}（{}）{}",
        t.round,
        t.family.label(),
        t.role.label(),
        t.model.as_deref().map(|m| format!(" · {m}")).unwrap_or_default()
    )
}

/// Full transcript as Markdown (for download / archiving).
pub fn session_markdown(r: &SessionRecord, project: Option<&Project>) -> String {
    let mut s = format!("# {}\n\n", r.title);
    s.push_str(&format!(
        "- 模式：{}\n- 專案：{}\n- 建立：{}\n- 狀態：{:?}\n- Agora 議題：`{}`\n\n",
        r.spec.mode.label(),
        project.map(|p| p.name.as_str()).unwrap_or("—"),
        format_date(r.created_at),
        r.status,
        r.id
    ));
    s.push_str("## 議題\n\n");
    s.push_str(r.spec.prompt.trim());
    s.push_str("\n\n");
    if let Some(v) = &r.verdict {
        s.push_str(&format!("## 結論（證據等級：{}）\n\n**{}**\n\n{}\n\n", v.grade.label(), v.headline, v.summary.trim()));
        if !v.open_issues.is_empty() {
            s.push_str("待處理：\n\n");
            for i in &v.open_issues {
                s.push_str(&format!("- {i}\n"));
            }
            s.push('\n');
        }
    }
    if let Some(p) = &r.plan {
        s.push_str(&format!("## 核准的方案（第 {} 版{}）\n\n{}\n\n", p.revision + 1, if p.edited { "，經人工修改" } else { "" }, p.text.trim()));
    }
    s.push_str("## 發言紀錄\n\n");
    for t in &r.turns {
        s.push_str(&format!("### {}{}\n\n", turn_label(t), if t.seeded { "（承接自上一個議題）" } else { "" }));
        if t.ok {
            s.push_str(t.text.trim());
        } else {
            s.push_str(&format!("> 失敗：{}", t.error.as_deref().unwrap_or("")));
        }
        s.push_str("\n\n");
    }
    for v in &r.verify_runs {
        s.push_str(&format!(
            "### 驗證（第 {} 輪）：`{}` → {}\n\n```\n{}\n```\n\n",
            v.round,
            v.command,
            if v.ok { "通過" } else { "失敗" },
            truncate_chars(v.output_tail.trim(), 3000)
        ));
    }
    if !r.notes.is_empty() {
        s.push_str("## 過程紀錄\n\n");
        for n in &r.notes {
            s.push_str(&format!("- {}\n", n.text));
        }
    }
    s
}

/// ASCII slug for file names; falls back to `fallback` when the title has no ASCII words.
pub fn slug(title: &str, fallback: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').chars().take(48).collect::<String>();
    if out.len() < 3 {
        fallback.to_string()
    } else {
        out
    }
}

/// Next free `NNNN` in a directory of `NNNN-*.md` files.
pub fn next_number(dir: &Path) -> u32 {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_string_lossy().get(..4).and_then(|n| n.parse::<u32>().ok()))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
        + 1
}

fn position_headlines(r: &SessionRecord) -> Vec<String> {
    let first_round: Vec<&Turn> = r.turns.iter().filter(|t| t.ok && t.round == 1).collect();
    first_round
        .iter()
        .map(|t| {
            let body = prompts::section(&t.text, "主張")
                .or_else(|| prompts::section(&t.text, "立場"))
                .or_else(|| prompts::section(&t.text, "方案"))
                .or_else(|| prompts::section(&t.text, "替代方案"))
                .unwrap_or_else(|| t.text.clone());
            format!("**{}（{}）**：{}", t.family.label(), t.role.label(), prompts::headline(&body, 160))
        })
        .collect()
}

pub fn adr_markdown(r: &SessionRecord, number: u32) -> Result<String> {
    let Some(v) = &r.verdict else { bail!("這個議題還沒有結論") };
    let options = position_headlines(r);
    let mut s = format!("# ADR-{number:04}: {}\n\n", r.title);
    s.push_str(&format!(
        "- **Status**: proposed\n- **Date**: {}\n- **Evidence grade of the decision**: {}\n- **Source**: Agora 議題 `{}`（{}）\n\n",
        format_date(r.created_at),
        v.grade.label(),
        r.id,
        r.spec.mode.label()
    ));
    s.push_str("## Context\n\n");
    s.push_str(r.spec.prompt.trim());
    s.push_str("\n\n## Options considered\n\n");
    if options.is_empty() {
        s.push_str("（見議題發言紀錄）\n");
    }
    for o in options {
        s.push_str(&format!("1. {o}\n"));
    }
    s.push_str(&format!("\n## Decision\n\n**{}**\n\n{}\n\n", v.headline, v.summary.trim()));
    s.push_str("## Consequences\n\n");
    if v.open_issues.is_empty() {
        s.push_str("- （尚未列出）\n");
    }
    for i in &v.open_issues {
        s.push_str(&format!("- 待驗證：{i}\n"));
    }
    Ok(s)
}

/// Write `docs/adr/NNNN-<slug>.md` in the project. Returns the path.
pub fn write_adr(project_dir: &Path, r: &SessionRecord) -> Result<PathBuf> {
    let dir = project_dir.join("docs").join("adr");
    std::fs::create_dir_all(&dir)?;
    let n = next_number(&dir);
    let path = dir.join(format!("{n:04}-{}.md", slug(&r.title, "decision")));
    std::fs::write(&path, adr_markdown(r, n)?)?;
    Ok(path)
}

pub fn lesson_markdown(r: &SessionRecord, number: u32) -> String {
    let date = format_date(r.created_at);
    let mut s = format!("# {number:04} — {}\n\n- **Date**: {date}\n- **Grade**: 候選（草稿，請補上失敗證據後再升級）\n- **Source**: Agora 議題 `{}`\n\n", r.title, r.id);
    s.push_str("## 發生了什麼\n\n");
    if let Some(e) = &r.error {
        s.push_str(&format!("議題中止：{e}\n\n"));
    }
    for t in r.turns.iter().filter(|t| !t.ok) {
        s.push_str(&format!("- {} 失敗：{}\n", turn_label(t), t.error.as_deref().unwrap_or("")));
    }
    for v in r.verify_runs.iter().filter(|v| !v.ok) {
        s.push_str(&format!("- 驗證 `{}` 失敗（exit {:?}）\n", v.command, v.exit_code));
    }
    if let Some(v) = &r.verdict {
        s.push_str(&format!("\n結論：{}（{}）\n", v.headline, v.grade.label()));
        for i in &v.open_issues {
            s.push_str(&format!("- 未解決：{i}\n"));
        }
    }
    s.push_str("\n## 證據\n\n- （補上重現步驟、錯誤輸出或修正前會失敗的測試）\n\n## 規則\n\n- （這次之後要改變的做法）\n");
    let guards: Vec<&Note> = r.notes.iter().filter(|n| n.level != NoteLevel::Info).collect();
    if !guards.is_empty() {
        s.push_str("\n## 過程中觸發的警示\n\n");
        for n in guards {
            s.push_str(&format!("- {}\n", n.text));
        }
    }
    s
}

/// Write `docs/lessons/NNNN-<slug>.md` and append a row to `INDEX.md` when it exists.
pub fn write_lesson(project_dir: &Path, r: &SessionRecord) -> Result<PathBuf> {
    let dir = project_dir.join("docs").join("lessons");
    std::fs::create_dir_all(&dir)?;
    let n = next_number(&dir);
    let file = format!("{n:04}-{}.md", slug(&r.title, "lesson"));
    let path = dir.join(&file);
    std::fs::write(&path, lesson_markdown(r, n))?;
    let index = dir.join("INDEX.md");
    if index.exists() {
        let mut idx = std::fs::read_to_string(&index)?;
        if !idx.ends_with('\n') {
            idx.push('\n');
        }
        idx.push_str(&format!("| {n:04} | [{}]({file}) | 候選 | （待補） |\n", r.title));
        std::fs::write(&index, idx)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grade::EvidenceGrade;

    fn rec() -> SessionRecord {
        SessionRecord {
            id: "s-1".into(),
            spec: SessionSpec::new("p", Mode::Debate, vec![Participant::new("a"), Participant::new("b")], "用 Postgres 還是 SQLite？"),
            title: "Postgres vs SQLite".into(),
            status: Status::Done,
            created_at: 1_794_787_200_000,
            updated_at: 0,
            turns: vec![],
            notes: vec![],
            interjections: vec![],
            verify_runs: vec![],
            verdict: Some(Verdict {
                outcome: "judged".into(),
                headline: "勝方：正方".into(),
                grade: EvidenceGrade::Inferred,
                summary: "採用 Postgres".into(),
                open_issues: vec!["負載測試".into()],
                memory_hint: None,
            }),
            error: None,
            plan: None,
            workspace: None,
            phase: None,
        }
    }

    #[test]
    fn slug_and_numbering() {
        assert_eq!(slug("Postgres vs SQLite", "x"), "postgres-vs-sqlite");
        assert_eq!(slug("資料庫選型", "decision"), "decision");
        let d = std::env::temp_dir().join(format!("agora-adr-{}", random_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("0007-old.md"), "").unwrap();
        assert_eq!(next_number(&d), 8);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn writes_adr_and_lesson_with_index_row() {
        let d = std::env::temp_dir().join(format!("agora-exp-{}", random_hex(4)));
        std::fs::create_dir_all(d.join("docs/lessons")).unwrap();
        std::fs::write(d.join("docs/lessons/INDEX.md"), "| # | Title |\n|---|---|\n").unwrap();
        let r = rec();
        let adr = write_adr(&d, &r).unwrap();
        let text = std::fs::read_to_string(&adr).unwrap();
        assert!(adr.ends_with("0001-postgres-vs-sqlite.md"));
        assert!(text.contains("# ADR-0001: Postgres vs SQLite") && text.contains("推論") && text.contains("待驗證：負載測試"));
        let lesson = write_lesson(&d, &r).unwrap();
        assert!(lesson.exists());
        assert!(std::fs::read_to_string(d.join("docs/lessons/INDEX.md")).unwrap().contains("Postgres vs SQLite"));
        assert!(session_markdown(&r, None).contains("## 結論（證據等級：推論）"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
