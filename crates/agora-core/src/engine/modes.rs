//! The five collaboration modes, written in terms of the runner primitives.
//!
//! - 單人 solo, 協做 relay, 討論 roundtable, 辯論 debate (v0.1)
//! - 方案工作坊 workshop (v0.2): proposals → critiques → synthesis (→ red team → revision) →
//!   human approval gate → build in an isolated worktree → verify → review against the plan's
//!   acceptance criteria. The relay mode and the workshop's build phase share one bounded
//!   build/verify/review loop.
//! - 專案團隊 (v0.5): a plan-only workshop with `spec.stage` runs `team_stage()` instead —
//!   role producers → integrator → the same approval gate (see team.rs).

use anyhow::{bail, Result};
use std::sync::Arc;

use super::runner::Runner;
use super::{GateDecision, MAX_PLAN_REVISIONS};
use crate::compact;
use crate::grade::EvidenceGrade;
use crate::model::*;
use crate::prompts::{self, ReviewDecision, Winner};
use crate::team;

pub(crate) fn rounds_for(spec: &SessionSpec) -> u32 {
    match spec.mode {
        Mode::Solo => 1,
        Mode::Relay | Mode::Workshop => spec.rounds.unwrap_or(1).min(3),
        Mode::Roundtable | Mode::Debate => spec.rounds.unwrap_or(2).clamp(1, 4),
    }
}

pub(crate) fn list_items(section: Option<&str>) -> Vec<String> {
    section
        .map(|s| {
            s.lines()
                .map(|l| l.trim().trim_start_matches(|c: char| "-*•0123456789.) ".contains(c)).trim().trim_matches('`').to_string())
                .filter(|l| !l.is_empty())
                .filter(|l| !(l.starts_with("勝方") || l.contains("證據等級") || l.to_ascii_uppercase().starts_with("VERDICT")))
                .take(10)
                .collect()
        })
        .unwrap_or_default()
}

/// Which loop flavour: plain relay, or implementing an approved workshop plan.
#[derive(Clone, Copy, PartialEq)]
enum BuildKind {
    Relay,
    Plan,
}

impl Runner {
    // ------------------------------------------------------------------ 單人

    pub(super) async fn solo(self: &Arc<Self>) -> Result<()> {
        if self.spec.access == Access::Write {
            self.ensure_workspace().await;
        }
        let p = &self.spec.participants[0];
        let t = self.turn(self.plan(p, Role::Solo, self.r(1), prompts::ask_solo(), self.spec.access), self.visible_now().await).await?;
        if !t.ok {
            bail!("模型發言失敗：{}", t.error.unwrap_or_default());
        }
        let headline = prompts::headline(&t.text, 120);
        let verdict = match &self.spec.verify_command {
            Some(cmd) => {
                let v = self.verify(cmd, t.round).await?;
                Verdict {
                    outcome: if v.ok { "verified" } else { "verify_failed" }.into(),
                    headline: if v.ok { "驗證通過".into() } else { "驗證失敗".into() },
                    grade: if v.ok { EvidenceGrade::Confirmed } else { EvidenceGrade::Unverified },
                    summary: headline,
                    open_issues: if v.ok { vec![] } else { vec![format!("`{}` 失敗（exit {:?}）", v.command, v.exit_code)] },
                    memory_hint: None,
                }
            }
            None => Verdict {
                outcome: "answered".into(),
                headline,
                grade: EvidenceGrade::Unverified,
                summary: "單一模型的回答，未經另一個模型審查，也未經測試驗證。".into(),
                open_issues: vec![],
                memory_hint: None,
            },
        };
        self.set_verdict(verdict).await;
        Ok(())
    }

    // ------------------------------------------------------------------ 協做

    pub(super) async fn relay(self: &Arc<Self>) -> Result<()> {
        if self.spec.access == Access::Write {
            self.ensure_workspace().await;
        }
        let builder = self.spec.participants[0].clone();
        let reviewer = self.spec.participants.get(1).cloned().unwrap_or_else(|| builder.clone());
        self.build_loop(BuildKind::Relay, &builder, &reviewer, self.spec.verify_command.clone(), self.spec.access).await
    }

    /// Build → (verify) → review, revising up to `rounds_for` times. At the bound, ship the
    /// current version and list what is still open — never loop further.
    async fn build_loop(
        self: &Arc<Self>,
        kind: BuildKind,
        builder: &Participant,
        reviewer: &Participant,
        verify_cmd: Option<String>,
        access: Access,
    ) -> Result<()> {
        let max_revisions = rounds_for(&self.spec);
        let first_ask = match kind {
            BuildKind::Relay => prompts::ask_builder_initial(),
            BuildKind::Plan => prompts::ask_build_from_plan(),
        };
        self.set_phase("實作").await;
        let mut round = self.next_round().await;
        let mut last_build = self.turn(self.plan(builder, Role::Builder, round, first_ask, access), self.visible_now().await).await?;
        if !last_build.ok {
            bail!("執行者發言失敗：{}", last_build.error.unwrap_or_default());
        }
        let mut revisions = 0;
        loop {
            let verify = match &verify_cmd {
                Some(c) => {
                    self.set_phase("驗證").await;
                    Some(self.verify(c, round).await?)
                }
                None => None,
            };
            self.set_phase("審查").await;
            let ask = match kind {
                BuildKind::Relay => prompts::ask_reviewer(verify.as_ref()),
                BuildKind::Plan => prompts::ask_review_against_plan(verify.as_ref()),
            };
            let review = self.turn(self.plan(reviewer, Role::Reviewer, round, ask, Access::ReadOnly), self.visible_now().await).await?;
            if !review.ok {
                bail!("審查者發言失敗：{}", review.error.unwrap_or_default());
            }
            let decision = prompts::parse_review(&review.text);
            let verify_ok = verify.as_ref().map(|v| v.ok);
            let summary = prompts::section(&last_build.text, "修訂摘要")
                .or_else(|| prompts::section(&last_build.text, "交付摘要"))
                .unwrap_or_else(|| prompts::headline(&last_build.text, 200));
            let against = if kind == BuildKind::Plan { "依核准方案的驗收標準" } else { "" };
            if decision == Some(ReviewDecision::Approve) && verify_ok != Some(false) {
                self.set_verdict(Verdict {
                    outcome: "approved".into(),
                    headline: format!(
                        "{against}審查通過{}",
                        match verify_ok {
                            Some(true) => "，驗證指令也通過",
                            _ => "（沒有設定驗證指令，屬於推論）",
                        }
                    ),
                    grade: if verify_ok == Some(true) { EvidenceGrade::Confirmed } else { EvidenceGrade::Inferred },
                    summary,
                    open_issues: prompts::extract_issues(&review.text),
                    memory_hint: None,
                })
                .await;
                return Ok(());
            }
            if decision.is_none() {
                self.note(NoteLevel::Warn, "審查者沒有給出 VERDICT 行，視為需要修訂。".into()).await;
            }
            if revisions >= max_revisions {
                let mut issues = prompts::extract_issues(&review.text);
                if kind == BuildKind::Plan {
                    issues.extend(review.text.lines().filter(|l| l.contains('❌')).map(|l| compact::truncate_chars(l.trim(), 160)).take(6));
                }
                if verify_ok == Some(false) {
                    issues.insert(0, "驗證指令仍然失敗".into());
                }
                self.note(NoteLevel::Guard, format!("已達修訂上限（{max_revisions} 次）——依設計交付目前版本，未解決的問題列在結論中。")).await;
                self.set_verdict(Verdict {
                    outcome: if verify_ok == Some(false) { "verify_failed" } else { "open_issues" }.into(),
                    headline: "已達修訂上限，仍有未解決的問題".into(),
                    grade: if verify_ok == Some(false) { EvidenceGrade::Unverified } else { EvidenceGrade::Candidate },
                    summary,
                    open_issues: issues,
                    memory_hint: None,
                })
                .await;
                return Ok(());
            }
            revisions += 1;
            self.set_phase("修訂實作").await;
            round = self.next_round().await;
            last_build = self
                .turn(self.plan(builder, Role::Builder, round, prompts::ask_builder_revise(verify.as_ref()), access), self.visible_now().await)
                .await?;
            if !last_build.ok {
                bail!("執行者修訂失敗：{}", last_build.error.unwrap_or_default());
            }
        }
    }

    // ------------------------------------------------------------------ 討論

    pub(super) async fn roundtable(self: &Arc<Self>) -> Result<()> {
        let rounds = rounds_for(&self.spec);
        for r in 1..=rounds {
            let plans = self.spec.participants.iter().map(|p| self.plan(p, Role::Panelist, self.r(r), prompts::ask_panelist(r), Access::ReadOnly)).collect();
            let turns = self.parallel(plans).await?;
            if turns.iter().all(|t| !t.ok) {
                bail!("第 {r} 輪所有與談人都失敗");
            }
        }
        let synth = self.spec.judge.clone().unwrap_or_else(|| self.spec.participants[0].clone());
        let s = self
            .turn(self.plan(&synth, Role::Synthesizer, self.r(rounds + 1), prompts::ask_synthesizer(), Access::ReadOnly), self.visible_now().await)
            .await?;
        if !s.ok {
            bail!("總結失敗：{}", s.error.unwrap_or_default());
        }
        let parsed = prompts::parse_grade(&s.text);
        if parsed == Some(EvidenceGrade::Confirmed) {
            self.note(NoteLevel::Info, "總結者自評「已確認」，但討論本身沒有直接觀察或測試，等級上限為「推論」。".into()).await;
        }
        let advice = prompts::section(&s.text, "建議");
        self.set_verdict(Verdict {
            outcome: "synthesized".into(),
            headline: advice.as_deref().map(|a| prompts::headline(a, 120)).unwrap_or_else(|| prompts::headline(&s.text, 120)),
            grade: parsed.unwrap_or(EvidenceGrade::Candidate).min(EvidenceGrade::Inferred),
            summary: advice.unwrap_or_else(|| compact::truncate_chars(&s.text, 600)),
            open_issues: list_items(prompts::section(&s.text, "待驗證").as_deref()),
            memory_hint: None,
        })
        .await;
        Ok(())
    }

    // ------------------------------------------------------------------ 辯論

    pub(super) async fn debate(self: &Arc<Self>) -> Result<()> {
        let pro = self.spec.participants[0].clone();
        let con = self.spec.participants.get(1).cloned().unwrap_or_else(|| pro.clone());
        let rounds = rounds_for(&self.spec);
        for r in 1..=rounds {
            let turns = self
                .parallel(vec![
                    self.plan(&pro, Role::Proponent, self.r(r), prompts::ask_proponent(r), Access::ReadOnly),
                    self.plan(&con, Role::Opponent, self.r(r), prompts::ask_opponent(r), Access::ReadOnly),
                ])
                .await?;
            if turns.iter().all(|t| !t.ok) {
                bail!("第 {r} 輪正反方都失敗");
            }
        }
        let judges: Vec<Participant> = match &self.spec.judge {
            Some(j) => vec![j.clone()],
            None => {
                let mut v = vec![pro.clone()];
                if con.provider != pro.provider {
                    v.push(con.clone());
                }
                v
            }
        };
        let cross = judges.len() > 1;
        let plans = judges.iter().map(|j| self.plan(j, Role::Judge, self.r(rounds + 1), prompts::ask_judge(cross), Access::ReadOnly)).collect();
        let rulings: Vec<Turn> = self.parallel(plans).await?.into_iter().filter(|t| t.ok).collect();
        if rulings.is_empty() {
            bail!("裁判發言失敗");
        }
        let winners: Vec<Option<Winner>> = rulings.iter().map(|t| prompts::parse_winner(&t.text)).collect();
        let grades: Vec<EvidenceGrade> = rulings.iter().filter_map(|t| prompts::parse_grade(&t.text)).collect();
        let first = &rulings[0];
        let advice = prompts::section(&first.text, "建議");
        let open_issues = list_items(prompts::section(&first.text, "待驗證").as_deref());
        let agreed = winners.iter().all(|w| w.is_some() && *w == winners[0]);
        let verdict = if agreed {
            let w = winners[0].unwrap();
            Verdict {
                outcome: "judged".into(),
                headline: format!("勝方：{}{}", w.label(), if cross { "（交叉裁判一致）" } else { "" }),
                grade: grades.iter().copied().min().unwrap_or(EvidenceGrade::Candidate).min(EvidenceGrade::Inferred),
                summary: advice.unwrap_or_else(|| compact::truncate_chars(&first.text, 600)),
                open_issues,
                memory_hint: None,
            }
        } else {
            let detail: Vec<String> = rulings
                .iter()
                .zip(&winners)
                .map(|(t, w)| format!("{} 判 {}", t.family.label(), w.map_or("（未表態）", |w| w.label())))
                .collect();
            Verdict {
                outcome: "split".into(),
                headline: format!("裁判意見分歧：{}", detail.join("、")),
                grade: EvidenceGrade::Candidate,
                summary: advice.unwrap_or_else(|| compact::truncate_chars(&first.text, 600)),
                open_issues,
                memory_hint: None,
            }
        };
        self.set_verdict(verdict).await;
        Ok(())
    }

    // ------------------------------------------------------------------ 方案工作坊

    fn synthesizer(&self) -> Participant {
        self.spec.judge.clone().unwrap_or_else(|| self.spec.participants[0].clone())
    }

    /// The planner whose provider differs from `other`, else the second planner, else `other`.
    fn counterpart(&self, other: &Participant) -> Participant {
        self.spec
            .participants
            .iter()
            .find(|p| p.provider != other.provider)
            .or_else(|| self.spec.participants.get(1))
            .cloned()
            .unwrap_or_else(|| other.clone())
    }

    pub(super) async fn workshop(self: &Arc<Self>) -> Result<()> {
        if let Some(st) = self.spec.stage.clone() {
            return self.team_stage(&st).await;
        }
        let synth = self.synthesizer();
        let resumed = self.resume.lock().unwrap().take();
        if resumed.is_none() {
            // 1. Independent proposals.
            self.set_phase("提案").await;
            let r1 = self.next_round().await;
            let plans = self.spec.participants.iter().map(|p| self.plan(p, Role::Planner, r1, prompts::ask_planner(), Access::ReadOnly)).collect();
            let proposals = self.parallel(plans).await?;
            if proposals.iter().all(|t| !t.ok) {
                bail!("所有提案者都失敗");
            }

            // 2. Cross-critique.
            self.set_phase("互評").await;
            let r2 = self.next_round().await;
            let plans = self.spec.participants.iter().map(|p| self.plan(p, Role::Critic, r2, prompts::ask_critic(), Access::ReadOnly)).collect();
            let critiques = self.parallel(plans).await?;
            if critiques.iter().all(|t| !t.ok) {
                self.note(NoteLevel::Warn, "互評全部失敗，直接以提案進行整合。".into()).await;
            }

            // 3. Synthesis.
            self.set_phase("整合").await;
            let r3 = self.next_round().await;
            let s = self.turn(self.plan(&synth, Role::Synthesizer, r3, prompts::ask_plan_synthesis(), Access::ReadOnly), self.visible_now().await).await?;
            if !s.ok {
                bail!("整合方案失敗：{}", s.error.unwrap_or_default());
            }
            let mut plan_text = prompts::extract_plan(&s.text);

            // 3b. Red team + one revision.
            if self.spec.red_team {
                self.set_phase("紅隊審視").await;
                let red = self.counterpart(&synth);
                let r4 = self.next_round().await;
                let rt = self.turn(self.plan(&red, Role::Critic, r4, prompts::ask_red_team(), Access::ReadOnly), self.visible_now().await).await?;
                if rt.ok {
                    self.set_phase("修訂方案").await;
                    let r5 = self.next_round().await;
                    let s2 = self
                        .turn(self.plan(&synth, Role::Synthesizer, r5, prompts::ask_plan_revise("紅隊的意見"), Access::ReadOnly), self.visible_now().await)
                        .await?;
                    if s2.ok {
                        plan_text = prompts::extract_plan(&s2.text);
                    }
                }
            }
            self.set_plan(Plan { text: plan_text, verify_command: self.spec.verify_command.clone(), ..Default::default() }).await;
        }

        // 4. Human approval gate.
        if !self.approval_loop(&synth, resumed).await? {
            return Ok(());
        }

        // 5–7. Build in an isolated worktree, verify, review against acceptance criteria.
        self.ensure_workspace().await;
        let builder = self.spec.builder.clone().unwrap_or_else(|| self.spec.participants[0].clone());
        let reviewer = self.spec.reviewer.clone().unwrap_or_else(|| self.counterpart(&builder));
        let verify_cmd = self.current_plan().await.verify_command.or_else(|| self.spec.verify_command.clone());
        if verify_cmd.is_none() {
            self.note(NoteLevel::Warn, "沒有設定驗證指令：審查通過最多只能算「推論」。".into()).await;
        }
        self.build_loop(BuildKind::Plan, &builder, &reviewer, verify_cmd, Access::Write).await
    }

    // ------------------------------------------------------------------ 專案團隊

    /// One team stage (v0.5, red-team #6): producers in parallel → integrator (review and
    /// write the document) → the workshop's approval gate. With a single producer and no
    /// integrator, the producer's output is the document. Revisions go to the stage owner.
    async fn team_stage(self: &Arc<Self>, st: &team::StageSpec) -> Result<()> {
        let owner = st.owner().participant.clone();
        let resumed = self.resume.lock().unwrap().take();
        if resumed.is_none() {
            self.set_phase("團隊產出").await;
            let r1 = self.next_round().await;
            let has_integrator = st.integrator.is_some();
            let plans = st
                .producers
                .iter()
                .map(|c| self.team_plan(&c.participant, Role::Planner, r1, team::ask_producer(st.kind, c, has_integrator), &c.roles))
                .collect();
            let outputs = self.parallel(plans).await?;
            if outputs.iter().all(|t| !t.ok) {
                bail!("這個階段的團隊成員全部失敗");
            }
            let doc_turn = match &st.integrator {
                Some(int) => {
                    self.set_phase("審查與整合").await;
                    let r2 = self.next_round().await;
                    let ask = team::ask_integrator(st.kind, int, &st.producers);
                    let t = self.turn(self.team_plan(&int.participant, Role::Synthesizer, r2, ask, &int.roles), self.visible_now().await).await?;
                    if !t.ok {
                        bail!("審查與整合失敗：{}", t.error.unwrap_or_default());
                    }
                    t
                }
                None => outputs.into_iter().next().filter(|t| t.ok).ok_or_else(|| anyhow::anyhow!("團隊成員發言失敗"))?,
            };
            self.set_plan(Plan { text: team::extract_stage_doc(&doc_turn.text), ..Default::default() }).await;
        }
        self.approval_loop(&owner, resumed).await?;
        Ok(())
    }

    /// Returns `true` to proceed to the build, `false` when the session ends with the plan.
    async fn approval_loop(self: &Arc<Self>, synth: &Participant, mut pending: Option<GateDecision>) -> Result<bool> {
        loop {
            let decision = match pending.take() {
                Some(d) => d,
                None if self.spec.auto_approve => GateDecision::Approve { plan: None, verify_command: None },
                None => self.await_gate().await?,
            };
            let mut plan = self.current_plan().await;
            match decision {
                GateDecision::Approve { plan: edited, verify_command } => {
                    if let Some(t) = edited.map(|t| t.trim().to_string()).filter(|t| !t.is_empty() && *t != plan.text.trim()) {
                        plan.text = t;
                        plan.edited = true;
                    }
                    if let Some(v) = verify_command.map(|v| v.trim().to_string()) {
                        plan.verify_command = (!v.is_empty()).then_some(v);
                    }
                    plan.approved = true;
                    plan.decided_at = Some(now_ms());
                    let rev = plan.revision + 1;
                    let edited = plan.edited;
                    self.set_plan(plan.clone()).await;
                    self.note(
                        NoteLevel::Info,
                        format!(
                            "方案已核准（第 {rev} 版{}{}）。",
                            if edited { "，經你修改" } else { "" },
                            if self.spec.auto_approve { "，自動核准" } else { "" }
                        ),
                    )
                    .await;
                    if self.spec.plan_only {
                        self.plan_verdict(&plan, "方案已核准（本議題只產出方案，不實作）").await;
                        return Ok(false);
                    }
                    return Ok(true);
                }
                GateDecision::Revise { feedback, comments, .. } => {
                    let request = prompts::revision_request(&feedback, &comments, &plan.blocks);
                    if request.trim().is_empty() {
                        continue;
                    }
                    let feedback = {
                        let mut f = feedback.trim().to_string();
                        for c in &comments {
                            let heading = plan.blocks.iter().find(|b| b.id == c.block_id).map(|b| b.heading.clone()).unwrap_or_else(|| c.heading.clone());
                            if !f.is_empty() {
                                f.push('\n');
                            }
                            f.push_str(&format!("「{heading}」：{}", c.text.trim()));
                        }
                        f
                    };
                    if plan.revision >= MAX_PLAN_REVISIONS {
                        self.note(NoteLevel::Guard, format!("已達方案修訂上限（{MAX_PLAN_REVISIONS} 次）：請直接修改文字後核准，或結束。")).await;
                        continue;
                    }
                    plan.feedback.push(feedback.clone());
                    self.set_plan(plan.clone()).await;
                    self.set_phase("修訂方案").await;
                    let r = self.next_round().await;
                    let turn_plan = match &self.spec.stage {
                        Some(st) => self.team_plan(synth, Role::Synthesizer, r, team::ask_revise(st, &request), &st.owner().roles),
                        None => self.plan(synth, Role::Synthesizer, r, prompts::ask_plan_revise(&request), Access::ReadOnly),
                    };
                    let s = self.turn(turn_plan, self.visible_now().await).await?;
                    if s.ok {
                        plan.text = match &self.spec.stage {
                            Some(_) => team::extract_stage_doc(&s.text),
                            None => prompts::extract_plan(&s.text),
                        };
                        plan.revision += 1;
                        self.set_plan(plan).await;
                    } else {
                        self.note(NoteLevel::Warn, "修訂方案失敗，保留上一版。".into()).await;
                    }
                }
                GateDecision::Reject { reason } => {
                    if let Some(r) = reason.filter(|r| !r.trim().is_empty()) {
                        self.note(NoteLevel::Info, format!("你決定不實作：{r}")).await;
                    }
                    self.plan_verdict(&plan, "方案保留，未實作").await;
                    return Ok(false);
                }
            }
        }
    }

    async fn plan_verdict(&self, plan: &Plan, headline: &str) {
        let synth_grade = {
            let r = self.ctl.record.lock().await;
            r.turns.iter().rev().find(|t| t.role == Role::Synthesizer && t.ok).and_then(|t| prompts::parse_grade(&t.text))
        };
        self.set_verdict(Verdict {
            outcome: "planned".into(),
            headline: headline.into(),
            grade: synth_grade.unwrap_or(EvidenceGrade::Candidate).min(EvidenceGrade::Inferred),
            summary: compact::truncate_chars(&plan.text, 1500),
            open_issues: list_items(prompts::section(&plan.text, "驗收標準").as_deref()),
            memory_hint: None,
        })
        .await;
    }
}
