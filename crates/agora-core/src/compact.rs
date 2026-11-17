//! 記憶壓縮 — transcript compaction.
//!
//! Every turn's prompt carries the discussion so far. When that exceeds a character budget,
//! the most recent turns stay (nearly) verbatim and older turns are reduced to an extractive
//! summary (headings, conclusions, bullet points first). Nothing is silently lost: every
//! compressed turn carries a visible marker, the engine records a `Note`, and the full text
//! stays in the session record.

#[derive(Clone, Debug)]
pub struct TranscriptEntry {
    pub index: u32,
    pub header: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompressionRecord {
    pub index: u32,
    pub original_chars: usize,
    pub kept_chars: usize,
}

#[derive(Clone, Debug)]
pub struct Compacted {
    pub text: String,
    pub compressed: Vec<CompressionRecord>,
    /// Oldest entries omitted entirely (only when even summaries would not fit).
    pub dropped: usize,
}

impl Compacted {
    pub fn was_compacted(&self) -> bool {
        !self.compressed.is_empty() || self.dropped > 0
    }
}

pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

pub fn truncate_chars(s: &str, max: usize) -> String {
    if char_len(s) <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Keep the head and tail of a long text with a visible gap marker.
pub fn head_tail(s: &str, max: usize) -> String {
    let n = char_len(s);
    if n <= max {
        return s.to_string();
    }
    let head_n = max * 2 / 3;
    let tail_n = max.saturating_sub(head_n);
    let head: String = s.chars().take(head_n).collect();
    let tail: String = s.chars().skip(n - tail_n).collect();
    format!("{head}\n〔…中略 {} 字…〕\n{tail}", n - head_n - tail_n)
}

const KEY_TERMS: &[&str] = &[
    "結論", "建議", "共識", "分歧", "立場", "風險", "摘要", "勝方", "證據等級", "裁決", "VERDICT",
    "決定", "問題", "修訂", "交付",
];

fn line_score(line: &str, is_first: bool) -> u8 {
    let t = line.trim_start();
    if t.starts_with('#') || KEY_TERMS.iter().any(|k| t.contains(k)) {
        3
    } else if t.starts_with("- ")
        || t.starts_with("* ")
        || t.starts_with("• ")
        || t.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    {
        2
    } else if is_first {
        2
    } else {
        0
    }
}

/// Extractive summary: pick the highest-signal lines (by score, then position) until the
/// budget is spent, then emit them in their original order.
pub fn summarize_extractive(text: &str, max_chars: usize) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(line_score(lines[i], i == 0)), i));
    let mut chosen = Vec::new();
    let mut used = 0usize;
    for i in order {
        let l = truncate_chars(lines[i], 200);
        let cost = char_len(&l) + 1;
        if used + cost > max_chars {
            if chosen.is_empty() {
                chosen.push((i, truncate_chars(&l, max_chars.max(8))));
            }
            continue;
        }
        used += cost;
        chosen.push((i, l));
    }
    chosen.sort_by_key(|(i, _)| *i);
    chosen.into_iter().map(|(_, l)| l).collect::<Vec<_>>().join("\n")
}

fn block(header: &str, body: &str) -> String {
    format!("### {header}\n{body}\n")
}

/// Render the transcript within `budget_chars`, keeping the last `keep_recent` entries
/// (nearly) verbatim.
pub fn render(entries: &[TranscriptEntry], budget_chars: usize, keep_recent: usize) -> Compacted {
    let full: String = entries.iter().map(|e| block(&e.header, &e.text)).collect::<Vec<_>>().join("\n");
    if char_len(&full) <= budget_chars || entries.is_empty() {
        return Compacted { text: full, compressed: vec![], dropped: 0 };
    }

    let keep_recent = keep_recent.clamp(1, entries.len());
    let split = entries.len() - keep_recent;
    let (older, recent) = entries.split_at(split);
    let mut compressed = Vec::new();

    let recent_budget = if older.is_empty() { budget_chars } else { budget_chars * 65 / 100 };
    let per_recent = (recent_budget / recent.len()).max(200);
    let recent_blocks: Vec<String> = recent
        .iter()
        .map(|e| {
            let n = char_len(&e.text);
            if n > per_recent {
                let kept = head_tail(&e.text, per_recent);
                compressed.push(CompressionRecord { index: e.index, original_chars: n, kept_chars: char_len(&kept) });
                block(&e.header, &kept)
            } else {
                block(&e.header, &e.text)
            }
        })
        .collect();
    let used_recent: usize = recent_blocks.iter().map(|b| char_len(b)).sum();

    let older_budget = budget_chars.saturating_sub(used_recent);
    const MIN_PER_OLDER: usize = 160;
    let fit = (older_budget / MIN_PER_OLDER).min(older.len());
    let dropped = older.len() - fit;
    let kept_older = &older[dropped..];
    let mut older_blocks = Vec::new();
    if dropped > 0 {
        older_blocks.push(format!("〔記憶壓縮：省略最早的 {dropped} 則發言；全文保留在專案紀錄〕\n"));
    }
    if !kept_older.is_empty() {
        let per_older = (older_budget / kept_older.len()).max(MIN_PER_OLDER) - 60;
        for e in kept_older {
            let n = char_len(&e.text);
            if n <= per_older {
                older_blocks.push(block(&e.header, &e.text));
                continue;
            }
            let summary = summarize_extractive(&e.text, per_older);
            let kept = char_len(&summary);
            compressed.push(CompressionRecord { index: e.index, original_chars: n, kept_chars: kept });
            older_blocks.push(block(
                &e.header,
                &format!("{summary}\n〔記憶壓縮：原文 {n} 字 → 摘要 {kept} 字；全文保留在專案紀錄〕"),
            ));
        }
    }
    compressed.sort_by_key(|c| c.index);
    let text = older_blocks.into_iter().chain(recent_blocks).collect::<Vec<_>>().join("\n");
    Compacted { text, compressed, dropped }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(i: u32, text: String) -> TranscriptEntry {
        TranscriptEntry { index: i, header: format!("第 {i} 則"), text }
    }

    #[test]
    fn short_transcript_is_verbatim() {
        let e = vec![entry(0, "甲".into()), entry(1, "乙".into())];
        let c = render(&e, 1000, 2);
        assert!(!c.was_compacted());
        assert!(c.text.contains("甲") && c.text.contains("乙"));
    }

    #[test]
    fn long_transcript_fits_budget_and_marks_compression() {
        let long = |tag: &str| {
            let mut s = format!("# {tag} 的立場\n");
            for k in 0..80 {
                s.push_str(&format!("這是第 {k} 行的冗長細節，用來撐大篇幅。\n"));
            }
            s.push_str("- 結論：採用方案 A\n");
            s
        };
        let e: Vec<_> = (0..6).map(|i| entry(i, long(&format!("發言{i}")))).collect();
        let budget = 4000;
        let c = render(&e, budget, 2);
        assert!(c.was_compacted());
        assert!(char_len(&c.text) <= budget + 400, "len {}", char_len(&c.text));
        assert!(c.text.contains("記憶壓縮"));
        // Older summaries keep the high-signal lines.
        assert!(c.text.contains("結論：採用方案 A"));
        assert!(c.text.contains("發言0") || c.dropped > 0);
    }

    #[test]
    fn extractive_summary_prefers_headings_and_conclusions() {
        let t = "開場白很長很長\n無關細節一\n無關細節二\n## 建議\n- 先做 X\n結論：選 X";
        let s = summarize_extractive(t, 30);
        assert!(s.contains("結論：選 X"));
        assert!(s.contains("## 建議"));
        assert!(!s.contains("無關細節二"));
    }

    #[test]
    fn head_tail_is_char_boundary_safe() {
        let s = "中文".repeat(100);
        let out = head_tail(&s, 50);
        assert!(out.contains("中略"));
    }
}
