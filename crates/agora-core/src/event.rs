//! Events broadcast by the engine. The HTTP server forwards them as Server-Sent Events, and
//! the desktop shell listens for `Status` to raise notifications.

use serde::Serialize;

use crate::model::*;
use crate::quota::QuotaSnapshot;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Status {
        session_id: String,
        title: String,
        status: Status,
        error: Option<String>,
    },
    TurnStarted {
        session_id: String,
        turn: Turn,
    },
    Delta {
        session_id: String,
        index: u32,
        seq: u64,
        text: String,
    },
    Tool {
        session_id: String,
        index: u32,
        note: ToolNote,
    },
    TurnEnded {
        session_id: String,
        turn: Turn,
    },
    Note {
        session_id: String,
        note: Note,
    },
    Interjection {
        session_id: String,
        interjection: Interjection,
    },
    Verify {
        session_id: String,
        run: VerifyRun,
    },
    Verdict {
        session_id: String,
        verdict: Verdict,
    },
    Quota {
        snapshot: QuotaSnapshot,
    },
    /// Workshop phase changed (提案 → 互評 → 整合 → 等待核准 → 實作 → 驗證 → 審查).
    Phase {
        session_id: String,
        phase: String,
    },
    /// The workshop plan changed (synthesized, revised, approved).
    Plan {
        session_id: String,
        plan: Plan,
    },
    Workspace {
        session_id: String,
        workspace: Workspace,
    },
    /// Daily digest progress: `running`, `done`, `failed`.
    Digest {
        status: String,
        date: String,
        error: Option<String>,
    },
}

impl Event {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Event::Status { session_id, .. }
            | Event::TurnStarted { session_id, .. }
            | Event::Delta { session_id, .. }
            | Event::Tool { session_id, .. }
            | Event::TurnEnded { session_id, .. }
            | Event::Note { session_id, .. }
            | Event::Interjection { session_id, .. }
            | Event::Verify { session_id, .. }
            | Event::Verdict { session_id, .. }
            | Event::Phase { session_id, .. }
            | Event::Plan { session_id, .. }
            | Event::Workspace { session_id, .. } => Some(session_id),
            Event::Quota { .. } | Event::Digest { .. } => None,
        }
    }

    /// Events worth sending on the global (non-session) stream: low volume only.
    pub fn is_global(&self) -> bool {
        matches!(self, Event::Status { .. } | Event::Quota { .. } | Event::Digest { .. })
    }
}
