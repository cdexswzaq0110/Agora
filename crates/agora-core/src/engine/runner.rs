//! The per-session runner: one model turn at a time (or a parallel round), the approval gate,
//! and workspace isolation. Modes in `modes.rs` are written in terms of these primitives.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use super::{Engine, GateDecision, Stop};
use crate::compact::{self, TranscriptEntry};
use crate::event::Event;
use crate::git;
use crate::model::*;
use crate::prompts::{self, PromptContext};
use crate::provider::{ProviderEvent, TurnOutput, TurnRequest};
use crate::store::Settings;
use crate::verify::run_verify;

pub(crate) struct RunCtl {
    pub cancel: CancellationToken,
    pub record: Mutex<SessionRecord>,
    /// Present while the session waits at the approval gate.
    pub gate: Mutex<Option<oneshot::Sender<GateDecision>>>,
}

impl RunCtl {
    pub fn new(rec: SessionRecord) -> Self {
        RunCtl { cancel: CancellationToken::new(), record: Mutex::new(rec), gate: Mutex::new(None) }
    }
}

pub(crate) struct Runner {
    pub engine: Arc<Engine>,
    pub ctl: Arc<RunCtl>,
    pub session_id: String,
    pub project: Project,
    pub spec: SessionSpec,
    pub settings: Settings,
    pub turns_used: AtomicU32,
    pub max_turns: u32,
    /// Rounds already used by seeded (forked) turns.
    pub round_offset: u32,
    /// Where CLI agents run: the project, or the isolated worktree once created.
    pub workdir: std::sync::Mutex<PathBuf>,
    /// 專案快照 for planning/discussion turns.
    pub snapshot: Option<String>,
    /// A gate decision to apply first (resuming a workshop after a restart).
    pub resume: std::sync::Mutex<Option<GateDecision>>,
}

pub(crate) struct TurnPlan {
    pub participant: Participant,
    pub role: Role,
    pub round: u32,
    pub ask: String,
    pub access: Access,
    /// 專案團隊: the roles this call plays (empty outside team stages).
    pub team_roles: Vec<String>,
}

const PLANNING_ROLES: &[Role] = &[
    Role::Planner,
    Role::Critic,
    Role::Synthesizer,
    Role::Panelist,
    Role::Proponent,
    Role::Opponent,
    Role::Judge,
];

impl Runner {
    pub fn plan(&self, p: &Participant, role: Role, round: u32, ask: String, access: Access) -> TurnPlan {
        TurnPlan { participant: p.clone(), role, round, ask, access, team_roles: vec![] }
    }

    /// A read-only team-stage call that plays `roles`.
    pub fn team_plan(&self, p: &Participant, role: Role, round: u32, ask: String, roles: &[String]) -> TurnPlan {
        TurnPlan { team_roles: roles.to_vec(), ..self.plan(p, role, round, ask, Access::ReadOnly) }
    }

    /// Mode-local round `n` → session round (after any seeded rounds).
    pub fn r(&self, n: u32) -> u32 {
        n + self.round_offset
    }

    pub async fn next_round(&self) -> u32 {
        self.ctl.record.lock().await.turns.iter().map(|t| t.round).max().unwrap_or(0) + 1
    }

    pub async fn run(self: Arc<Self>) {
        let result = match self.spec.mode {
            Mode::Solo => self.solo().await,
            Mode::Relay => self.relay().await,
            Mode::Roundtable => self.roundtable().await,
            Mode::Debate => self.debate().await,
            Mode::Workshop => self.workshop().await,
        };
        let (status, error) = match result {
            Ok(()) => (Status::Done, None),
            Err(e) => match e.downcast_ref::<Stop>() {
                Some(Stop::Cancelled) => (Status::Cancelled, None),
                Some(Stop::Budget(_)) => (Status::Done, None),
                None => (Status::Failed, Some(format!("{e:#}"))),
            },
        };
        self.workspace_summary_note().await;
        let snapshot = {
            let mut r = self.ctl.record.lock().await;
            r.status = status;
            r.error = error.clone();
            r.phase = None;
            r.updated_at = now_ms();
            r.clone()
        };
        let _ = self.engine.store.save_session(&snapshot);
        self.engine.running.lock().await.remove(&self.session_id);
        self.engine.emit(Event::Status { session_id: self.session_id.clone(), title: snapshot.title, status, error });
        self.sync_workflow().await;
    }

    pub async fn persist(&self) {
        let snapshot = self.ctl.record.lock().await.clone();
        let _ = self.engine.store.save_session(&snapshot);
    }

    pub async fn note(&self, level: NoteLevel, text: String) {
        let note = Note { at: now_ms(), level, text };
        self.ctl.record.lock().await.notes.push(note.clone());
        self.engine.emit(Event::Note { session_id: self.session_id.clone(), note });
    }

    pub async fn set_phase(&self, phase: &str) {
        self.ctl.record.lock().await.phase = Some(phase.to_string());
        self.engine.emit(Event::Phase { session_id: self.session_id.clone(), phase: phase.to_string() });
    }

    pub async fn set_status(&self, status: Status, phase: &str) {
        let title = {
            let mut r = self.ctl.record.lock().await;
            r.status = status;
            r.phase = Some(phase.to_string());
            r.updated_at = now_ms();
            r.title.clone()
        };
        self.persist().await;
        self.engine.emit(Event::Phase { session_id: self.session_id.clone(), phase: phase.to_string() });
        self.engine.emit(Event::Status { session_id: self.session_id.clone(), title, status, error: None });
        self.sync_workflow().await;
    }

    /// A team stage's workflow follows its session (reconciled from the record, idempotent).
    async fn sync_workflow(&self) {
        if let Some(st) = &self.spec.stage {
            self.engine.sync_workflow(&st.workflow_id).await;
        }
    }

    pub async fn visible_now(&self) -> usize {
        self.ctl.record.lock().await.turns.len()
    }

    pub fn check_cancel(&self) -> Result<()> {
        if self.ctl.cancel.is_cancelled() {
            return Err(Stop::Cancelled.into());
        }
        Ok(())
    }

    fn cwd(&self) -> PathBuf {
        self.workdir.lock().unwrap().clone()
    }

    /// Stable anonymous labels for blind sessions (成員 A, B, …), assigned by first appearance
    /// in the transcript, keyed by the original provider and the model actually used.
    fn blind_labels(turns: &[Turn]) -> HashMap<String, char> {
        let mut m = HashMap::new();
        for t in turns {
            let next = (b'A' + (m.len() as u8).min(25)) as char;
            m.entry(Self::blind_key(t)).or_insert(next);
        }
        m
    }

    fn blind_key(t: &Turn) -> String {
        format!("{}|{}", t.rerouted_from.clone().unwrap_or_else(|| t.provider.clone()), t.model.clone().unwrap_or_default())
    }

    /// One model turn. `visible` = how many earlier turns this turn may see (parallel turns in
    /// the same round share a value so they stay independent).
    pub async fn turn(self: &Arc<Self>, plan: TurnPlan, visible: usize) -> Result<Turn> {
        self.check_cancel()?;
        let used = self.turns_used.fetch_add(1, Ordering::SeqCst);
        if used >= self.max_turns {
            self.note(NoteLevel::Guard, format!("已達本議題的發言上限（{} 次）——依設計停止，保留目前結果。", self.max_turns)).await;
            return Err(Stop::Budget(self.max_turns).into());
        }
        let TurnPlan { participant, role, round, ask, access, team_roles } = plan;
        let mut provider = self.engine.provider(&participant.provider).ok_or_else(|| anyhow!("找不到模型提供者 {}", participant.provider))?;
        let mut model = participant.model.clone().or_else(|| self.settings.default_models.get(provider.id()).cloned().filter(|m| !m.is_empty()));

        // Quota guard — reroute to another ready provider, or refuse before spending anything.
        let mut blocked = self.engine.quota_block(provider.id());
        let mut rerouted_from = None;
        if let Some(reason) = blocked.clone() {
            if self.settings.auto_reroute {
                if let Some(alt) = self.engine.reroute_candidate(&provider) {
                    self.note(
                        NoteLevel::Guard,
                        format!("額度保護：{reason}；這一則改由 {} 代打（可在設定關閉自動改派）。", alt.display_name()),
                    )
                    .await;
                    rerouted_from = Some(provider.id().to_string());
                    model = self.settings.default_models.get(alt.id()).cloned().filter(|m| !m.is_empty());
                    provider = alt;
                    blocked = None;
                }
            }
        }

        // Reasoning effort. An explicit participant value was validated at start; it is dropped
        // when the turn moved to another provider. Otherwise the settings default applies only
        // when this model lists it — never sent to a model that would reject it.
        let models = provider.models();
        let supported = crate::provider::supported_efforts(&models, model.as_deref());
        let mut effort_note = None;
        let effort = if rerouted_from.is_some() {
            None
        } else if let Some(e) = participant.effort {
            Some(e)
        } else {
            match self.settings.default_efforts.get(provider.id()).copied() {
                Some(d) if supported.contains(&d) => Some(d),
                Some(d) => {
                    effort_note = Some(format!(
                        "預設推理強度「{}」不適用於 {}（{}），這一則改用模型預設。",
                        d.as_str(),
                        provider.display_name(),
                        model.clone().unwrap_or_else(|| "CLI 預設模型".into())
                    ));
                    None
                }
                None => None,
            }
        };
        let model_id = model.clone().unwrap_or_default();
        let effective_effort = effort.or_else(|| models.iter().find(|m| m.id == model_id).and_then(|m| m.default_effort));
        if let Some(n) = effort_note {
            self.note(NoteLevel::Info, n).await;
        }

        let (turn, prompt, compaction_note) = {
            let mut r = self.ctl.record.lock().await;
            let labels = self.spec.blind.then(|| Self::blind_labels(&r.turns));
            let entries: Vec<TranscriptEntry> = r.turns[..visible.min(r.turns.len())]
                .iter()
                .filter(|t| t.ended_at.is_some())
                .map(|t| {
                    let who = match &labels {
                        Some(l) => format!("成員 {}", l.get(&Self::blind_key(t)).copied().unwrap_or('?')),
                        None => format!("{}{}", t.family.label(), t.model.as_deref().map(|m| format!(" · {m}")).unwrap_or_default()),
                    };
                    TranscriptEntry {
                        index: t.index,
                        header: format!("第 {} 輪｜{}（{}）", t.round, who, t.role.label()),
                        text: if t.ok { t.text.clone() } else { format!("（此發言失敗：{}）", t.error.clone().unwrap_or_default()) },
                    }
                })
                .collect();
            let keep_recent = self.spec.participants.len().max(2);
            let compacted = compact::render(&entries, self.settings.transcript_budget_chars, keep_recent);
            let interjections: Vec<String> = r.interjections.iter().map(|i| i.text.clone()).collect();
            if blocked.is_none() {
                for i in r.interjections.iter_mut() {
                    i.delivered = true;
                }
            }
            let approved_plan = r.plan.as_ref().filter(|p| p.approved).map(|p| p.text.clone());
            let use_plan = matches!(role, Role::Builder | Role::Reviewer) && self.spec.mode == Mode::Workshop && self.spec.stage.is_none();
            let prompt = prompts::build(&PromptContext {
                project: &self.project,
                spec: &self.spec,
                family: provider.family(),
                role,
                round,
                access,
                transcript: &compacted.text,
                interjections: &interjections,
                ask: &ask,
                snapshot: self.snapshot.as_deref().filter(|_| PLANNING_ROLES.contains(&role)),
                plan: approved_plan.as_deref().filter(|_| use_plan),
            });
            let turn = Turn {
                index: r.turns.len() as u32,
                round,
                role,
                provider: provider.id().to_string(),
                family: provider.family(),
                model: model.clone(),
                started_at: now_ms(),
                ended_at: None,
                text: String::new(),
                ok: false,
                error: None,
                usage: None,
                tools: vec![],
                delta_seq: 0,
                context_compacted: compacted.was_compacted(),
                seeded: false,
                rerouted_from: rerouted_from.clone(),
                effort,
                effective_effort,
                team_roles,
            };
            r.turns.push(turn.clone());
            r.updated_at = now_ms();
            let note = compacted.was_compacted().then(|| {
                let total_orig: usize = compacted.compressed.iter().map(|c| c.original_chars).sum();
                let total_kept: usize = compacted.compressed.iter().map(|c| c.kept_chars).sum();
                format!(
                    "記憶壓縮：第 {} 則發言的上下文壓縮了 {} 則較早發言（{} 字 → {} 字）{}；全文仍保留在紀錄中。",
                    turn.index + 1,
                    compacted.compressed.len(),
                    total_orig,
                    total_kept,
                    if compacted.dropped > 0 { format!("，另省略最早 {} 則", compacted.dropped) } else { String::new() }
                )
            });
            (turn, prompt, note)
        };
        let idx = turn.index as usize;
        self.engine.emit(Event::TurnStarted { session_id: self.session_id.clone(), turn: turn.clone() });
        if let Some(n) = compaction_note {
            self.note(NoteLevel::Info, n).await;
        }

        let out = if let Some(reason) = blocked {
            self.note(NoteLevel::Guard, format!("額度保護：{reason}，本輪不呼叫 {}。", provider.family().label())).await;
            TurnOutput::fail(format!("額度保護：{reason}（可在設定調整門檻或開啟自動改派）"))
        } else {
            let (tx, mut rx) = mpsc::unbounded_channel();
            let req = TurnRequest {
                prompt,
                model: model.clone(),
                cwd: self.cwd(),
                access,
                timeout: Duration::from_secs(self.settings.turn_timeout_secs.max(30)),
                cancel: self.ctl.cancel.child_token(),
                effort,
                clean_config: self.settings.isolate_cli_config,
            };
            let prov = provider.clone();
            let handle = tokio::spawn(async move { prov.run(req, tx).await });
            while let Some(ev) = rx.recv().await {
                match ev {
                    ProviderEvent::Delta(t) => {
                        let seq = {
                            let mut r = self.ctl.record.lock().await;
                            let tr = &mut r.turns[idx];
                            tr.text.push_str(&t);
                            tr.delta_seq += 1;
                            tr.delta_seq
                        };
                        self.engine.emit(Event::Delta { session_id: self.session_id.clone(), index: idx as u32, seq, text: t });
                    }
                    ProviderEvent::Tool(n) => {
                        self.ctl.record.lock().await.turns[idx].tools.push(n.clone());
                        self.engine.emit(Event::Tool { session_id: self.session_id.clone(), index: idx as u32, note: n });
                    }
                    ProviderEvent::Quota(q) => self.engine.record_quota(q),
                    ProviderEvent::Warning(w) => {
                        self.note(NoteLevel::Warn, format!("{}：{}", provider.family().label(), compact::truncate_chars(&w, 300))).await
                    }
                }
            }
            handle.await.unwrap_or_else(|e| TurnOutput::fail(format!("內部錯誤：{e}")))
        };

        if out.unsupported_model {
            if let Some(m) = out.rejected_model.clone().or_else(|| model.clone()) {
                self.engine.mark_unsupported(provider.id(), &m);
                let hint = if model.is_none() {
                    "這是 CLI 設定檔裡的預設模型；請到「設定 → 預設模型」選一個你的方案能用的模型。"
                } else {
                    "模型選單會標示出來。"
                };
                self.note(NoteLevel::Warn, format!("已記下：你的 {} 方案不支援模型 {m}。{hint}", provider.family().label())).await;
            }
        }
        if out.guard_tripped {
            self.note(NoteLevel::Guard, out.error.clone().unwrap_or_default()).await;
        }
        let finished = {
            let mut r = self.ctl.record.lock().await;
            let tr = &mut r.turns[idx];
            tr.ended_at = Some(now_ms());
            tr.ok = out.ok;
            tr.usage = out.usage.clone();
            if out.ok {
                if !out.text.trim().is_empty() {
                    tr.text = out.text.clone();
                }
            } else {
                tr.error = out.error.clone().or_else(|| Some("未知錯誤".into()));
                if tr.text.trim().is_empty() {
                    tr.text = out.text.clone();
                }
            }
            r.updated_at = now_ms();
            r.turns[idx].clone()
        };
        self.engine.emit(Event::TurnEnded { session_id: self.session_id.clone(), turn: finished.clone() });
        self.persist().await;
        self.check_cancel()?;
        Ok(finished)
    }

    /// Run turns concurrently; all of them see the same transcript (round independence).
    pub async fn parallel(self: &Arc<Self>, plans: Vec<TurnPlan>) -> Result<Vec<Turn>> {
        let visible = self.visible_now().await;
        let mut set = JoinSet::new();
        for (i, plan) in plans.into_iter().enumerate() {
            let me = self.clone();
            set.spawn(async move { (i, me.turn(plan, visible).await) });
        }
        let mut results: Vec<(usize, Result<Turn>)> = Vec::new();
        while let Some(r) = set.join_next().await {
            results.push(r.map_err(|e| anyhow!("{e}"))?);
        }
        results.sort_by_key(|(i, _)| *i);
        results.into_iter().map(|(_, r)| r).collect()
    }

    pub async fn verify(&self, command: &str, round: u32) -> Result<VerifyRun> {
        self.check_cancel()?;
        let run = run_verify(command, &self.cwd(), round, Duration::from_secs(self.settings.verify_timeout_secs.max(10)), &self.ctl.cancel).await;
        self.ctl.record.lock().await.verify_runs.push(run.clone());
        self.engine.emit(Event::Verify { session_id: self.session_id.clone(), run: run.clone() });
        self.persist().await;
        self.check_cancel()?;
        Ok(run)
    }

    pub async fn set_verdict(&self, mut v: Verdict) {
        if v.memory_hint.is_none() {
            // Newest successful turn of this session that offered a `記憶：` line.
            let r = self.ctl.record.lock().await;
            v.memory_hint = r.turns.iter().rev().filter(|t| t.ok && !t.seeded).find_map(|t| prompts::extract_memory(&t.text));
        }
        self.ctl.record.lock().await.verdict = Some(v.clone());
        self.engine.emit(Event::Verdict { session_id: self.session_id.clone(), verdict: v });
        self.persist().await;
    }

    pub async fn set_plan(&self, mut plan: Plan) {
        plan.blocks = prompts::plan_blocks(&plan.text);
        self.ctl.record.lock().await.plan = Some(plan.clone());
        self.engine.emit(Event::Plan { session_id: self.session_id.clone(), plan });
        self.persist().await;
    }

    pub async fn current_plan(&self) -> Plan {
        self.ctl.record.lock().await.plan.clone().unwrap_or_default()
    }

    /// Pause at the approval gate until the user decides (or the session is cancelled).
    pub async fn await_gate(self: &Arc<Self>) -> Result<GateDecision> {
        let (tx, rx) = oneshot::channel();
        *self.ctl.gate.lock().await = Some(tx);
        self.set_status(Status::AwaitingApproval, "等待核准").await;
        let decision = tokio::select! {
            d = rx => d.map_err(|_| anyhow!("核准通道已關閉"))?,
            _ = self.ctl.cancel.cancelled() => return Err(Stop::Cancelled.into()),
        };
        self.set_status(Status::Running, "已收到決定").await;
        Ok(decision)
    }

    /// Before the first write turn: an isolated git worktree when possible, else the project
    /// directory itself — either way recorded and announced.
    pub async fn ensure_workspace(&self) {
        if let Some(ws) = self.ctl.record.lock().await.workspace.clone() {
            *self.workdir.lock().unwrap() = PathBuf::from(&ws.path);
            return;
        }
        let project = PathBuf::from(&self.project.path);
        let (ws, note) = if !self.spec.isolate {
            (None, (NoteLevel::Warn, "已關閉隔離：模型會直接修改專案資料夾。".to_string()))
        } else if !git::is_repo(&project).await {
            (None, (NoteLevel::Warn, "專案不是 git repo，無法隔離：模型會直接修改專案資料夾。".to_string()))
        } else {
            let root = self.engine.store.worktrees_dir().join(&self.session_id);
            let branch = format!("agora/{}", self.session_id);
            match git::create_worktree(&project, &root, &branch).await {
                Ok(wt) => (
                    Some(Workspace {
                        kind: WorkspaceKind::Worktree,
                        path: wt.workdir.display().to_string(),
                        root: Some(wt.root.display().to_string()),
                        branch: Some(wt.branch.clone()),
                        base: Some(wt.base.clone()),
                        state: WorkspaceState::Active,
                        message: None,
                    }),
                    (
                        NoteLevel::Info,
                        format!(
                            "隔離工作區：在 git worktree（分支 {}，以 HEAD {} 為基準）實作；你的工作目錄不會被改動，未 commit 的修改也不包含在內。完成後檢查 diff 再決定是否套用。",
                            wt.branch,
                            &wt.base[..wt.base.len().min(8)]
                        ),
                    ),
                ),
                Err(e) => (None, (NoteLevel::Warn, format!("無法建立隔離工作區（{e:#}），改為直接修改專案資料夾。"))),
            }
        };
        let ws = ws.unwrap_or(Workspace {
            kind: WorkspaceKind::Direct,
            path: self.project.path.clone(),
            root: None,
            branch: None,
            base: None,
            state: WorkspaceState::Active,
            message: None,
        });
        *self.workdir.lock().unwrap() = PathBuf::from(&ws.path);
        self.ctl.record.lock().await.workspace = Some(ws.clone());
        self.note(note.0, note.1).await;
        self.engine.emit(Event::Workspace { session_id: self.session_id.clone(), workspace: ws });
        self.persist().await;
    }

    /// At the end of a session with an active worktree: summarize the diff in the log.
    async fn workspace_summary_note(&self) {
        let ws = self.ctl.record.lock().await.workspace.clone();
        let Some(ws) = ws.filter(|w| w.kind == WorkspaceKind::Worktree && w.state == WorkspaceState::Active) else { return };
        let (Some(root), Some(base)) = (ws.root.as_deref(), ws.base.as_deref()) else { return };
        match git::diff(std::path::Path::new(root), base).await {
            Ok(d) if d.files.is_empty() => self.note(NoteLevel::Info, "隔離工作區沒有任何變更。".into()).await,
            Ok(d) => {
                self.note(
                    NoteLevel::Info,
                    format!("隔離工作區共變更 {} 個檔案（+{} −{}）。檢查 diff 後可「套用到專案」或「丟棄」。", d.files.len(), d.added, d.removed),
                )
                .await
            }
            Err(e) => self.note(NoteLevel::Warn, format!("無法計算 diff：{e:#}")).await,
        }
    }
}
