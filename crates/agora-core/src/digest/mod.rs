//! 每日技術情報 (daily tech digest).
//!
//! Every day Agora reads the *listing* pages of a few public sources — titles, scores, short
//! descriptions; never full articles — ranks and de-duplicates them, and asks one CLI model to
//! write a Traditional-Chinese digest that cites every claim with `[n]`. If no model is
//! available (or the quota guard says no), a deterministic digest is produced instead, so the
//! day is never empty.
//!
//! Politeness/privacy: a generic User-Agent, one request per source (Reddit: one per
//! subreddit, spaced out), short timeouts, nothing personal in any request.

pub mod rank;
pub mod sources;
pub mod trend;

use serde::{Deserialize, Serialize};

use crate::compact::truncate_chars;
use crate::model::{Effort, TokenUsage};

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct DigestItem {
    /// Display number used for `[n]` citations (assigned after ranking).
    #[serde(default)]
    pub n: u32,
    pub source: String,
    pub source_label: String,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub discussion_url: Option<String>,
    #[serde(default)]
    pub score: Option<u64>,
    #[serde(default)]
    pub comments: Option<u64>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub stars_today: Option<u64>,
    /// Other sources where the same link was trending (cross-forum attention).
    #[serde(default)]
    pub also_on: Vec<String>,
    #[serde(default)]
    pub matched_keywords: Vec<String>,
    #[serde(default)]
    pub rank: f64,
    /// 和昨天相比 — see `trend::annotate`. `None` when there is no earlier digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trend: Option<trend::Trend>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SourceStatus {
    pub source: String,
    pub label: String,
    pub ok: bool,
    pub count: usize,
    pub error: Option<String>,
    pub ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Digest {
    /// Local date `YYYY-MM-DD`.
    pub date: String,
    pub generated_at: u64,
    pub items: Vec<DigestItem>,
    pub markdown: String,
    pub llm_ok: bool,
    pub llm_error: Option<String>,
    /// e.g. `codex · gpt-5.6-sol`
    pub summarizer: Option<String>,
    pub usage: Option<TokenUsage>,
    pub sources: Vec<SourceStatus>,
    /// Date of the digest this one was compared with (`None` = no comparison).
    #[serde(default)]
    pub compared_with: Option<String>,
    /// Keyword hit counts, today vs the compared digest.
    #[serde(default)]
    pub topic_trends: Vec<trend::TopicTrend>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DigestSettings {
    pub enabled: bool,
    /// Local time `HH:MM`.
    pub time: String,
    pub sources: Vec<String>,
    pub subreddits: Vec<String>,
    /// Topics to boost and to recommend reading on.
    pub keywords: Vec<String>,
    /// Provider id for the summary; `None` = first ready CLI.
    pub provider: Option<String>,
    pub model: Option<String>,
    pub max_items: usize,
    pub use_llm: bool,
    /// Reasoning effort for the summarizing CLI call; `None` = the model's default.
    #[serde(default)]
    pub effort: Option<Effort>,
}

impl Default for DigestSettings {
    fn default() -> Self {
        DigestSettings {
            enabled: true,
            time: "08:00".into(),
            sources: sources::ALL.iter().map(|s| s.to_string()).collect(),
            subreddits: ["programming", "ExperiencedDevs", "rust", "MachineLearning", "LocalLLaMA", "webdev"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            keywords: ["agent", "llm", "mcp", "claude", "codex", "rust", "tauri", "python", "system design", "database"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            provider: None,
            model: None,
            max_items: 36,
            use_llm: true,
            effort: None,
        }
    }
}

impl DigestSettings {
    /// `(hour, minute)` from `time`, defaulting to 08:00 on bad input.
    pub fn hm(&self) -> (u32, u32) {
        let mut it = self.time.split(':');
        let h = it.next().and_then(|x| x.trim().parse().ok()).filter(|h: &u32| *h < 24).unwrap_or(8);
        let m = it.next().and_then(|x| x.trim().parse().ok()).filter(|m: &u32| *m < 60).unwrap_or(0);
        (h, m)
    }
}

/// `【新上榜】`, `【升溫 +40 分】`, `【連續 3 天】` — facts the model may cite, never infer.
fn trend_tags(i: &DigestItem) -> String {
    let Some(t) = &i.trend else {
        return String::new();
    };
    let mut tags = Vec::new();
    match t.kind {
        trend::TrendKind::New => tags.push("【新上榜】".to_string()),
        trend::TrendKind::Rising => {
            let unit = match t.metric.as_deref() {
                Some("stars_today") => " stars",
                Some("comments") => " 則留言",
                _ => " 分",
            };
            tags.push(format!("【升溫 +{}{unit}】", t.delta.unwrap_or(0)));
        }
        _ => {}
    }
    if t.streak >= 2 {
        tags.push(format!("【連續 {} 天】", t.streak));
    }
    tags.concat()
}

fn item_line(i: &DigestItem) -> String {
    let mut meta = Vec::new();
    if let Some(s) = i.score {
        meta.push(format!("{s} 分"));
    }
    if let Some(s) = i.stars_today {
        meta.push(format!("今日 +{s} stars"));
    }
    if let Some(c) = i.comments {
        meta.push(format!("{c} 則留言"));
    }
    if let Some(l) = &i.language {
        meta.push(l.clone());
    }
    if !i.tags.is_empty() {
        meta.push(format!("標籤：{}", i.tags.iter().take(5).cloned().collect::<Vec<_>>().join(", ")));
    }
    if !i.also_on.is_empty() {
        meta.push(format!("也出現在 {}", i.also_on.join("、")));
    }
    let mut s = format!("[{}] （{}）{}{}", i.n, i.source_label, trend_tags(i), i.title);
    if !meta.is_empty() {
        s.push_str(&format!(" — {}", meta.join(" · ")));
    }
    if let Some(sum) = i.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        s.push_str(&format!("\n    簡介：{}", truncate_chars(sum.trim(), 260)));
    }
    s
}

/// The single prompt that turns ranked items into the digest.
pub fn llm_prompt(items: &[DigestItem], keywords: &[String], date: &str, compare: Option<(&str, &[trend::TopicTrend])>) -> String {
    let list: Vec<String> = items.iter().map(item_line).collect();
    let compare_section = match compare {
        None => String::new(),
        Some((prev, topics)) => {
            let moved: Vec<String> = topics.iter().map(|t| format!("{} {}→{}", t.keyword, t.previous, t.today)).collect();
            format!(
                "## 和昨天相比\n（對照 {prev} 的情報，2–4 點：根據項目上的【新上榜】【升溫】【連續 N 天】標記，說明今天多了什麼、什麼持續升溫；{}。沒有標記就不要宣稱「新」或「升溫」）\n",
                if moved.is_empty() { "關注主題的出現次數沒有變化".to_string() } else { format!("關注主題出現次數（前一份→今天）：{}", moved.join("、")) }
            )
        }
    };
    format!(
        "你是 Agora 的每日技術情報編輯。以下是 {date} 從 Hacker News、Lobsters、Reddit、GitHub Trending、dev.to、Hugging Face Papers 擷取的熱門項目。\
         你拿到的只有標題、熱度與簡短介紹，**沒有全文**；不需要讀檔，也不要上網查詢。\n\n\
         請用繁體中文，為一位軟體工程師／AI 工程師寫一份 5 分鐘能讀完的摘要，依序輸出：\n\
         ## 今日三大重點\n（每點 2–3 句：發生了什麼、為什麼重要）\n\
         {compare_section}\
         ## 技術趨勢觀察\n（3–5 點：跨來源反覆出現的主題，說明是哪些項目共同指向它）\n\
         ## GitHub 熱門專案\n（每個一行：名稱 — 它做什麼；為什麼現在受關注）\n\
         ## 論壇熱議\n（每則一行：主題 — 大家在爭論或關心的點）\n\
         ## AI 與論文\n（若有）\n\
         ## 建議深入閱讀\n（3 則，優先挑和讀者關注的主題相關的：{}）\n\
         ## 名詞小辭典\n（今天出現、值得認識的新名詞或專案，每個一句解釋）\n\n\
         規則：\n- 每個陳述後面用 [編號] 標出依據的項目，例如 [3][12]。\n- 只根據下面提供的資料；從標題推測的內容要標註【推論】。\n- 不要編造數字、版本號或引言。\n\n\
         ## 今日項目\n{}\n",
        if keywords.is_empty() { "（未設定）".to_string() } else { keywords.join("、") },
        list.join("\n")
    )
}

/// Deterministic digest when no model is available.
pub fn fallback_markdown(items: &[DigestItem], reason: Option<&str>, compare: Option<&str>) -> String {
    let mut s = String::new();
    if let Some(r) = reason {
        s.push_str(&format!("> 這份是未經模型整理的清單：{r}\n\n"));
    }
    if let Some(c) = compare {
        s.push_str(c);
    }
    let mut groups: Vec<(String, Vec<&DigestItem>)> = Vec::new();
    for i in items {
        match groups.iter_mut().find(|(l, _)| *l == i.source_label) {
            Some((_, v)) => v.push(i),
            None => groups.push((i.source_label.clone(), vec![i])),
        }
    }
    for (label, list) in groups {
        s.push_str(&format!("## {label}\n"));
        for i in list {
            let mut meta = Vec::new();
            if let Some(x) = i.score {
                meta.push(format!("{x} 分"));
            }
            if let Some(x) = i.stars_today {
                meta.push(format!("今日 +{x} stars"));
            }
            if let Some(x) = i.comments {
                meta.push(format!("{x} 留言"));
            }
            s.push_str(&format!("- [{}] [{}]({}){}", i.n, i.title, i.url, if meta.is_empty() { String::new() } else { format!(" — {}", meta.join(" · ")) }));
            if let Some(sum) = i.summary.as_deref().filter(|x| !x.trim().is_empty()) {
                s.push_str(&format!("：{}", truncate_chars(sum.trim(), 140)));
            }
            s.push('\n');
        }
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_numbers_items_and_forbids_invention() {
        let items = vec![DigestItem {
            n: 1,
            source: "hn".into(),
            source_label: "Hacker News".into(),
            title: "Show HN: X".into(),
            url: "https://x.dev".into(),
            score: Some(321),
            ..Default::default()
        }];
        let p = llm_prompt(&items, &["rust".into()], "2026-11-16", None);
        assert!(p.contains("[1] （Hacker News）Show HN: X — 321 分"));
        assert!(p.contains("不要編造") && p.contains("rust"));
        assert!(!p.contains("和昨天相比"), "no earlier digest → no comparison section");
        let mut tagged = items.clone();
        tagged[0].trend = Some(trend::Trend { kind: trend::TrendKind::Rising, streak: 3, metric: Some("score".into()), delta: Some(40), ..Default::default() });
        let topics = [trend::TopicTrend { keyword: "rust".into(), today: 2, previous: 1 }];
        let p = llm_prompt(&tagged, &[], "2026-11-16", Some(("2026-11-15", &topics)));
        assert!(p.contains("[1] （Hacker News）【升溫 +40 分】【連續 3 天】Show HN: X"), "{p}");
        assert!(p.contains("## 和昨天相比") && p.contains("對照 2026-11-15") && p.contains("rust 1→2"));
        let f = fallback_markdown(&items, Some("沒有可用的模型"), None);
        assert!(f.contains("## Hacker News") && f.contains("[Show HN: X](https://x.dev)"));
    }

    #[test]
    fn time_parsing_is_forgiving() {
        let mut s = DigestSettings::default();
        assert_eq!(s.hm(), (8, 0));
        s.time = "21:35".into();
        assert_eq!(s.hm(), (21, 35));
        s.time = "nonsense".into();
        assert_eq!(s.hm(), (8, 0));
    }
}
