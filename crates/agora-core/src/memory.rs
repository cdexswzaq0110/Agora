//! 檢索式專案記憶 — when project memory outgrows its budget, inject the entries most relevant
//! to the current question (TF-IDF over word tokens + CJK character bigrams) plus the most
//! recent ones, in their original order, with a visible note of what was left out.
//!
//! Same idea as Agent Kernel's `context.py::_tfidf_relevance`, extended for Chinese text.

use std::collections::{HashMap, HashSet};

use crate::compact::char_len;

/// Always keep this many most-recent entries.
pub const KEEP_RECENT: usize = 5;

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// Lowercased ASCII words (len ≥ 2) and CJK character bigrams.
pub fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut prev_cjk: Option<char> = None;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c.to_ascii_lowercase());
            prev_cjk = None;
            continue;
        }
        if word.len() >= 2 {
            out.push(std::mem::take(&mut word));
        } else {
            word.clear();
        }
        if is_cjk(c) {
            if let Some(p) = prev_cjk {
                out.push(format!("{p}{c}"));
            }
            prev_cjk = Some(c);
        } else {
            prev_cjk = None;
        }
    }
    if word.len() >= 2 {
        out.push(word);
    }
    out
}

/// Split memory into entries: a bullet/heading line starts an entry; other lines continue it.
pub fn entries(memory: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in memory.lines() {
        let t = line.trim_start();
        if t.is_empty() {
            continue;
        }
        let starts = t.starts_with("- ") || t.starts_with("* ") || t.starts_with('#') || out.is_empty();
        if starts {
            out.push(line.trim_end().to_string());
        } else if let Some(last) = out.last_mut() {
            last.push('\n');
            last.push_str(line.trim_end());
        }
    }
    out
}

/// TF-IDF relevance of each entry to `query`.
pub fn scores(query: &str, docs: &[String]) -> Vec<f64> {
    let q: HashSet<String> = tokens(query).into_iter().collect();
    let doc_tokens: Vec<Vec<String>> = docs.iter().map(|d| tokens(d)).collect();
    let n = docs.len().max(1) as f64;
    let mut df: HashMap<&str, usize> = HashMap::new();
    for toks in &doc_tokens {
        for t in toks.iter().collect::<HashSet<_>>() {
            *df.entry(t.as_str()).or_default() += 1;
        }
    }
    doc_tokens
        .iter()
        .map(|toks| {
            if toks.is_empty() {
                return 0.0;
            }
            let mut tf: HashMap<&str, usize> = HashMap::new();
            for t in toks {
                *tf.entry(t.as_str()).or_default() += 1;
            }
            q.iter()
                .filter_map(|t| {
                    let f = *tf.get(t.as_str())? as f64 / toks.len() as f64;
                    let idf = (n / (*df.get(t.as_str()).unwrap_or(&1) as f64)).ln() + 1.0;
                    Some(f * idf)
                })
                .sum()
        })
        .collect()
}

/// Memory text to inject for `query`, within `budget` characters.
pub fn select(memory: &str, query: &str, budget: usize) -> String {
    let m = memory.trim();
    if char_len(m) <= budget {
        return m.to_string();
    }
    let es = entries(m);
    let n = es.len();
    let s = scores(query, &es);
    let mut keep: HashSet<usize> = (n.saturating_sub(KEEP_RECENT)..n).collect();
    let mut used: usize = keep.iter().map(|&i| char_len(&es[i]) + 1).sum();
    let mut order: Vec<usize> = (0..n).filter(|i| !keep.contains(i)).collect();
    order.sort_by(|a, b| s[*b].partial_cmp(&s[*a]).unwrap_or(std::cmp::Ordering::Equal).then(b.cmp(a)));
    for i in order {
        if s[i] <= 0.0 {
            break;
        }
        let cost = char_len(&es[i]) + 1;
        if used + cost > budget {
            continue;
        }
        used += cost;
        keep.insert(i);
    }
    let mut idx: Vec<usize> = keep.into_iter().collect();
    idx.sort_unstable();
    let body: Vec<&str> = idx.iter().map(|&i| es[i].as_str()).collect();
    format!(
        "〔檢索式記憶：專案記憶共 {n} 則，依本議題選出最相關與最近的 {} 則；其餘仍保存在專案記憶中〕\n{}",
        idx.len(),
        body.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_words_and_cjk_bigrams() {
        let t = tokens("用 Rust 寫快取");
        assert!(t.contains(&"rust".to_string()));
        assert!(t.contains(&"快取".to_string()));
    }

    #[test]
    fn selects_relevant_and_recent_entries_within_budget() {
        let mut mem = String::new();
        for i in 0..40 {
            mem.push_str(&format!("- [2026-01-{:02}] 無關的決策第 {i} 號，談的是行銷頁面配色與字型\n", i % 28 + 1));
        }
        mem.insert_str(0, "- [2025-12-01] 資料庫決定採用 PostgreSQL，因為需要交易與 JSONB\n");
        let out = select(&mem, "這個服務的資料庫要用什麼？PostgreSQL 還是 SQLite", 600);
        assert!(out.contains("PostgreSQL，因為需要交易"), "relevant old entry kept:\n{out}");
        assert!(out.contains("第 39 號"), "most recent entry kept");
        assert!(out.contains("檢索式記憶"));
        assert!(char_len(&out) <= 700);
    }

    #[test]
    fn short_memory_is_returned_verbatim() {
        assert_eq!(select("- a\n- b", "x", 100), "- a\n- b");
    }
}
