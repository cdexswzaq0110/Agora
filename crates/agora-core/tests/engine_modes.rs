//! End-to-end engine tests against the deterministic mock providers — no CLI, no quota, no
//! network.

use agora_core::digest::{DigestItem, SourceStatus};
use agora_core::engine::GateDecision;
use agora_core::provider::mock::MockProvider;
use agora_core::provider::Provider;
use agora_core::scheduler::Schedule;
use agora_core::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn temp_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("agora-test-{tag}-{}", random_hex(4)))
}

fn engine(tag: &str) -> Arc<Engine> {
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(MockProvider::new("mock-claude", Family::Claude).with_delay(Duration::from_millis(3))),
        Arc::new(MockProvider::new("mock-gpt", Family::Gpt).with_delay(Duration::from_millis(3))),
    ];
    Engine::with_providers(Store::open(temp_dir(tag)).unwrap(), providers).unwrap()
}

fn p(id: &str) -> Participant {
    Participant::new(id)
}

fn spec(e: &Engine, mode: Mode, parts: Vec<Participant>, prompt: &str) -> SessionSpec {
    SessionSpec::new(&e.projects()[0].id, mode, parts, prompt)
}

fn both() -> Vec<Participant> {
    vec![p("mock-claude"), p("mock-gpt")]
}

async fn wait_until(e: &Engine, id: &str, done: impl Fn(&SessionRecord) -> bool) -> SessionRecord {
    for _ in 0..800 {
        let r = e.session(id).await.unwrap();
        if done(&r) {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    panic!("session {id} did not reach the expected state");
}

async fn wait_done(e: &Engine, id: &str) -> SessionRecord {
    wait_until(e, id, |r| !r.status.is_live()).await
}

async fn wait_gate(e: &Engine, id: &str) -> SessionRecord {
    wait_until(e, id, |r| r.status == Status::AwaitingApproval || !r.status.is_live()).await
}

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git").current_dir(dir).args(args).status().unwrap().success();
    assert!(ok, "git {args:?}");
}

fn git_repo(tag: &str) -> PathBuf {
    let d = temp_dir(tag);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("README.md"), "# demo\n").unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["add", "-A"]);
    git(&d, &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"]);
    d
}

// ------------------------------------------------------------------ v0.1 modes

#[tokio::test]
async fn relay_revises_once_then_approves_and_is_only_inferred_without_verify() {
    let e = engine("relay");
    let rec = e.start(spec(&e, Mode::Relay, both(), "做一個登入頁")).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert_eq!(r.status, Status::Done, "{:?}", r.error);
    let roles: Vec<Role> = r.turns.iter().map(|t| t.role).collect();
    assert_eq!(roles, vec![Role::Builder, Role::Reviewer, Role::Builder, Role::Reviewer]);
    let v = r.verdict.unwrap();
    assert_eq!(v.outcome, "approved");
    assert_eq!(v.grade, EvidenceGrade::Inferred);
}

#[tokio::test]
async fn relay_with_passing_verify_is_confirmed() {
    let e = engine("relay-verify");
    let mut s = spec(&e, Mode::Relay, vec![p("mock-gpt"), p("mock-claude")], "修 bug");
    s.verify_command = Some("echo tests-pass".into());
    let rec = e.start(s).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert_eq!(r.status, Status::Done, "{:?}", r.error);
    assert!(!r.verify_runs.is_empty() && r.verify_runs.iter().all(|v| v.ok));
    assert_eq!(r.verdict.unwrap().grade, EvidenceGrade::Confirmed);
}

#[tokio::test]
async fn relay_with_failing_verify_stops_at_the_bound_and_says_so() {
    let e = engine("relay-fail");
    let mut s = spec(&e, Mode::Relay, vec![p("mock-gpt"), p("mock-claude")], "修 bug");
    s.verify_command = Some("exit 1".into());
    s.rounds = Some(1);
    let rec = e.start(s).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    let v = r.verdict.unwrap();
    assert_eq!(v.outcome, "verify_failed");
    assert_eq!(v.grade, EvidenceGrade::Unverified);
    assert!(v.open_issues.iter().any(|i| i.contains("驗證")));
    assert!(r.notes.iter().any(|n| n.level == NoteLevel::Guard && n.text.contains("修訂上限")));
}

#[tokio::test]
async fn roundtable_first_round_is_blind_and_synthesis_is_capped_at_inferred() {
    let e = engine("roundtable");
    let rec = e.start(spec(&e, Mode::Roundtable, both(), "要不要用微服務")).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert_eq!(r.status, Status::Done, "{:?}", r.error);
    assert_eq!(r.turns.len(), 5);
    assert_eq!(r.turns.last().unwrap().role, Role::Synthesizer);
    let v = r.verdict.unwrap();
    assert_eq!(v.outcome, "synthesized");
    assert!(v.grade <= EvidenceGrade::Inferred);
    assert!(!v.open_issues.is_empty());
}

#[tokio::test]
async fn debate_cross_judging_agreement_and_split() {
    let e = engine("debate");
    let rec = e.start(spec(&e, Mode::Debate, both(), "SQL 還是 NoSQL")).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert_eq!(r.status, Status::Done, "{:?}", r.error);
    assert_eq!(r.turns.iter().filter(|t| t.role == Role::Judge).count(), 2);
    let v = r.verdict.unwrap();
    assert_eq!(v.outcome, "judged");
    assert!(v.headline.contains("交叉裁判一致"));
    assert!(!v.open_issues.is_empty());
    assert!(v.open_issues.iter().all(|i| !i.contains("勝方") && !i.contains("證據等級")), "{:?}", v.open_issues);

    let rec = e.start(spec(&e, Mode::Debate, both(), "[mock:split] 要不要重寫")).await.unwrap();
    let v = wait_done(&e, &rec.id).await.verdict.unwrap();
    assert_eq!(v.outcome, "split");
    assert_eq!(v.grade, EvidenceGrade::Candidate);
}

#[tokio::test]
async fn turn_budget_is_enforced() {
    let e = engine("budget");
    let mut s = spec(&e, Mode::Roundtable, both(), "預算測試");
    s.max_turns = Some(3);
    let rec = e.start(s).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert_eq!(r.turns.len(), 3, "never more CLI calls than the budget");
    assert!(r.notes.iter().any(|n| n.level == NoteLevel::Guard && n.text.contains("上限")));
}

#[tokio::test]
async fn cancel_stops_a_running_session() {
    let e = engine("cancel");
    let rec = e.start(spec(&e, Mode::Solo, vec![p("mock-claude")], "[mock:slow] 慢慢來")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(e.cancel(&rec.id).await);
    assert_eq!(wait_done(&e, &rec.id).await.status, Status::Cancelled);
}

fn block(e: &Engine, provider: &str) {
    e.record_quota(QuotaSnapshot {
        provider: provider.into(),
        family: if provider.contains("claude") { Family::Claude } else { Family::Gpt },
        plan: None,
        status: QuotaStatus::Warning,
        windows: vec![QuotaWindow {
            key: "five_hour".into(),
            label: "5 小時".into(),
            used_percent: 99.0,
            resets_at: Some(now_ms() / 1000 + 3600),
            window_minutes: Some(300),
        }],
        source: "test".into(),
        observed_at: now_ms(),
    });
}

#[tokio::test]
async fn quota_guard_refuses_without_calling_the_cli_when_reroute_is_off() {
    let e = engine("quota");
    e.update_settings(|s| s.auto_reroute = false).unwrap();
    block(&e, "mock-gpt");
    let rec = e.start(spec(&e, Mode::Solo, vec![p("mock-gpt")], "問題")).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert_eq!(r.status, Status::Failed);
    let t = &r.turns[0];
    assert!(!t.ok && t.error.as_deref().unwrap().contains("額度保護"));
    assert!(t.usage.is_none(), "no CLI call was made");
}

#[tokio::test]
async fn quota_guard_reroutes_to_the_other_family() {
    let e = engine("reroute");
    block(&e, "mock-gpt");
    let rec = e.start(spec(&e, Mode::Solo, vec![p("mock-gpt")], "問題")).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert_eq!(r.status, Status::Done, "{:?}", r.error);
    let t = &r.turns[0];
    assert_eq!(t.provider, "mock-claude");
    assert_eq!(t.rerouted_from.as_deref(), Some("mock-gpt"));
    assert!(r.notes.iter().any(|n| n.text.contains("代打")));
}

#[tokio::test]
async fn interjection_reaches_the_next_prompt() {
    let e = engine("interject");
    let mut s = spec(&e, Mode::Roundtable, both(), "插話測試");
    s.rounds = Some(2);
    let rec = e.start(s).await.unwrap();
    e.interject(&rec.id, "請考慮成本").await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert!(r.interjections[0].delivered);
    assert!(r.turns.iter().any(|t| t.text.contains("已納入使用者插話")));
}

#[tokio::test]
async fn remember_appends_verdict_to_project_memory() {
    let e = engine("memory");
    let rec = e.start(spec(&e, Mode::Debate, both(), "記憶測試")).await.unwrap();
    wait_done(&e, &rec.id).await;
    let proj = e.remember(&rec.id, None).await.unwrap();
    assert!(proj.memory.contains("記憶測試") && proj.memory.contains("推論"));
}

#[tokio::test]
async fn validation_rejects_bad_specs() {
    let e = engine("validate");
    assert!(e.start(spec(&e, Mode::Roundtable, vec![p("mock-claude")], "x")).await.is_err());
    assert!(e.start(spec(&e, Mode::Solo, vec![p("nope")], "x")).await.is_err());
    assert!(e.start(spec(&e, Mode::Solo, vec![p("mock-claude")], "   ")).await.is_err());
    assert!(e.start(spec(&e, Mode::Workshop, vec![], "x")).await.is_err());
}

// ------------------------------------------------------------------ v0.2 workshop

#[tokio::test]
async fn workshop_waits_for_approval_then_builds_in_an_isolated_worktree() {
    let e = engine("workshop");
    let repo = git_repo("ws-repo");
    let proj = e.create_project("repo", &repo.display().to_string()).unwrap();
    let mut s = SessionSpec::new(&proj.id, Mode::Workshop, both(), "[mock:write] 加一個新功能");
    s.verify_command = Some("echo ok".into());
    let rec = e.start(s).await.unwrap();

    let r = wait_gate(&e, &rec.id).await;
    assert_eq!(r.status, Status::AwaitingApproval, "{:?}", r.error);
    let roles: Vec<Role> = r.turns.iter().map(|t| t.role).collect();
    assert_eq!(&roles[..2], &[Role::Planner, Role::Planner]);
    assert_eq!(&roles[2..4], &[Role::Critic, Role::Critic]);
    assert_eq!(roles[4], Role::Synthesizer);
    let plan = r.plan.clone().unwrap();
    assert!(plan.text.contains("驗收標準") && !plan.approved);
    assert!(r.workspace.is_none(), "nothing is written before approval");

    // Approve with an edited plan.
    let edited = format!("{}\n- 額外要求：加上註解", plan.text);
    e.decide(&rec.id, GateDecision::Approve { plan: Some(edited.clone()), verify_command: None }).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert_eq!(r.status, Status::Done, "{:?}", r.error);
    let plan = r.plan.clone().unwrap();
    assert!(plan.approved && plan.edited && plan.text.contains("額外要求"));
    let v = r.verdict.clone().unwrap();
    assert_eq!(v.outcome, "approved");
    assert_eq!(v.grade, EvidenceGrade::Confirmed, "verify passed + reviewer approved");
    assert!(v.headline.contains("驗收標準"));

    // The project itself is untouched until applied.
    let ws = r.workspace.clone().unwrap();
    assert_eq!(ws.kind, WorkspaceKind::Worktree);
    assert!(!repo.join("agora-mock.txt").exists());
    let diff = e.workspace_diff(&rec.id).await.unwrap();
    assert!(diff.files.iter().any(|f| f.path.ends_with("agora-mock.txt")), "{:?}", diff.files);

    let r = e.apply_workspace(&rec.id).await.unwrap();
    assert_eq!(r.workspace.unwrap().state, WorkspaceState::Applied);
    assert!(repo.join("agora-mock.txt").exists(), "applied as an uncommitted change");
    assert!(e.apply_workspace(&rec.id).await.is_err(), "cannot apply twice");
}

#[tokio::test]
async fn workshop_revise_then_reject_keeps_the_plan_without_building() {
    let e = engine("ws-revise");
    let rec = e.start(spec(&e, Mode::Workshop, both(), "規劃一個快取層")).await.unwrap();
    wait_gate(&e, &rec.id).await;
    e.decide(&rec.id, GateDecision::Revise { feedback: "更簡單一點".into(), comments: vec![], plan_revision: None }).await.unwrap();
    let r = wait_until(&e, &rec.id, |r| r.plan.as_ref().is_some_and(|p| p.revision == 1) && r.status == Status::AwaitingApproval).await;
    assert_eq!(r.plan.as_ref().unwrap().feedback, vec!["更簡單一點".to_string()]);
    e.decide(&rec.id, GateDecision::Reject { reason: Some("先不做".into()) }).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert_eq!(r.status, Status::Done);
    assert_eq!(r.verdict.unwrap().outcome, "planned");
    assert!(r.turns.iter().all(|t| t.role != Role::Builder));
    assert!(r.workspace.is_none());
}

#[tokio::test]
async fn workshop_plan_only_auto_approve_with_red_team() {
    let e = engine("ws-plan");
    let mut s = spec(&e, Mode::Workshop, both(), "寫一份規格");
    s.plan_only = true;
    s.auto_approve = true;
    s.red_team = true;
    let rec = e.start(s).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert_eq!(r.status, Status::Done, "{:?}", r.error);
    let syn = r.turns.iter().filter(|t| t.role == Role::Synthesizer).count();
    let critics = r.turns.iter().filter(|t| t.role == Role::Critic).count();
    assert_eq!(syn, 2, "synthesis + revision after red team");
    assert_eq!(critics, 3, "two cross-critiques + one red team");
    let v = r.verdict.unwrap();
    assert_eq!(v.outcome, "planned");
    assert!(v.open_issues.iter().any(|i| i.contains("測試")), "acceptance criteria listed: {:?}", v.open_issues);
}

#[tokio::test]
async fn non_git_write_sessions_are_direct_and_say_so() {
    let e = engine("direct");
    let mut s = spec(&e, Mode::Solo, vec![p("mock-claude")], "[mock:write] 改檔");
    s.access = Access::Write;
    let rec = e.start(s).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    assert_eq!(r.workspace.unwrap().kind, WorkspaceKind::Direct);
    assert!(r.notes.iter().any(|n| n.text.contains("不是 git repo")));
}

// ------------------------------------------------------------------ v0.2 helpers

#[tokio::test]
async fn fork_carries_the_earlier_transcript() {
    let e = engine("fork");
    let a = e.start(spec(&e, Mode::Debate, both(), "原始辯論")).await.unwrap();
    wait_done(&e, &a.id).await;
    let mut s = spec(&e, Mode::Roundtable, both(), "延續討論");
    s.seed = Some(Seed { session_id: a.id.clone(), upto: 1 });
    s.rounds = Some(1);
    let b = e.start(s).await.unwrap();
    let r = wait_done(&e, &b.id).await;
    let seeded: Vec<&Turn> = r.turns.iter().filter(|t| t.seeded).collect();
    assert_eq!(seeded.len(), 2);
    let new: Vec<&Turn> = r.turns.iter().filter(|t| !t.seeded).collect();
    assert_eq!(new.len(), 3);
    assert!(new.iter().all(|t| t.round > 1), "new rounds continue after the seeded ones");
    assert_eq!(SessionSummary::from(&r).turns, 3);
}

#[tokio::test]
async fn blind_sessions_hide_model_names_from_later_turns() {
    let e = engine("blind");
    let mut s = spec(&e, Mode::Roundtable, both(), "[mock:echo-headers] 盲審測試");
    s.blind = true;
    s.rounds = Some(2);
    let rec = e.start(s).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    let round2: Vec<&Turn> = r.turns.iter().filter(|t| t.round == 2).collect();
    assert!(round2.iter().all(|t| t.text.contains("成員 A") && !t.text.contains("Claude（與談人）")), "{}", round2[0].text);
}

#[tokio::test]
async fn search_finds_sessions_by_turn_text() {
    let e = engine("search");
    let a = e.start(spec(&e, Mode::Solo, vec![p("mock-claude")], "第一個問題")).await.unwrap();
    let b = e.start(spec(&e, Mode::Solo, vec![p("mock-gpt")], "Kubernetes 部署")).await.unwrap();
    wait_done(&e, &a.id).await;
    wait_done(&e, &b.id).await;
    let hits = e.sessions(None, Some("kubernetes")).await;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, b.id);
    assert_eq!(e.sessions(None, Some("模擬 claude")).await.len(), 1, "matches turn text too");
}

#[tokio::test]
async fn experiment_runs_modes_one_after_another() {
    let e = engine("experiment");
    let base = spec(&e, Mode::Solo, both(), "比較題");
    let x = e.start_experiment(base, vec![Mode::Solo, Mode::Debate, Mode::Workshop]).await.unwrap();
    let mut view = None;
    for _ in 0..800 {
        let v = e.experiment(&x.id).await.unwrap();
        if v.experiment.done {
            view = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let v = view.expect("experiment finished");
    assert_eq!(v.sessions.len(), 3, "{:?}", v.experiment.errors);
    assert!(v.sessions.iter().all(|s| s.group_id.as_deref() == Some(x.id.as_str()) && s.status == Status::Done));
    assert!(v.sessions.iter().any(|s| s.mode == Mode::Workshop && s.grade.is_some()), "workshop runs plan-only in experiments");
}

#[tokio::test]
async fn digest_is_summarized_by_a_ready_provider_or_falls_back() {
    let e = engine("digest");
    let items = vec![
        DigestItem { source: "hn".into(), source_label: "Hacker News".into(), title: "Show HN: Agents".into(), url: "https://a.dev".into(), score: Some(500), ..Default::default() },
        DigestItem { source: "github".into(), source_label: "GitHub Trending".into(), title: "x/y".into(), url: "https://github.com/x/y".into(), stars_today: Some(900), ..Default::default() },
    ];
    let sources = vec![SourceStatus { source: "hn".into(), label: "Hacker News".into(), ok: true, count: 1, error: None, ms: 1 }];
    let d = e.compose_digest("2026-11-16", items.clone(), sources.clone()).await.unwrap();
    assert!(d.llm_ok, "{:?}", d.llm_error);
    assert!(d.markdown.contains("今日三大重點"));
    assert_eq!(d.items[0].n, 1);
    assert!(e.digest("latest").is_some());

    e.update_settings(|s| s.digest.use_llm = false).unwrap();
    let d = e.compose_digest("2026-11-17", items, sources).await.unwrap();
    assert!(!d.llm_ok);
    assert!(d.markdown.contains("## Hacker News") && d.markdown.contains("未經模型整理"));
    assert!(e.compose_digest("2026-11-18", vec![], vec![]).await.is_err());
}

#[tokio::test]
async fn schedules_start_a_session_once_per_day() {
    let e = engine("schedule");
    let s = e
        .upsert_schedule(Schedule {
            id: String::new(),
            name: "每晚審查".into(),
            enabled: true,
            time: "7:5".into(),
            weekdays: vec![1, 9],
            spec: spec(&e, Mode::Solo, vec![p("mock-claude")], "審查今天的變更"),
            last_run: None,
        })
        .unwrap();
    assert_eq!(s.time, "07:05");
    assert_eq!(s.weekdays, vec![1]);
    let rec = e.run_schedule(&s.id, "2026-11-16").await.unwrap().unwrap();
    assert!(rec.title.contains("每晚審查"));
    assert_eq!(e.settings().schedules[0].last_run.as_deref(), Some("2026-11-16"));
    wait_done(&e, &rec.id).await;
}


#[tokio::test]
async fn effort_is_validated_recorded_and_defaulted() {
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(MockProvider::new("mock-claude", Family::Claude).with_delay(Duration::from_millis(3))),
        Arc::new(MockProvider::new("mock-gpt", Family::Gpt).with_delay(Duration::from_millis(3)).with_efforts(&[Effort::Low, Effort::High])),
    ];
    let e = Engine::with_providers(Store::open(temp_dir("effort")).unwrap(), providers).unwrap();

    // An effort the model does not list is a hard error (→ 400), never a silent downgrade.
    let mut bad = p("mock-gpt");
    bad.effort = Some(Effort::Xhigh);
    let err = e.start(spec(&e, Mode::Solo, vec![bad], "effort?")).await.unwrap_err().to_string();
    assert!(err.contains("不支援推理強度") && err.contains("low") && err.contains("high"), "{err}");

    // A listed effort is passed and recorded on the turn.
    let mut ok = p("mock-gpt");
    ok.effort = Some(Effort::High);
    let r = wait_done(&e, &e.start(spec(&e, Mode::Solo, vec![ok], "effort ok")).await.unwrap().id).await;
    assert_eq!(r.turns[0].effort, Some(Effort::High));
    assert_eq!(r.turns[0].effective_effort, Some(Effort::High));

    // No effort: nothing passed; the model's reported default is the effective one.
    let r = wait_done(&e, &e.start(spec(&e, Mode::Solo, vec![p("mock-claude")], "default")).await.unwrap().id).await;
    assert_eq!(r.turns[0].effort, None);
    assert_eq!(r.turns[0].effective_effort, Some(Effort::Medium));

    // A settings default the model does not list is skipped with a visible note.
    e.update_settings(|s| {
        s.default_efforts.insert("mock-gpt".into(), Effort::Max);
        s.default_efforts.insert("mock-claude".into(), Effort::Xhigh);
    })
    .unwrap();
    let r = wait_done(&e, &e.start(spec(&e, Mode::Solo, vec![p("mock-gpt")], "settings default")).await.unwrap().id).await;
    assert_eq!(r.turns[0].effort, None);
    assert!(r.notes.iter().any(|n| n.text.contains("不適用")), "{:?}", r.notes);
    let r = wait_done(&e, &e.start(spec(&e, Mode::Solo, vec![p("mock-claude")], "settings default ok")).await.unwrap().id).await;
    assert_eq!(r.turns[0].effort, Some(Effort::Xhigh));

    // Old JSON (before v0.4) still loads.
    let old: Participant = serde_json::from_str(r#"{"provider":"codex","model":null}"#).unwrap();
    assert_eq!(old.effort, None);
    assert!(!serde_json::to_string(&old).unwrap().contains("effort"), "None is not written");
    let s: Settings = serde_json::from_str(r#"{"port":4717}"#).unwrap();
    assert!(s.default_efforts.is_empty());
    assert_eq!(Effort::parse(" XHigh "), Some(Effort::Xhigh));
    assert_eq!(Effort::parse("turbo"), None);
}


#[tokio::test]
async fn plan_section_comments_are_validated_and_reach_the_synthesizer() {
    let e = engine("comments");
    let mut s = spec(&e, Mode::Workshop, both(), "段落留言");
    s.plan_only = true;
    let rec = e.start(s).await.unwrap();
    let r = wait_gate(&e, &rec.id).await;
    assert_eq!(r.status, Status::AwaitingApproval);
    let plan = r.plan.clone().unwrap();
    assert!(plan.blocks.len() >= 3, "{:?}", plan.blocks);
    let (b0, b1) = (plan.blocks[0].clone(), plan.blocks[1].clone());
    let c = |b: &PlanBlock, text: &str| PlanComment { block_id: b.id.clone(), heading: b.heading.clone(), quote: String::new(), text: text.into() };
    let revise = |comments: Vec<PlanComment>, rev: Option<u32>, feedback: &str| GateDecision::Revise { feedback: feedback.into(), comments, plan_revision: rev };

    // Rejected before anything reaches the model: nothing at all, stale revision, unknown
    // section, too many, control characters.
    assert!(e.decide(&rec.id, revise(vec![], None, "  ")).await.is_err());
    assert!(e.decide(&rec.id, revise(vec![c(&b0, "x")], Some(plan.revision + 1), "")).await.unwrap_err().to_string().contains("已經更新"));
    assert!(e.decide(&rec.id, revise(vec![c(&b0, "x")], None, "")).await.is_err(), "comments need the revision they were written against");
    let mut ghost = c(&b0, "x");
    ghost.block_id = "不存在".into();
    assert!(e.decide(&rec.id, revise(vec![ghost], Some(plan.revision), "")).await.is_err());
    assert!(e.decide(&rec.id, revise((0..21).map(|_| c(&b0, "x")).collect(), Some(plan.revision), "")).await.is_err());
    assert!(e.decide(&rec.id, revise(vec![c(&b0, "a\u{0007}b")], Some(plan.revision), "")).await.is_err());
    assert_eq!(e.session(&rec.id).await.unwrap().status, Status::AwaitingApproval, "a rejected decision leaves the gate open");

    // Valid: two comments → one revision; both recorded on the plan's feedback history.
    e.decide(&rec.id, revise(vec![c(&b1, "第二段要具體"), c(&b0, "<script>x</script> 第一段")], Some(plan.revision), "")).await.unwrap();
    let r = wait_until(&e, &rec.id, |r| r.plan.as_ref().is_some_and(|p| p.revision == 1) && r.status != Status::Running).await;
    let fb = r.plan.as_ref().unwrap().feedback.join("\n");
    assert!(fb.contains(&format!("「{}」：第二段要具體", b1.heading)) && fb.contains("<script>x</script> 第一段"), "{fb}");
    assert!(!r.plan.as_ref().unwrap().blocks.is_empty(), "blocks recomputed for the new revision");

    // Old clients: feedback only, no comments.
    let old: GateDecision = serde_json::from_str(r#"{"action":"revise","feedback":"更短"}"#).unwrap();
    assert!(matches!(old, GateDecision::Revise { ref comments, plan_revision: None, .. } if comments.is_empty()));
}

#[tokio::test]
async fn digest_read_cursor_is_verified_monotonic_and_shared() {
    let e = engine("read");
    e.update_settings(|s| s.digest.use_llm = false).unwrap();
    let items = || vec![DigestItem { source: "hn".into(), source_label: "HN".into(), title: "A".into(), url: "https://a.dev".into(), score: Some(10), ..Default::default() }];
    let ok = || vec![SourceStatus { source: "hn".into(), label: "HN".into(), ok: true, count: 1, error: None, ms: 1 }];
    assert!(!e.digest_state().unread, "nothing stored yet");

    let older = e.compose_digest("2026-11-15", items(), ok()).await.unwrap();
    let d1 = e.compose_digest("2026-11-16", items(), ok()).await.unwrap();
    assert_eq!(d1.compared_with.as_deref(), Some("2026-11-15"), "trends computed against the earlier digest");
    let st = e.digest_state();
    assert_eq!((st.latest.as_deref(), st.unread), (Some("2026-11-16"), true));

    assert!(e.mark_digest_read("2026-11-16", d1.generated_at + 1).is_err(), "a version that is not stored is refused");
    assert!(e.mark_digest_read("../x", 1).is_err());
    assert!(!e.mark_digest_read("2026-11-16", d1.generated_at).unwrap().unread);

    // Reading an older day does not touch the latest.
    e.mark_digest_read("2026-11-15", older.generated_at).unwrap();
    assert!(!e.digest_state().unread);

    // Same-day regeneration → unread again; the old cursor is no longer accepted.
    tokio::time::sleep(Duration::from_millis(5)).await;
    let d2 = e.compose_digest("2026-11-16", items(), ok()).await.unwrap();
    assert!(d2.generated_at > d1.generated_at);
    assert!(e.digest_state().unread);
    assert!(e.mark_digest_read("2026-11-16", d1.generated_at).is_err());
    assert!(!e.mark_digest_read("2026-11-16", d2.generated_at).unwrap().unread);
}

#[tokio::test]
async fn memory_hint_is_suggested_and_custom_lines_are_cleaned() {
    let e = engine("memory");
    let rec = e.start(spec(&e, Mode::Roundtable, both(), "記憶建議")).await.unwrap();
    let r = wait_done(&e, &rec.id).await;
    let v = r.verdict.clone().unwrap();
    assert_eq!(v.memory_hint.as_deref(), Some("先上線最小版本，再依使用量擴充。"));

    assert!(e.remember(&rec.id, Some(" \n ")).await.is_err(), "blank is refused");
    assert!(e.remember(&rec.id, Some(&"長".repeat(301))).await.is_err());
    let p = e.remember(&rec.id, Some("  採用最小版本\n先上線  ")).await.unwrap();
    let last = p.memory.lines().last().unwrap().to_string();
    assert!(last.starts_with("- [") && last.contains("] 採用最小版本 先上線【") && last.ends_with('】'), "{last}");
    let p = e.remember(&rec.id, None).await.unwrap();
    assert!(p.memory.lines().last().unwrap().contains("（討論）"), "no line → the pre-v0.4 format");
}
