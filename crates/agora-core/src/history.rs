//! 用量歷史 — quota samples over time (`quota_history.jsonl`) and token usage aggregated from
//! session records, for the charts on the usage page.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use crate::engine::format_date;
use crate::grade::EvidenceGrade;
use crate::model::{Mode, SessionRecord};
use crate::quota::QuotaSnapshot;

/// Minimum spacing between stored samples of the same provider, unless a value moved.
pub const SAMPLE_EVERY_MS: u64 = 10 * 60_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct QuotaSample {
    pub t: u64,
    pub provider: String,
    /// window key → used percent
    pub used: BTreeMap<String, f64>,
}

impl QuotaSample {
    pub fn from_snapshot(q: &QuotaSnapshot) -> Self {
        QuotaSample {
            t: q.observed_at,
            provider: q.provider.clone(),
            used: q.windows.iter().map(|w| (w.key.clone(), (w.used_percent * 10.0).round() / 10.0)).collect(),
        }
    }
}

/// Whether `next` is worth storing after `prev`.
pub fn worth_storing(prev: Option<&QuotaSample>, next: &QuotaSample) -> bool {
    match prev {
        None => true,
        Some(p) => {
            next.t.saturating_sub(p.t) >= SAMPLE_EVERY_MS
                || p.used.iter().any(|(k, v)| next.used.get(k).is_some_and(|n| (n - v).abs() >= 1.0))
                || next.used.len() != p.used.len()
        }
    }
}

pub fn append(path: &Path, s: &QuotaSample) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(s).unwrap_or_default())
}

pub fn read_since(path: &Path, since_ms: u64) -> Vec<QuotaSample> {
    std::fs::read_to_string(path)
        .map(|raw| {
            raw.lines()
                .filter_map(|l| serde_json::from_str::<QuotaSample>(l).ok())
                .filter(|s| s.t >= since_ms)
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Debug, Serialize, Default, PartialEq)]
pub struct DayStat {
    pub date: String,
    pub sessions: u32,
    pub turns: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// provider id → input + output tokens that day
    pub by_provider: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize, Default, PartialEq)]
pub struct ModeStat {
    pub mode: String,
    pub sessions: u32,
    pub turns: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Sessions whose verdict reached 已確認.
    pub confirmed: u32,
    pub with_verdict: u32,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct UsageStats {
    pub by_day: Vec<DayStat>,
    pub by_mode: Vec<ModeStat>,
    pub by_provider: BTreeMap<String, (u32, u64, u64)>,
}

/// Aggregate CLI turns (seeded turns excluded) from sessions created since `since_ms`.
pub fn stats(sessions: &[SessionRecord], since_ms: u64) -> UsageStats {
    let mut days: BTreeMap<String, DayStat> = BTreeMap::new();
    let mut modes: BTreeMap<String, ModeStat> = BTreeMap::new();
    let mut prov: BTreeMap<String, (u32, u64, u64)> = BTreeMap::new();
    for r in sessions.iter().filter(|r| r.created_at >= since_ms) {
        let date = format_date(r.created_at);
        let mode = format!("{:?}", r.spec.mode).to_lowercase();
        let d = days.entry(date.clone()).or_insert_with(|| DayStat { date, ..Default::default() });
        let m = modes.entry(mode.clone()).or_insert_with(|| ModeStat { mode, ..Default::default() });
        d.sessions += 1;
        m.sessions += 1;
        if let Some(v) = &r.verdict {
            m.with_verdict += 1;
            if v.grade == EvidenceGrade::Confirmed {
                m.confirmed += 1;
            }
        }
        for t in r.turns.iter().filter(|t| !t.seeded && t.ended_at.is_some()) {
            d.turns += 1;
            m.turns += 1;
            let (i, o) = t.usage.as_ref().map_or((0, 0), |u| (u.input_tokens, u.output_tokens));
            d.input_tokens += i;
            d.output_tokens += o;
            *d.by_provider.entry(t.provider.clone()).or_default() += i + o;
            m.input_tokens += i;
            m.output_tokens += o;
            let p = prov.entry(t.provider.clone()).or_default();
            p.0 += 1;
            p.1 += i;
            p.2 += o;
        }
    }
    let order = |m: &str| Mode::all().iter().position(|x| format!("{x:?}").to_lowercase() == m).unwrap_or(99);
    let mut by_mode: Vec<ModeStat> = modes.into_values().collect();
    by_mode.sort_by_key(|m| order(&m.mode));
    UsageStats { by_day: days.into_values().collect(), by_mode, by_provider: prov }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: u64, v: f64) -> QuotaSample {
        QuotaSample { t, provider: "codex".into(), used: [("five_hour".to_string(), v)].into_iter().collect() }
    }

    #[test]
    fn samples_are_thinned_unless_values_move() {
        assert!(worth_storing(None, &s(0, 1.0)));
        assert!(!worth_storing(Some(&s(0, 1.0)), &s(60_000, 1.2)));
        assert!(worth_storing(Some(&s(0, 1.0)), &s(60_000, 3.0)));
        assert!(worth_storing(Some(&s(0, 1.0)), &s(SAMPLE_EVERY_MS, 1.0)));
    }

    #[test]
    fn history_roundtrip() {
        let p = std::env::temp_dir().join(format!("agora-hist-{}.jsonl", crate::model::random_hex(4)));
        append(&p, &s(10, 1.0)).unwrap();
        append(&p, &s(20, 2.0)).unwrap();
        assert_eq!(read_since(&p, 15).len(), 1);
        let _ = std::fs::remove_file(&p);
    }
}
