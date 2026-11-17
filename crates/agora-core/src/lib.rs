//! Agora core — a multi-model collaboration engine that drives the **Claude Code** and
//! **Codex** command-line agents on the user's own subscriptions.
//!
//! Hard rule (see `docs/adr/0001-cli-only-never-paid-api.md`): this crate never calls a paid
//! API with an API key. Every provider scrubs API-key environment variables before spawning a
//! CLI and refuses to run if the CLI reports an API-key billing source.

pub mod compact;
pub mod digest;
pub mod engine;
pub mod event;
pub mod export;
pub mod git;
pub mod grade;
pub mod history;
pub mod memory;
pub mod model;
pub mod prompts;
pub mod provider;
pub mod quota;
pub mod repo;
pub mod scheduler;
pub mod skills;
pub mod store;
pub mod review;
pub mod team;
pub mod templates;
pub mod verify;

pub use engine::Engine;
pub use engine::{AssistantAnswer, AssistantError};
pub use event::Event;
pub use grade::EvidenceGrade;
pub use model::*;
pub use quota::{QuotaSnapshot, QuotaStatus, QuotaWindow};
pub use store::{Settings, Store};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
