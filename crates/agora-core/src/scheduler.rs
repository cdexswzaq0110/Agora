//! 排程 — the daily digest and user-defined scheduled sessions (e.g. a nightly two-model code
//! review). A tick runs every minute while Agora is open; anything missed while the computer
//! was off runs once when Agora starts (catch-up), never twice on the same day.

use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::engine::Engine;
use crate::model::SessionSpec;

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Local `HH:MM`.
    pub time: String,
    /// ISO weekdays 1 (Mon) – 7 (Sun); empty = every day.
    #[serde(default)]
    pub weekdays: Vec<u8>,
    pub spec: SessionSpec,
    /// Local date of the last run.
    #[serde(default)]
    pub last_run: Option<String>,
}

pub fn local_date() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub fn parse_hm(s: &str) -> (u32, u32) {
    let mut it = s.split(':');
    let h = it.next().and_then(|x| x.trim().parse().ok()).filter(|h: &u32| *h < 24).unwrap_or(8);
    let m = it.next().and_then(|x| x.trim().parse().ok()).filter(|m: &u32| *m < 60).unwrap_or(0);
    (h, m)
}

/// Pure due-check: today is an allowed weekday, the time has passed, and it has not run today.
pub fn due(today: &str, now_hm: (u32, u32), weekday: u8, time: &str, weekdays: &[u8], last_run: Option<&str>) -> bool {
    if last_run == Some(today) {
        return false;
    }
    if !weekdays.is_empty() && !weekdays.contains(&weekday) {
        return false;
    }
    now_hm >= parse_hm(time)
}

/// One scheduler pass.
pub async fn tick(engine: &Arc<Engine>) {
    let now = Local::now();
    let today = now.format("%Y-%m-%d").to_string();
    let hm = (now.hour(), now.minute());
    let weekday = now.weekday().number_from_monday() as u8;
    let settings = engine.settings();

    let d = &settings.digest;
    if d.enabled && engine.store().load_digest(&today).is_none() && due(&today, hm, weekday, &d.time, &[], None) && engine.digest_may_retry() {
        let e = engine.clone();
        tokio::spawn(async move {
            let _ = e.run_digest().await;
        });
    }

    for s in settings.schedules.iter().filter(|s| s.enabled) {
        if due(&today, hm, weekday, &s.time, &s.weekdays, s.last_run.as_deref()) {
            engine.run_schedule(&s.id, &today).await;
        }
    }
}

pub fn spawn(engine: Arc<Engine>) {
    tokio::spawn(async move {
        // Let startup health checks finish first.
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            tick(&engine).await;
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_respects_time_weekday_and_last_run() {
        assert!(due("2026-11-16", (8, 0), 1, "08:00", &[], None));
        assert!(!due("2026-11-16", (7, 59), 1, "08:00", &[], None));
        assert!(!due("2026-11-16", (9, 0), 1, "08:00", &[], Some("2026-11-16")));
        assert!(due("2026-11-16", (23, 0), 1, "08:00", &[], Some("2026-11-15")), "catch-up later the same day");
        assert!(!due("2026-11-16", (9, 0), 1, "08:00", &[6, 7], None), "weekends only");
        assert!(due("2026-11-21", (9, 0), 6, "08:00", &[6, 7], None));
    }
}
