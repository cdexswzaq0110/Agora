//! Subscription quota ("剩餘用量") snapshots.
//!
//! Two sources, both free of charge:
//! - **Codex**: `codex app-server` JSON-RPC `account/rateLimits/read` → `rateLimits.primary`
//!   (5-hour window) and `.secondary` (weekly), each `{usedPercent, windowDurationMins, resetsAt}`.
//! - **Claude**: `claude -p --output-format stream-json` emits `rate_limit_event` lines whose
//!   `rate_limit_info.unifiedWindows.{five_hour,seven_day}` carry `{utilization (0–1), resetsAt}`.
//!   Captured passively from turns Agora already runs — no extra call.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::Family;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct QuotaWindow {
    /// Stable key: `five_hour`, `weekly`, `weekly_overage`, or `window_<minutes>`.
    pub key: String,
    pub label: String,
    /// 0–100 (may exceed 100 when usage runs past the window, per Claude's docs).
    pub used_percent: f64,
    /// Unix seconds.
    pub resets_at: Option<u64>,
    pub window_minutes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuotaStatus {
    Ok,
    Warning,
    Limited,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct QuotaSnapshot {
    pub provider: String,
    pub family: Family,
    pub plan: Option<String>,
    pub status: QuotaStatus,
    pub windows: Vec<QuotaWindow>,
    /// Human-readable origin of the numbers, shown in the UI.
    pub source: String,
    /// Unix milliseconds when the numbers were observed.
    pub observed_at: u64,
}

impl QuotaSnapshot {
    pub fn max_used(&self) -> Option<f64> {
        self.windows
            .iter()
            .map(|w| w.used_percent)
            .fold(None, |acc, x| Some(acc.map_or(x, |a: f64| a.max(x))))
    }

    pub fn remaining_percent(&self) -> Option<f64> {
        self.max_used().map(|u| (100.0 - u).clamp(0.0, 100.0))
    }

    /// Whether a new turn should be refused: the provider reported a hard limit, or a window
    /// is at/above `stop_percent` and has not reset yet (relative to `now_secs`).
    pub fn blocks(&self, stop_percent: f64, now_secs: u64) -> Option<String> {
        for w in &self.windows {
            let not_reset = w.resets_at.map_or(true, |r| r > now_secs);
            if not_reset && w.used_percent >= stop_percent {
                return Some(format!(
                    "{} {} 已用 {:.0}%（門檻 {:.0}%）",
                    self.family.label(),
                    w.label,
                    w.used_percent,
                    stop_percent
                ));
            }
        }
        if self.status == QuotaStatus::Limited {
            let all_reset = !self.windows.is_empty()
                && self
                    .windows
                    .iter()
                    .all(|w| w.resets_at.is_some_and(|r| r <= now_secs));
            if !all_reset {
                return Some(format!("{} 回報已達使用上限", self.family.label()));
            }
        }
        None
    }

    /// Fold a partial update into the previous snapshot: windows present in `newer` replace
    /// those with the same key; windows only in `self` are kept.
    pub fn merged_with(&self, newer: &QuotaSnapshot) -> QuotaSnapshot {
        let mut windows = self.windows.clone();
        for w in &newer.windows {
            match windows.iter_mut().find(|x| x.key == w.key) {
                Some(slot) => *slot = w.clone(),
                None => windows.push(w.clone()),
            }
        }
        windows.sort_by_key(|w| w.window_minutes.unwrap_or(u64::MAX));
        QuotaSnapshot {
            provider: newer.provider.clone(),
            family: newer.family,
            plan: newer.plan.clone().or_else(|| self.plan.clone()),
            status: newer.status,
            windows,
            source: newer.source.clone(),
            observed_at: newer.observed_at,
        }
    }
}

pub fn window_label(minutes: Option<u64>) -> String {
    match minutes {
        Some(300) => "5 小時".into(),
        Some(10080) => "每週".into(),
        Some(1440) => "每日".into(),
        Some(m) if m % 60 == 0 => format!("{} 小時", m / 60),
        Some(m) => format!("{m} 分鐘"),
        None => "時段".into(),
    }
}

fn status_from_percent(windows: &[QuotaWindow]) -> QuotaStatus {
    let max = windows.iter().map(|w| w.used_percent).fold(0.0_f64, f64::max);
    if windows.is_empty() {
        QuotaStatus::Unknown
    } else if max >= 100.0 {
        QuotaStatus::Limited
    } else if max >= 80.0 {
        QuotaStatus::Warning
    } else {
        QuotaStatus::Ok
    }
}

/// Map the `result` of Codex `account/rateLimits/read`.
pub fn from_codex_rate_limits(result: &Value, provider: &str, now_ms: u64) -> Option<QuotaSnapshot> {
    let rl = result.get("rateLimits").unwrap_or(result);
    let mut windows = Vec::new();
    for (slot, fallback_key) in [("primary", "five_hour"), ("secondary", "weekly")] {
        let Some(w) = rl.get(slot).filter(|w| !w.is_null()) else {
            continue;
        };
        let minutes = w.get("windowDurationMins").and_then(Value::as_u64);
        let key = match minutes {
            Some(300) => "five_hour".to_string(),
            Some(10080) => "weekly".to_string(),
            Some(m) => format!("window_{m}"),
            None => fallback_key.to_string(),
        };
        windows.push(QuotaWindow {
            key,
            label: window_label(minutes),
            used_percent: w.get("usedPercent").and_then(Value::as_f64).unwrap_or(0.0),
            resets_at: w.get("resetsAt").and_then(Value::as_u64),
            window_minutes: minutes,
        });
    }
    if windows.is_empty() {
        return None;
    }
    let mut status = status_from_percent(&windows);
    if rl.get("rateLimitReachedType").is_some_and(|v| !v.is_null()) {
        status = QuotaStatus::Limited;
    }
    Some(QuotaSnapshot {
        provider: provider.to_string(),
        family: Family::Gpt,
        plan: rl.get("planType").and_then(Value::as_str).map(str::to_string),
        status,
        windows,
        source: "codex app-server · account/rateLimits/read".into(),
        observed_at: now_ms,
    })
}

/// Map Claude Code's `rate_limit_event.rate_limit_info`.
pub fn from_claude_rate_limit_info(info: &Value, provider: &str, now_ms: u64) -> Option<QuotaSnapshot> {
    let mut windows = Vec::new();
    let push = |windows: &mut Vec<QuotaWindow>, key: &str, minutes: Option<u64>, label: String, w: &Value| {
        if let Some(util) = w.get("utilization").and_then(Value::as_f64) {
            windows.push(QuotaWindow {
                key: key.to_string(),
                label,
                used_percent: util * 100.0,
                resets_at: w.get("resetsAt").and_then(Value::as_u64),
                window_minutes: minutes,
            });
        }
    };
    if let Some(u) = info.get("unifiedWindows").filter(|u| u.is_object()) {
        if let Some(w) = u.get("five_hour") {
            push(&mut windows, "five_hour", Some(300), window_label(Some(300)), w);
        }
        if let Some(w) = u.get("seven_day") {
            push(&mut windows, "weekly", Some(10080), window_label(Some(10080)), w);
        }
        if let Some(w) = u.get("seven_day_overage_included") {
            push(&mut windows, "weekly_overage", Some(10080), "每週（含超額）".into(), w);
        }
    }
    if windows.is_empty() {
        // Older shape: a single window named by `rateLimitType`.
        let kind = info.get("rateLimitType").and_then(Value::as_str);
        let (key, minutes) = match kind {
            Some("five_hour") => ("five_hour", Some(300)),
            Some(k) if k.starts_with("seven_day") => ("weekly", Some(10080)),
            _ => ("window", None),
        };
        push(&mut windows, key, minutes, window_label(minutes), info);
    }
    let reported = info.get("status").and_then(Value::as_str);
    if windows.is_empty() && reported.is_none() {
        return None;
    }
    let mut status = status_from_percent(&windows);
    match reported {
        Some("rejected") => status = QuotaStatus::Limited,
        Some("allowed_warning") if status == QuotaStatus::Ok => status = QuotaStatus::Warning,
        Some("allowed") if windows.is_empty() => status = QuotaStatus::Ok,
        _ => {}
    }
    Some(QuotaSnapshot {
        provider: provider.to_string(),
        family: Family::Claude,
        plan: None,
        status,
        windows,
        source: "claude stream-json · rate_limit_event".into(),
        observed_at: now_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_codex_primary_and_secondary_windows() {
        // Shape observed from codex-cli 0.147 `account/rateLimits/read` (values changed).
        let v = json!({"rateLimits": {
            "primary": {"usedPercent": 12, "windowDurationMins": 300, "resetsAt": 1794781804u64},
            "secondary": {"usedPercent": 32, "windowDurationMins": 10080, "resetsAt": 1795222883u64},
            "planType": "plus", "limitId": "codex", "rateLimitReachedType": null
        }});
        let s = from_codex_rate_limits(&v, "codex", 1).unwrap();
        assert_eq!(s.windows.len(), 2);
        assert_eq!(s.windows[0].key, "five_hour");
        assert_eq!(s.windows[0].label, "5 小時");
        assert_eq!(s.windows[1].label, "每週");
        assert_eq!(s.plan.as_deref(), Some("plus"));
        assert_eq!(s.status, QuotaStatus::Ok);
        assert_eq!(s.remaining_percent(), Some(68.0));
    }

    #[test]
    fn codex_limit_reached_is_limited_even_below_100() {
        let v = json!({"rateLimits": {"primary": {"usedPercent": 40, "windowDurationMins": 300},
            "rateLimitReachedType": "rate_limit_reached"}});
        assert_eq!(from_codex_rate_limits(&v, "codex", 1).unwrap().status, QuotaStatus::Limited);
    }

    #[test]
    fn maps_claude_unified_windows_fraction_to_percent() {
        let info = json!({"status": "allowed_warning", "rateLimitType": "five_hour", "utilization": 0.81,
            "unifiedWindows": {"five_hour": {"utilization": 0.81, "resetsAt": 100},
                               "seven_day": {"utilization": 0.25, "resetsAt": 200}}});
        let s = from_claude_rate_limit_info(&info, "claude", 1).unwrap();
        assert_eq!(s.windows.len(), 2);
        assert!((s.windows[0].used_percent - 81.0).abs() < 1e-9);
        assert_eq!(s.status, QuotaStatus::Warning);
    }

    #[test]
    fn claude_rejected_maps_to_limited_and_blocks() {
        let info = json!({"status": "rejected", "rateLimitType": "five_hour", "utilization": 1.0, "resetsAt": 500});
        let s = from_claude_rate_limit_info(&info, "claude", 1).unwrap();
        assert_eq!(s.status, QuotaStatus::Limited);
        assert!(s.blocks(97.0, 100).is_some());
        // After the reset time passes, the stale snapshot no longer blocks.
        assert!(s.blocks(97.0, 600).is_none());
    }

    #[test]
    fn merge_keeps_windows_missing_from_partial_update() {
        let a = from_claude_rate_limit_info(
            &json!({"unifiedWindows": {"five_hour": {"utilization": 0.1, "resetsAt": 1},
                    "seven_day": {"utilization": 0.2, "resetsAt": 2}}}),
            "claude", 1).unwrap();
        let b = from_claude_rate_limit_info(
            &json!({"rateLimitType": "five_hour", "utilization": 0.5, "resetsAt": 3}), "claude", 2).unwrap();
        let m = a.merged_with(&b);
        assert_eq!(m.windows.len(), 2);
        assert!((m.windows[0].used_percent - 50.0).abs() < 1e-9);
        assert!((m.windows[1].used_percent - 20.0).abs() < 1e-9);
        assert_eq!(m.observed_at, 2);
    }
}
