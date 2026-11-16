//! Domain types shared by the engine, the HTTP server and the UI (serialized as JSON).

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::grade::EvidenceGrade;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Hex string from the OS CSPRNG (`getrandom`). Used for ids and pairing tokens.
pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    if getrandom::fill(&mut buf).is_err() {
        // Extremely unlikely; degrade to time-derived bytes rather than panic.
        let t = now_ms().to_le_bytes();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = t[i % t.len()] ^ (i as u8).wrapping_mul(31);
        }
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}-{:x}{}", now_ms(), random_hex(3))
}

fn yes() -> bool {
    true
}

/// Which model family a provider belongs to.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Claude,
    Gpt,
}

impl Family {
    pub fn label(self) -> &'static str {
        match self {
            Family::Claude => "Claude",
            Family::Gpt => "GPT",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// 單人 — one model answers.
    Solo,
    /// 協做 — builder → (verify) → reviewer, bounded revisions.
    Relay,
    /// 討論 — blind first round, then rounds of response, then a synthesis.
    Roundtable,
    /// 辯論 — proponent vs opponent, rebuttal rounds, then (cross-)judging.
    Debate,
    /// 方案工作坊 — independent proposals → cross-critique → synthesized plan → human
    /// approval → build (isolated) → verify → review against the plan's acceptance criteria.
    Workshop,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Solo => "單人",
            Mode::Relay => "協做",
            Mode::Roundtable => "討論",
            Mode::Debate => "辯論",
            Mode::Workshop => "方案工作坊",
        }
    }

    pub fn all() -> [Mode; 5] {
        [Mode::Workshop, Mode::Solo, Mode::Relay, Mode::Roundtable, Mode::Debate]
    }
}

/// Whether a turn may modify files in the project directory.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    #[default]
    ReadOnly,
    Write,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Solo,
    Builder,
    Reviewer,
    Panelist,
    Synthesizer,
    Proponent,
    Opponent,
    Judge,
    Planner,
    Critic,
    Summarizer,
    Probe,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Solo => "單人",
            Role::Builder => "執行者",
            Role::Reviewer => "審查者",
            Role::Panelist => "與談人",
            Role::Synthesizer => "總結者",
            Role::Proponent => "正方",
            Role::Opponent => "反方",
            Role::Judge => "裁判",
            Role::Planner => "提案者",
            Role::Critic => "評論者",
            Role::Summarizer => "情報編輯",
            Role::Probe => "探測",
        }
    }
}

/// Reasoning effort passed to the CLI (Codex `-c model_reasoning_effort=…`, Claude `--effort`).
/// A closed set so typos never reach a CLI; which values a model accepts comes from the
/// provider's capability list (`ModelOption::efforts`) and is checked in `Engine::validate`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    Ultra,
}

impl Effort {
    pub const ALL: [Effort; 6] = [Effort::Low, Effort::Medium, Effort::High, Effort::Xhigh, Effort::Max, Effort::Ultra];

    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::Xhigh => "xhigh",
            Effort::Max => "max",
            Effort::Ultra => "ultra",
        }
    }

    /// Unknown names (a newer CLI) are ignored, never guessed.
    pub fn parse(s: &str) -> Option<Effort> {
        Effort::ALL.into_iter().find(|e| e.as_str().eq_ignore_ascii_case(s.trim()))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Participant {
    /// Provider id, e.g. `claude`, `codex`, `mock-claude`.
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    /// `None` = the model's own default effort (nothing is passed to the CLI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
}

impl Participant {
    pub fn new(provider: &str) -> Self {
        Participant { provider: provider.into(), model: None, effort: None }
    }
}

/// Continue from an earlier session's transcript (分岔).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Seed {
    pub session_id: String,
    /// Copy turns `0..=upto` (by index).
    pub upto: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSpec {
    pub project_id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub mode: Mode,
    pub participants: Vec<Participant>,
    /// Roundtable/Workshop: the synthesizer. Debate: the judge; `None` = cross-judging.
    #[serde(default)]
    pub judge: Option<Participant>,
    /// Workshop: who implements the approved plan (default: first participant).
    #[serde(default)]
    pub builder: Option<Participant>,
    /// Workshop: who reviews the implementation (default: the other participant).
    #[serde(default)]
    pub reviewer: Option<Participant>,
    pub prompt: String,
    #[serde(default)]
    pub rounds: Option<u32>,
    #[serde(default)]
    pub access: Access,
    #[serde(default)]
    pub verify_command: Option<String>,
    #[serde(default)]
    pub max_turns: Option<u32>,
    /// Hide which model wrote what from reviewers/judges/critics (盲審).
    #[serde(default)]
    pub blind: bool,
    /// Workshop: add a red-team critique of the synthesized plan before approval.
    #[serde(default)]
    pub red_team: bool,
    /// Workshop: stop after the plan is approved (no implementation).
    #[serde(default)]
    pub plan_only: bool,
    /// Workshop: do not wait for a human at the approval gate.
    #[serde(default)]
    pub auto_approve: bool,
    /// Write sessions in a git repo run in a separate worktree (default on).
    #[serde(default = "yes")]
    pub isolate: bool,
    #[serde(default)]
    pub seed: Option<Seed>,
    /// Sessions started together by a mode comparison share a group id.
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
}

impl SessionSpec {
    pub fn new(project_id: &str, mode: Mode, participants: Vec<Participant>, prompt: &str) -> Self {
        SessionSpec {
            project_id: project_id.into(),
            title: None,
            mode,
            participants,
            judge: None,
            builder: None,
            reviewer: None,
            prompt: prompt.into(),
            rounds: None,
            access: Access::ReadOnly,
            verify_command: None,
            max_turns: None,
            blind: false,
            red_team: false,
            plan_only: false,
            auto_approve: false,
            isolate: true,
            seed: None,
            group_id: None,
            template: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_input_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolNote {
    pub name: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    pub index: u32,
    pub round: u32,
    pub role: Role,
    pub provider: String,
    pub family: Family,
    pub model: Option<String>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub text: String,
    pub ok: bool,
    pub error: Option<String>,
    pub usage: Option<TokenUsage>,
    pub tools: Vec<ToolNote>,
    /// Sequence number of the last streamed delta folded into `text`; lets SSE clients
    /// drop deltas that race with a snapshot.
    pub delta_seq: u64,
    /// True when the transcript shown to this turn was compressed (see `compact.rs`).
    pub context_compacted: bool,
    /// Copied from another session by a fork; not a CLI call of this session.
    #[serde(default)]
    pub seeded: bool,
    /// Set when the quota guard moved this turn to another provider.
    #[serde(default)]
    pub rerouted_from: Option<String>,
    /// Reasoning effort actually passed to the CLI for this turn (audit trail).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    /// `effort`, or the model's own default when the CLI reports it; `None` = unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_effort: Option<Effort>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    /// Workshop: the plan is ready and waits for a human decision.
    AwaitingApproval,
    Done,
    Failed,
    Cancelled,
}

impl Status {
    pub fn is_live(self) -> bool {
        matches!(self, Status::Running | Status::AwaitingApproval)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoteLevel {
    Info,
    Warn,
    /// A safety guard fired (budget, quota, billing source).
    Guard,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub at: u64,
    pub level: NoteLevel,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Interjection {
    pub at: u64,
    pub text: String,
    /// Set once the interjection has been included in at least one turn's prompt.
    pub delivered: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifyRun {
    pub command: String,
    pub round: u32,
    pub exit_code: Option<i32>,
    pub ok: bool,
    pub timed_out: bool,
    pub output_tail: String,
    pub started_at: u64,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Verdict {
    /// Short outcome key: `approved`, `open_issues`, `verify_failed`, `answered`, `verified`,
    /// `synthesized`, `judged`, `split`, `planned`, `rejected`.
    pub outcome: String,
    pub headline: String,
    pub grade: EvidenceGrade,
    pub summary: String,
    pub open_issues: Vec<String>,
    /// One sentence worth remembering for later sessions, from the model's `記憶：` line.
    /// Only ever a suggestion: the user edits and confirms before anything is written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_hint: Option<String>,
}

/// The workshop's synthesized plan and the human decision on it.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Plan {
    pub text: String,
    /// 0 = first synthesis; +1 per requested revision.
    pub revision: u32,
    pub approved: bool,
    /// The human edited the text before approving.
    pub edited: bool,
    pub verify_command: Option<String>,
    pub decided_at: Option<u64>,
    /// Revision requests sent back to the synthesizer.
    pub feedback: Vec<String>,
    /// `##`/`###` sections of `text` (outside code fences), recomputed whenever the plan is
    /// set — the only valid anchors for section comments on *this* revision.
    #[serde(default)]
    pub blocks: Vec<PlanBlock>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct PlanBlock {
    pub id: String,
    pub heading: String,
    pub level: u8,
}

/// A comment on one section of the plan, sent with 要求修訂.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct PlanComment {
    pub block_id: String,
    #[serde(default)]
    pub heading: String,
    #[serde(default)]
    pub quote: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    /// A separate `git worktree` on branch `agora/<session>`.
    Worktree,
    /// Edits go straight into the project directory.
    Direct,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState {
    Active,
    Applied,
    Discarded,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workspace {
    pub kind: WorkspaceKind,
    /// Directory the CLI agents work in.
    pub path: String,
    /// Worktree root (differs from `path` when the project is a subdirectory of the repo).
    pub root: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub state: WorkspaceState,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub spec: SessionSpec,
    pub title: String,
    pub status: Status,
    pub created_at: u64,
    pub updated_at: u64,
    pub turns: Vec<Turn>,
    pub notes: Vec<Note>,
    pub interjections: Vec<Interjection>,
    pub verify_runs: Vec<VerifyRun>,
    pub verdict: Option<Verdict>,
    pub error: Option<String>,
    #[serde(default)]
    pub plan: Option<Plan>,
    #[serde(default)]
    pub workspace: Option<Workspace>,
    /// Human-readable current phase (e.g. 提案, 互評, 整合, 等待核准, 實作, 驗證, 審查).
    #[serde(default)]
    pub phase: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: Mode,
    pub status: Status,
    pub participants: Vec<Participant>,
    pub turns: usize,
    pub created_at: u64,
    pub updated_at: u64,
    pub grade: Option<EvidenceGrade>,
    pub phase: Option<String>,
    pub group_id: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl From<&SessionRecord> for SessionSummary {
    fn from(r: &SessionRecord) -> Self {
        let (input_tokens, output_tokens) = r
            .turns
            .iter()
            .filter(|t| !t.seeded)
            .filter_map(|t| t.usage.as_ref())
            .fold((0, 0), |(i, o), u| (i + u.input_tokens, o + u.output_tokens));
        SessionSummary {
            id: r.id.clone(),
            project_id: r.spec.project_id.clone(),
            title: r.title.clone(),
            mode: r.spec.mode,
            status: r.status,
            participants: r.spec.participants.clone(),
            turns: r.turns.iter().filter(|t| !t.seeded).count(),
            created_at: r.created_at,
            updated_at: r.updated_at,
            grade: r.verdict.as_ref().map(|v| v.grade),
            phase: r.phase.clone(),
            group_id: r.spec.group_id.clone(),
            input_tokens,
            output_tokens,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Working directory handed to the CLI agents.
    pub path: String,
    pub created_at: u64,
    /// Project memory: durable decisions/lessons injected (bounded) into every prompt.
    #[serde(default)]
    pub memory: String,
}
