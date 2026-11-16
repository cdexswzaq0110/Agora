//! Local JSON persistence under the data directory (`%APPDATA%\Agora` on Windows).
//!
//! ```text
//! settings.json      ports, paired phones, CLI paths, guards
//! projects.json      projects (+ project memory)
//! quota.json         last quota snapshot per provider (so the meter survives restarts)
//! sessions/<id>.json full session records
//! sandbox/           default working directory for ad-hoc questions
//! ```

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::digest::{Digest, DigestSettings};
use crate::model::{new_id, now_ms, Effort, Project, SessionRecord};
use crate::quota::QuotaSnapshot;
use crate::scheduler::Schedule;
use crate::templates::Template;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub token: String,
    pub created_at: u64,
    pub last_seen: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Loopback port for the UI/API.
    pub port: u16,
    /// Also listen on all interfaces (for phones on the LAN) on `lan_port`.
    pub lan_enabled: bool,
    pub lan_port: u16,
    pub devices: Vec<PairedDevice>,
    pub claude_path: Option<String>,
    pub codex_path: Option<String>,
    /// provider id → model id used when a participant leaves the model empty.
    pub default_models: BTreeMap<String, String>,
    /// provider id → reasoning effort used when a participant leaves effort empty. Applied only
    /// when the turn's model accepts it (see runner); never sent to a model that rejects it.
    pub default_efforts: BTreeMap<String, Effort>,
    /// provider id → models the CLI rejected for this account.
    pub unsupported_models: BTreeMap<String, Vec<String>>,
    /// Refuse new turns when any quota window is at/above this percentage.
    pub quota_stop_percent: f64,
    /// Add the offline mock providers.
    pub demo_mode: bool,
    pub turn_timeout_secs: u64,
    pub verify_timeout_secs: u64,
    /// Character budget for the transcript inside each prompt (記憶壓縮 threshold).
    pub transcript_budget_chars: usize,
    /// When a provider's quota is nearly gone, hand its turn to another ready provider.
    pub auto_reroute: bool,
    /// Put a project snapshot (tree, README head, recent commits) into planning prompts.
    pub context_pack: bool,
    /// Run the CLIs without the user's personal MCP servers, plugins and hooks.
    pub isolate_cli_config: bool,
    pub digest: DigestSettings,
    pub schedules: Vec<Schedule>,
    /// User-saved templates (built-ins live in `templates::builtins`).
    pub templates: Vec<Template>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            port: 4717,
            lan_enabled: false,
            lan_port: 4718,
            devices: vec![],
            claude_path: None,
            codex_path: None,
            default_models: BTreeMap::new(),
            default_efforts: BTreeMap::new(),
            unsupported_models: BTreeMap::new(),
            quota_stop_percent: 97.0,
            demo_mode: false,
            turn_timeout_secs: 20 * 60,
            verify_timeout_secs: 10 * 60,
            transcript_budget_chars: 24_000,
            auto_reroute: true,
            context_pack: true,
            isolate_cli_config: true,
            digest: DigestSettings::default(),
            schedules: vec![],
            templates: vec![],
        }
    }
}

pub struct Store {
    root: PathBuf,
    write_lock: Mutex<()>,
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("寫入 {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("更新 {}", path.display()))?;
    Ok(())
}

fn valid_date(d: &str) -> bool {
    d.len() == 10 && d.chars().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

impl Store {
    /// `AGORA_DATA_DIR`, else `%APPDATA%\Agora`, else `$XDG_DATA_HOME/agora`, else
    /// `~/.local/share/agora`.
    pub fn default_root() -> PathBuf {
        if let Some(p) = std::env::var_os("AGORA_DATA_DIR") {
            return PathBuf::from(p);
        }
        if cfg!(windows) {
            if let Some(a) = std::env::var_os("APPDATA") {
                return PathBuf::from(a).join("Agora");
            }
        }
        if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(x).join("agora");
        }
        if let Some(h) = std::env::var_os("HOME") {
            return PathBuf::from(h).join(".local/share/agora");
        }
        PathBuf::from(".agora-data")
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Store> {
        let root = root.into();
        fs::create_dir_all(root.join("sessions")).with_context(|| format!("建立資料夾 {}", root.display()))?;
        fs::create_dir_all(root.join("sandbox"))?;
        fs::create_dir_all(root.join("digests"))?;
        fs::create_dir_all(root.join("experiments"))?;
        Ok(Store { root, write_lock: Mutex::new(()) })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn sandbox_dir(&self) -> PathBuf {
        self.root.join("sandbox")
    }

    pub fn worktrees_dir(&self) -> PathBuf {
        self.root.join("worktrees")
    }

    pub fn quota_history_path(&self) -> PathBuf {
        self.root.join("quota_history.jsonl")
    }

    fn read_json<T: for<'de> Deserialize<'de>>(&self, name: &str) -> Option<T> {
        let raw = fs::read_to_string(self.root.join(name)).ok()?;
        serde_json::from_str(&raw).ok()
    }

    fn write_json<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let _g = self.write_lock.lock().unwrap();
        write_atomic(&self.root.join(name), &serde_json::to_vec_pretty(value)?)
    }

    pub fn settings(&self) -> Settings {
        self.read_json("settings.json").unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        self.write_json("settings.json", s)
    }

    pub fn projects(&self) -> Vec<Project> {
        self.read_json("projects.json").unwrap_or_default()
    }

    pub fn save_projects(&self, p: &[Project]) -> Result<()> {
        self.write_json("projects.json", &p)
    }

    /// Make sure at least one project exists: the sandbox.
    pub fn ensure_default_project(&self) -> Result<Vec<Project>> {
        let mut ps = self.projects();
        if ps.is_empty() {
            ps.push(Project {
                id: new_id("p"),
                name: "沙盒".into(),
                path: self.sandbox_dir().display().to_string(),
                created_at: now_ms(),
                memory: String::new(),
            });
            self.save_projects(&ps)?;
        }
        Ok(ps)
    }

    pub fn quota_cache(&self) -> Vec<QuotaSnapshot> {
        self.read_json("quota.json").unwrap_or_default()
    }

    pub fn save_quota_cache(&self, q: &[QuotaSnapshot]) -> Result<()> {
        self.write_json("quota.json", &q)
    }

    fn session_path(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            bail!("無效的議題 id");
        }
        Ok(self.root.join("sessions").join(format!("{id}.json")))
    }

    pub fn save_session(&self, rec: &SessionRecord) -> Result<()> {
        let path = self.session_path(&rec.id)?;
        let _g = self.write_lock.lock().unwrap();
        write_atomic(&path, &serde_json::to_vec_pretty(rec)?)
    }

    pub fn load_session(&self, id: &str) -> Option<SessionRecord> {
        let raw = fs::read_to_string(self.session_path(id).ok()?).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn save_digest(&self, d: &Digest) -> Result<()> {
        if !valid_date(&d.date) {
            bail!("無效的日期");
        }
        let _g = self.write_lock.lock().unwrap();
        write_atomic(&self.root.join("digests").join(format!("{}.json", d.date)), &serde_json::to_vec_pretty(d)?)
    }

    /// 情報已讀游標：date → `generated_at` of the version that was opened. One cursor shared by
    /// every device (the same user reading on the phone has read it).
    pub fn digest_reads(&self) -> BTreeMap<String, u64> {
        fs::read_to_string(self.root.join("digest-read.json")).ok().and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default()
    }

    /// Mark `date` read at `generated_at`. The version must be the one stored right now (no
    /// forged or future cursor) and the cursor only moves forward. Shares the write lock with
    /// `save_digest`, so a regeneration and a read cannot interleave.
    pub fn mark_digest_read(&self, date: &str, generated_at: u64) -> Result<()> {
        if !valid_date(date) {
            bail!("無效的日期");
        }
        let _g = self.write_lock.lock().unwrap();
        let Some(d) = self.load_digest(date) else {
            bail!("找不到這一天的情報");
        };
        if d.generated_at != generated_at {
            bail!("情報已經重新整理過，請重新載入後再標記已讀");
        }
        let mut m = self.digest_reads();
        let cur = m.get(date).copied().unwrap_or(0);
        m.insert(date.to_string(), cur.max(generated_at));
        while m.len() > 90 {
            let oldest = m.keys().next().cloned().unwrap_or_default();
            m.remove(&oldest);
        }
        write_atomic(&self.root.join("digest-read.json"), &serde_json::to_vec_pretty(&m)?)
    }

    pub fn load_digest(&self, date: &str) -> Option<Digest> {
        if !valid_date(date) {
            return None;
        }
        let raw = fs::read_to_string(self.root.join("digests").join(format!("{date}.json"))).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// Dates with a stored digest, newest first.
    pub fn digest_dates(&self) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(self.root.join("digests"))
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".json").map(str::to_string))
                    .filter(|d| valid_date(d))
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by(|a, b| b.cmp(a));
        v
    }

    pub fn save_experiment(&self, e: &crate::engine::Experiment) -> Result<()> {
        if !valid_id(&e.id) {
            bail!("無效的實驗 id");
        }
        let _g = self.write_lock.lock().unwrap();
        write_atomic(&self.root.join("experiments").join(format!("{}.json", e.id)), &serde_json::to_vec_pretty(e)?)
    }

    pub fn load_experiment(&self, id: &str) -> Option<crate::engine::Experiment> {
        if !valid_id(id) {
            return None;
        }
        serde_json::from_str(&fs::read_to_string(self.root.join("experiments").join(format!("{id}.json"))).ok()?).ok()
    }

    pub fn experiments(&self) -> Vec<crate::engine::Experiment> {
        let mut v: Vec<crate::engine::Experiment> = fs::read_dir(self.root.join("experiments"))
            .map(|rd| rd.flatten().filter_map(|e| fs::read_to_string(e.path()).ok()).filter_map(|r| serde_json::from_str(&r).ok()).collect())
            .unwrap_or_default();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }

    pub fn sessions(&self) -> Vec<SessionRecord> {
        let Ok(rd) = fs::read_dir(self.root.join("sessions")) else {
            return vec![];
        };
        let mut v: Vec<SessionRecord> = rd
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| fs::read_to_string(e.path()).ok())
            .filter_map(|raw| serde_json::from_str(&raw).ok())
            .collect();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_ids() {
        assert!(!valid_id("../x"));
        assert!(!valid_id("a/b"));
        assert!(valid_id("s-18f00aa1b2c3"));
    }

    #[test]
    fn digest_dates_are_validated() {
        assert!(valid_date("2026-11-16"));
        assert!(!valid_date("../../x.json"));
        assert!(!valid_date("2026-1-16"));
    }

    #[test]
    fn settings_tolerate_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"port": 5000}"#).unwrap();
        assert_eq!(s.port, 5000);
        assert_eq!(s.quota_stop_percent, 97.0);
    }
}
