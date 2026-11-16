//! 和昨天相比 — cross-day annotations for a digest, computed only from digests already stored.
//!
//! Rules (docs/design/2026-11-16-v0.4/05-final-plan.md, B + red-team revisions):
//! - Compare only with digests dated strictly before today (a same-day regeneration never
//!   compares with itself); look back at most `WINDOW_DAYS`.
//! - Match by canonical URL first, then by source + normalized title.
//! - `new` ("新上榜") only when the previous digest fetched that source successfully; otherwise
//!   the item's absence proves nothing → `unknown` (no badge).
//! - `rising` only within the same source and the same metric, with delta ≥ 30% and ≥ +10.
//! - `streak` counts calendar days: today, yesterday, … each must have a stored digest that
//!   contains the item; one missing day ends the streak.

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use super::rank::{canonical, norm_title};
use super::{Digest, DigestItem};

pub const WINDOW_DAYS: usize = 7;
const RISING_PCT: u64 = 30;
const RISING_MIN: u64 = 10;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrendKind {
    New,
    Continuing,
    Rising,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Trend {
    pub kind: TrendKind,
    /// Earliest date within the look-back window on which the item appeared (or today).
    pub first_seen_in_window: String,
    /// Consecutive calendar days ending today on which the item was listed (today = 1).
    pub streak: u32,
    /// `stars_today` / `score` / `comments` — the field compared for `rising`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_metric: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TopicTrend {
    pub keyword: String,
    pub today: u32,
    pub previous: u32,
}

fn same_item(a: &DigestItem, b: &DigestItem) -> bool {
    let (ca, cb) = (canonical(&a.url), canonical(&b.url));
    if !ca.is_empty() && ca == cb {
        return true;
    }
    a.source == b.source && !norm_title(&a.title).is_empty() && norm_title(&a.title) == norm_title(&b.title)
}

fn find<'a>(d: &'a Digest, item: &DigestItem) -> Option<&'a DigestItem> {
    d.items.iter().find(|x| same_item(x, item))
}

/// The first metric both sides report, in order of how comparable it is within a source.
fn metric(today: &DigestItem, prev: &DigestItem) -> Option<(&'static str, u64, u64)> {
    [
        ("stars_today", today.stars_today, prev.stars_today),
        ("score", today.score, prev.score),
        ("comments", today.comments, prev.comments),
    ]
    .into_iter()
    .find_map(|(name, t, p)| Some((name, t?, p?)))
}

fn parse(date: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

/// Annotate `today` in place from `history` (any order; only earlier dates are used).
/// With no earlier digest every trend is cleared and `compared_with` is `None`.
pub fn annotate(today: &mut Digest, history: &[Digest], keywords: &[String]) {
    let Some(day) = parse(&today.date) else {
        return;
    };
    let mut earlier: Vec<&Digest> = history.iter().filter(|d| parse(&d.date).is_some_and(|x| x < day)).collect();
    earlier.sort_by(|a, b| b.date.cmp(&a.date));
    earlier.truncate(WINDOW_DAYS);
    let Some(prev) = earlier.first().copied() else {
        today.compared_with = None;
        today.topic_trends.clear();
        for i in today.items.iter_mut() {
            i.trend = None;
        }
        return;
    };
    today.compared_with = Some(prev.date.clone());
    let by_date = |d: NaiveDate| earlier.iter().copied().find(|x| parse(&x.date) == Some(d));

    for item in today.items.iter_mut() {
        let mut streak = 1;
        let mut cursor = day;
        loop {
            cursor -= Duration::days(1);
            match by_date(cursor) {
                Some(d) if find(d, item).is_some() => streak += 1,
                _ => break,
            }
        }
        let first_seen = earlier.iter().rev().find(|d| find(d, item).is_some()).map(|d| d.date.clone()).unwrap_or_else(|| today.date.clone());
        let mut t = Trend { kind: TrendKind::Unknown, first_seen_in_window: first_seen, streak, ..Default::default() };
        match find(prev, item) {
            Some(p) => {
                t.kind = TrendKind::Continuing;
                if p.source == item.source {
                    if let Some((name, now, before)) = metric(item, p) {
                        let delta = now as i64 - before as i64;
                        t.metric = Some(name.into());
                        t.previous_metric = Some(before);
                        t.delta = Some(delta);
                        if now >= before + RISING_MIN && (now - before) * 100 >= before * RISING_PCT {
                            t.kind = TrendKind::Rising;
                        }
                    }
                }
            }
            None => {
                let fetched = prev.sources.iter().any(|s| s.source == item.source && s.ok);
                t.kind = if fetched { TrendKind::New } else { TrendKind::Unknown };
            }
        }
        item.trend = Some(t);
    }

    let count = |d: &Digest, k: &str| d.items.iter().filter(|i| i.matched_keywords.iter().any(|m| m.eq_ignore_ascii_case(k))).count() as u32;
    today.topic_trends = keywords
        .iter()
        .map(|k| TopicTrend { keyword: k.clone(), today: count(today, k), previous: count(prev, k) })
        .filter(|t| t.today > 0 || t.previous > 0)
        .collect();
}

/// "## 和昨天相比" for the no-model digest (the model gets the same facts in its prompt).
pub fn summary_markdown(d: &Digest) -> Option<String> {
    let prev = d.compared_with.as_ref()?;
    let pick = |k: TrendKind| d.items.iter().filter(move |i| i.trend.as_ref().is_some_and(|t| t.kind == k));
    let mut s = format!("## 和昨天相比\n\n> 對照 {prev} 的情報。\n\n");
    let new: Vec<_> = pick(TrendKind::New).collect();
    let rising: Vec<_> = pick(TrendKind::Rising).collect();
    let long: Vec<_> = d.items.iter().filter(|i| i.trend.as_ref().is_some_and(|t| t.streak >= 3)).collect();
    if !new.is_empty() {
        s.push_str(&format!("- **新上榜 {} 則**：{}\n", new.len(), new.iter().take(6).map(|i| format!("{} [{}]", i.title, i.n)).collect::<Vec<_>>().join("、")));
    }
    if !rising.is_empty() {
        s.push_str(&format!("- **升溫 {} 則**：{}\n", rising.len(), rising.iter().take(6).map(|i| format!("{} [{}]", i.title, i.n)).collect::<Vec<_>>().join("、")));
    }
    if !long.is_empty() {
        s.push_str(&format!("- **連續 3 天以上**：{}\n", long.iter().take(6).map(|i| format!("{} [{}]", i.title, i.n)).collect::<Vec<_>>().join("、")));
    }
    let moved: Vec<String> = d.topic_trends.iter().filter(|t| t.today != t.previous).map(|t| format!("{} {}→{}", t.keyword, t.previous, t.today)).collect();
    if !moved.is_empty() {
        s.push_str(&format!("- **關注主題**：{}\n", moved.join("、")));
    }
    if new.is_empty() && rising.is_empty() && long.is_empty() && moved.is_empty() {
        s.push_str("- 和前一份差不多：沒有新上榜或明顯升溫的項目。\n");
    }
    s.push('\n');
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::SourceStatus;

    fn item(source: &str, title: &str, url: &str, score: Option<u64>) -> DigestItem {
        DigestItem { source: source.into(), source_label: source.into(), title: title.into(), url: url.into(), score, ..Default::default() }
    }

    fn digest(date: &str, items: Vec<DigestItem>, ok_sources: &[&str]) -> Digest {
        Digest {
            date: date.into(),
            items,
            sources: ok_sources.iter().map(|s| SourceStatus { source: s.to_string(), label: s.to_string(), ok: true, ..Default::default() }).collect(),
            ..Default::default()
        }
    }

    fn kind(d: &Digest, title: &str) -> TrendKind {
        d.items.iter().find(|i| i.title == title).unwrap().trend.as_ref().unwrap().kind
    }

    #[test]
    fn new_continuing_rising_and_unknown() {
        let yesterday = digest(
            "2026-11-15",
            vec![
                item("hn", "Steady", "https://a.dev/x?utm_source=hn", Some(100)),
                item("hn", "Hot", "https://b.dev", Some(100)),
                item("hn", "Small bump", "https://c.dev", Some(5)),
                item("github", "Repo", "https://github.com/o/r", None),
            ],
            &["hn", "github"],
        );
        let mut today = digest(
            "2026-11-16",
            vec![
                item("hn", "Steady (renamed)", "https://www.a.dev/x/", Some(110)),
                item("hn", "Hot", "https://b.dev", Some(140)),
                item("hn", "Small bump", "https://c.dev", Some(12)),
                item("hn", "Brand new", "https://d.dev", Some(50)),
                item("reddit", "From a failed source", "https://e.dev", Some(50)),
                item("lobsters", "Hot", "https://b.dev", Some(999)),
            ],
            &[],
        );
        annotate(&mut today, &[yesterday], &[]);
        assert_eq!(today.compared_with.as_deref(), Some("2026-11-15"));
        assert_eq!(kind(&today, "Steady (renamed)"), TrendKind::Continuing, "URL match survives utm/www/trailing slash; +10% is not rising");
        assert_eq!(kind(&today, "Hot"), TrendKind::Rising, "+40 (40%) within the same source");
        assert_eq!(kind(&today, "Small bump"), TrendKind::Continuing, "+140% but only +7 → below the absolute floor");
        assert_eq!(kind(&today, "Brand new"), TrendKind::New);
        assert_eq!(kind(&today, "From a failed source"), TrendKind::Unknown, "source not fetched yesterday proves nothing");
        let cross = today.items.iter().find(|i| i.source == "lobsters").unwrap().trend.clone().unwrap();
        assert_eq!(cross.kind, TrendKind::Continuing, "same link from another source: never compare different units");
        assert_eq!(cross.metric, None);
        let hot = today.items.iter().find(|i| i.title == "Hot" && i.source == "hn").unwrap().trend.clone().unwrap();
        assert_eq!((hot.metric.as_deref(), hot.previous_metric, hot.delta), (Some("score"), Some(100), Some(40)));
    }

    #[test]
    fn title_fallback_streak_window_and_same_day_regeneration() {
        let x = || item("hn", "Rust 2030 roadmap!", "", Some(10));
        let d13 = digest("2026-11-13", vec![x()], &["hn"]);
        let d14 = digest("2026-11-14", vec![x()], &["hn"]);
        let d15 = digest("2026-11-15", vec![x()], &["hn"]);
        let earlier_today = digest("2026-11-16", vec![], &["hn"]);
        let mut today = digest("2026-11-16", vec![item("hn", "rust 2030 Roadmap", "", Some(11))], &["hn"]);
        annotate(&mut today, &[d15.clone(), d13.clone(), d14.clone(), earlier_today], &[]);
        let t = today.items[0].trend.clone().unwrap();
        assert_eq!(t.kind, TrendKind::Continuing, "matched by source + normalized title when there is no URL");
        assert_eq!(t.streak, 4, "13, 14, 15, 16");
        assert_eq!(t.first_seen_in_window, "2026-11-13");
        assert_eq!(today.compared_with.as_deref(), Some("2026-11-15"), "a same-day digest is never the comparison");

        // A missing calendar day ends the streak.
        let mut today = digest("2026-11-16", vec![x()], &["hn"]);
        annotate(&mut today, &[d15.clone(), d13], &[]);
        assert_eq!(today.items[0].trend.as_ref().unwrap().streak, 2);
        assert_eq!(today.items[0].trend.as_ref().unwrap().first_seen_in_window, "2026-11-13", "window still sees the 13th");

        // Nothing earlier → no comparison at all.
        let mut alone = digest("2026-11-16", vec![x()], &["hn"]);
        annotate(&mut alone, &[], &[]);
        assert!(alone.compared_with.is_none() && alone.items[0].trend.is_none());
        assert!(summary_markdown(&alone).is_none());
    }

    #[test]
    fn previous_digest_without_source_status_never_claims_new() {
        // Digests written before source statuses existed have `sources: []`.
        let old = digest("2026-11-15", vec![item("hn", "A", "https://a.dev", Some(1))], &[]);
        let mut today = digest("2026-11-16", vec![item("hn", "B", "https://b.dev", Some(1))], &["hn"]);
        annotate(&mut today, &[old], &[]);
        assert_eq!(today.items[0].trend.as_ref().unwrap().kind, TrendKind::Unknown);
    }

    #[test]
    fn topic_trends_and_summary() {
        let mut a = item("hn", "Agents everywhere", "https://a.dev", Some(10));
        a.matched_keywords = vec!["agent".into()];
        let yesterday = digest("2026-11-15", vec![a.clone()], &["hn"]);
        let mut b = item("hn", "MCP servers", "https://b.dev", Some(10));
        b.matched_keywords = vec!["mcp".into(), "agent".into()];
        b.n = 2;
        a.n = 1;
        let mut today = digest("2026-11-16", vec![a, b], &["hn"]);
        annotate(&mut today, &[yesterday], &["agent".into(), "mcp".into(), "rust".into()]);
        assert_eq!(
            today.topic_trends,
            vec![
                TopicTrend { keyword: "agent".into(), today: 2, previous: 1 },
                TopicTrend { keyword: "mcp".into(), today: 1, previous: 0 }
            ]
        );
        let md = summary_markdown(&today).unwrap();
        assert!(md.contains("## 和昨天相比") && md.contains("新上榜 1 則") && md.contains("MCP servers [2]") && md.contains("agent 1→2"), "{md}");
    }

    #[test]
    fn old_json_without_trend_fields_still_loads() {
        let d: Digest = serde_json::from_str(r#"{"date":"2026-11-10","generated_at":1,"items":[{"source":"hn","source_label":"HN","title":"t","url":"u"}],"markdown":"","llm_ok":false,"llm_error":null,"summarizer":null,"usage":null,"sources":[]}"#).unwrap();
        assert!(d.compared_with.is_none() && d.topic_trends.is_empty() && d.items[0].trend.is_none());
    }
}
