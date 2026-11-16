//! Providers drive one model family through its official CLI on the user's subscription.
//!
//! - [`claude::ClaudeCli`] — `claude -p --output-format stream-json`
//! - [`codex::CodexCli`] — `codex exec --json` (+ `codex app-server` for quota/account)
//! - [`mock::MockProvider`] — deterministic, offline; used by tests and demo mode
//!
//! None of them ever uses an API key: see [`proc::PAID_API_ENV`] and each provider's
//! billing-source guard.

pub mod claude;
pub mod codex;
pub mod mock;
pub mod proc;
pub mod resolve;
pub mod stream;

use async_trait::async_trait;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::model::{Access, Effort, Family, TokenUsage, ToolNote};
use crate::quota::QuotaSnapshot;

pub struct TurnRequest {
    pub prompt: String,
    pub model: Option<String>,
    pub cwd: PathBuf,
    pub access: Access,
    pub timeout: Duration,
    pub cancel: CancellationToken,
    /// Reasoning effort; `None` passes nothing (the model's default).
    pub effort: Option<Effort>,
    /// Run the CLI without the user's personal configuration: no user-level MCP servers,
    /// plugins or hooks (Claude: `--strict-mcp-config --setting-sources project,local`;
    /// Codex: `--ignore-user-config`). See docs/lessons/0007.
    pub clean_config: bool,
}

#[derive(Clone, Debug)]
pub enum ProviderEvent {
    Delta(String),
    Tool(ToolNote),
    Quota(QuotaSnapshot),
    Warning(String),
}

pub type Sink = mpsc::UnboundedSender<ProviderEvent>;

#[derive(Clone, Debug, Default)]
pub struct TurnOutput {
    pub ok: bool,
    pub text: String,
    pub error: Option<String>,
    pub usage: Option<TokenUsage>,
    /// The CLI rejected the requested model for this account (e.g. Codex `model_not_found`).
    pub unsupported_model: bool,
    /// Name of the rejected model when the CLI said it — needed when no model was requested
    /// and the CLI's own configured default was the one rejected.
    pub rejected_model: Option<String>,
    /// A billing guard aborted the run (never retried automatically).
    pub guard_tripped: bool,
}

impl TurnOutput {
    pub fn fail(msg: impl Into<String>) -> Self {
        TurnOutput { ok: false, error: Some(msg.into()), ..Default::default() }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Health {
    pub ok: bool,
    pub summary: String,
    /// What the user should do, if anything (shown verbatim; may contain a command).
    pub fix: Option<String>,
    pub version: Option<String>,
    /// Account/plan description when known (never secrets).
    pub account: Option<String>,
    pub executable: Option<String>,
    pub checked_at: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
    /// Reasoning efforts this model accepts; empty = effort cannot be set for it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<Effort>,
    /// The model's own default effort, when the CLI reports one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<Effort>,
}

impl ModelOption {
    pub fn plain(id: &str, label: &str) -> Self {
        ModelOption { id: id.into(), label: label.into(), efforts: vec![], default_effort: None }
    }
}

/// Efforts accepted for `model` (`None`/empty = the CLI default entry, id `""`). A model that
/// is not in the list falls back to the CLI-default entry, which is the intersection of all
/// listed models — never wider than what every model accepts.
pub fn supported_efforts(models: &[ModelOption], model: Option<&str>) -> Vec<Effort> {
    let id = model.map(str::trim).unwrap_or("");
    models
        .iter()
        .find(|m| m.id == id)
        .or_else(|| models.iter().find(|m| m.id.is_empty()))
        .map(|m| m.efforts.clone())
        .unwrap_or_default()
}

/// Intersection of the efforts of every listed model (for the "CLI default" entry).
pub fn common_efforts(models: &[ModelOption]) -> Vec<Effort> {
    let listed: Vec<&ModelOption> = models.iter().filter(|m| !m.id.is_empty()).collect();
    if listed.is_empty() {
        return vec![];
    }
    Effort::ALL.into_iter().filter(|e| listed.iter().all(|m| m.efforts.contains(e))).collect()
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn family(&self) -> Family;
    fn display_name(&self) -> String;
    /// True for the offline mock providers.
    fn is_mock(&self) -> bool {
        false
    }
    fn models(&self) -> Vec<ModelOption>;
    /// Cheap local check: binary present, logged in, subscription (not API-key) auth.
    async fn health(&self) -> Health;
    /// Actively poll quota without spending any of it. `None` when the provider has no
    /// free way to do that (Claude: quota arrives passively with each turn).
    async fn poll_quota(&self) -> Option<QuotaSnapshot> {
        None
    }
    async fn run(&self, req: TurnRequest, sink: Sink) -> TurnOutput;
}
