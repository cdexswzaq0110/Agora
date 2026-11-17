//! v0.6 auditable review end to end (mock providers, real git worktrees): the contract sealed
//! at approval, typed spec verdicts, quality review only after every condition passed, format
//! retries, unverified conditions, version fingerprints and the call budget.
//! Acceptance criteria 3–6 of docs/design/2026-11-17-v0.6/05-final-plan.md.

use agora_core::engine::GateDecision;
use agora_core::provider::mock::MockProvider;
use agora_core::provider::Provider;
use agora_core::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn temp_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("agora-review-{tag}-{}", random_hex(4)))
}

fn engine(tag: &str) -> Arc<Engine> {
    let providers: Vec<Arc<dyn Provider>> = vec![
        Arc::new(MockProvider::new("mock-claude", Family::Claude).with_delay(Duration::from_millis(2))),
        Arc::new(MockProvider::new("mock-gpt", Family::Gpt).with_delay(Duration::from_millis(2))),
    ];
    Engine::with_providers(Store::open(temp_dir(tag)).unwrap(), providers).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    assert!(std::process::Command::new("git").current_dir(dir).args(args).status().unwrap().success(), "git {args:?}");
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

async fn wait_until(e: &Engine, id: &str, done: impl Fn(&SessionRecord) -> bool) -> SessionRecord {
    for _ in 0..1500 {
        let r = e.session(id).await.unwrap();
        if done(&r) {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("session {id} did not reach the expected state");
}

/// Start a workshop in `project`, approve its plan, return the finished record.
async fn run(e: &Arc<Engine>, project: &str, prompt: &str, verify: Option<&str>) -> SessionRecord {
    let mut s = SessionSpec::new(project, Mode::Workshop, vec![Participant::new("mock-claude"), Participant::new("mock-gpt")], prompt);
    s.verify_command = verify.map(String::from);
    let rec = e.start(s).await.unwrap();
    let r = wait_until(e, &rec.id, |r| r.status == Status::AwaitingApproval || !r.status.is_live()).await;
    assert_eq!(r.status, Status::AwaitingApproval, "{:?}", r.error);
    e.decide(&rec.id, GateDecision::Approve { plan: None, verify_command: None }).await.unwrap();
    let r = wait_until(e, &rec.id, |r| !r.status.is_live()).await;
    assert_eq!(r.status, Status::Done, "{:?}", r.error);
    r
}

fn stages(r: &SessionRecord) -> Vec<(ReviewStage, ReviewOutcome)> {
    r.reviews.iter().map(|x| (x.stage, x.outcome)).collect()
}

fn repo_project(e: &Engine, tag: &str) -> String {
    let repo = git_repo(tag);
    e.create_project("repo", &repo.display().to_string()).unwrap().id
}

#[tokio::test]
async fn contract_is_sealed_and_quality_runs_only_after_every_condition_passed() {
    let e = engine("happy");
    let pid = repo_project(&e, "happy-repo");
    let r = run(&e, &pid, "[mock:write] 加一個匯出功能", Some("echo ok")).await;

    let contract = r.plan.as_ref().unwrap().contract.clone().expect("sealed at approval");
    assert_eq!(contract.criteria.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["AC1", "AC2"]);
    assert!(r.reviews.iter().all(|x| x.contract_sha256 == contract.sha256));

    // Build 1: AC1 fails → revise (no quality review); build 2: all pass → one quality review.
    assert_eq!(
        stages(&r),
        vec![(ReviewStage::Spec, ReviewOutcome::Accepted), (ReviewStage::Spec, ReviewOutcome::Accepted), (ReviewStage::Quality, ReviewOutcome::Accepted)]
    );
    assert_eq!(r.reviews[0].verdicts.iter().find(|v| v.id == "AC1").unwrap().verdict, review::AcVerdict::Fail);
    assert!(r.reviews.iter().all(|x| x.fp_before.is_some() && x.fp_before == x.fp_after), "fingerprints recorded and equal");
    // Listing the changes stages new files (`git add -A`); the version must not look different.
    assert!(r.reviews.iter().all(|x| x.fp_verify == x.fp_before), "verify and review saw the same version: {:?}", r.reviews.iter().map(|x| (&x.fp_verify, &x.fp_before)).collect::<Vec<_>>());

    let v = r.verdict.clone().unwrap();
    assert_eq!(v.outcome, "approved");
    assert_eq!(v.grade, EvidenceGrade::Confirmed, "verify passed, same version, mock reviewer proven read-only");
    assert!(v.open_issues.iter().any(|i| i.starts_with("Q1【低】")), "{:?}", v.open_issues);
    // Reviewers saw no transcript: the spec prompt is self-contained.
    let reviewer_turns: Vec<&Turn> = r.turns.iter().filter(|t| t.role == Role::Reviewer).collect();
    assert_eq!(reviewer_turns.len(), 3);

    // Red-team #11: skills per workshop call, from a table written here (not `skills_for`).
    let ids = |t: &Turn| t.skills.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    let builders: Vec<&Turn> = r.turns.iter().filter(|t| t.role == Role::Builder).collect();
    assert_eq!(ids(builders[0]), vec!["se-minimal-implementation"]);
    assert_eq!(ids(builders[1]), vec!["se-minimal-implementation", "se-debug"]);
    assert_eq!(ids(reviewer_turns[0]), vec!["se-spec-review"]);
    assert_eq!(ids(reviewer_turns[2]), vec!["se-quality-review"]);
    for t in r.turns.iter().filter(|t| matches!(t.role, Role::Planner | Role::Synthesizer)) {
        assert_eq!(ids(t), vec!["se-work-order"], "turn {}", t.index);
    }
    assert!(r.turns.iter().filter(|t| t.role == Role::Critic).all(|t| t.skills.is_empty()));
}

#[tokio::test]
async fn an_unusable_answer_is_retried_once_then_handed_to_you() {
    let e = engine("garbage");
    let pid = repo_project(&e, "garbage-repo");
    let r = run(&e, &pid, "[mock:write] [mock:review-garbage] 加功能", Some("echo ok")).await;
    assert_eq!(stages(&r), vec![(ReviewStage::Spec, ReviewOutcome::Rejected), (ReviewStage::Spec, ReviewOutcome::Rejected)]);
    assert!(r.reviews[0].problems.iter().any(|p| p.contains("不是有效的 JSON")));
    let v = r.verdict.unwrap();
    assert_eq!(v.outcome, "review_unusable");
    assert_ne!(v.grade, EvidenceGrade::Confirmed);
}

#[tokio::test]
async fn unverifiable_conditions_stop_for_the_human_without_a_quality_review() {
    let e = engine("unverified");
    let pid = repo_project(&e, "unverified-repo");
    let r = run(&e, &pid, "[mock:write] [mock:review-unverified] [mock:review-pass] 加功能", Some("echo ok")).await;
    assert_eq!(stages(&r), vec![(ReviewStage::Spec, ReviewOutcome::Accepted)]);
    let v = r.verdict.unwrap();
    assert_eq!(v.outcome, "acceptance_unverified");
    assert!(v.open_issues[0].contains("AC1") && v.open_issues[0].contains("需要實機"));
}

#[tokio::test]
async fn a_high_quality_finding_sends_the_work_back() {
    let e = engine("high");
    let pid = repo_project(&e, "high-repo");
    let r = run(&e, &pid, "[mock:write] [mock:review-pass] [mock:quality-high] 加功能", Some("echo ok")).await;
    assert_eq!(
        stages(&r).iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        vec![ReviewStage::Spec, ReviewStage::Quality, ReviewStage::Spec, ReviewStage::Quality]
    );
    let fix = r.turns.iter().filter(|t| t.role == Role::Builder).nth(1).expect("a second build");
    assert!(fix.text.contains("修訂"));
    assert_eq!(r.verdict.unwrap().outcome, "approved");
}

#[tokio::test]
async fn a_change_during_review_voids_that_review() {
    let e = engine("touch");
    let pid = repo_project(&e, "touch-repo");
    let r = run(&e, &pid, "[mock:write] [mock:review-touch] [mock:review-pass] 加功能", Some("echo ok")).await;
    assert_eq!(r.reviews[0].outcome, ReviewOutcome::VersionChanged);
    assert_ne!(r.reviews[0].fp_before, r.reviews[0].fp_after);
    assert_eq!(r.verdict.unwrap().outcome, "review_unusable", "both attempts saw the tree change");
}

#[tokio::test]
async fn without_git_the_version_is_unknown_and_the_grade_tops_out_at_inferred() {
    let e = engine("nogit");
    let dir = temp_dir("nogit-dir");
    std::fs::create_dir_all(&dir).unwrap();
    let pid = e.create_project("plain", &dir.display().to_string()).unwrap().id;
    let r = run(&e, &pid, "[mock:review-pass] 加功能", Some("echo ok")).await;
    assert!(r.reviews.iter().all(|x| x.fp_before.is_none()));
    let v = r.verdict.unwrap();
    assert_eq!(v.outcome, "approved");
    assert_eq!(v.grade, EvidenceGrade::Inferred);
    assert!(r.notes.iter().any(|n| n.text.contains("沒有 git")));
}

#[tokio::test]
async fn the_call_budget_covers_the_worst_case_and_never_passes_the_hard_cap() {
    let e = engine("budget");
    let pid = repo_project(&e, "budget-repo");
    let mut s = SessionSpec::new(&pid, Mode::Workshop, vec![Participant::new("mock-claude"), Participant::new("mock-gpt"), Participant::new("mock-claude"), Participant::new("mock-gpt")], "[mock:write] 加功能");
    s.rounds = Some(3);
    s.red_team = true;
    let rec = e.start(s).await.unwrap();
    let budget_note = rec.notes.iter().find(|n| n.text.contains("最多")).unwrap().text.clone();
    let budget: u32 = budget_note.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap();
    assert!(budget <= 24, "{budget_note}");
    assert!(rec.notes.iter().any(|n| n.text.contains("修訂次數從 3 降為")), "{:?}", rec.notes);
    // The worst path (every review rejected twice) still fits: let it run to the end.
    let r = wait_until(&e, &rec.id, |r| r.status == Status::AwaitingApproval || !r.status.is_live()).await;
    e.decide(&r.id, GateDecision::Approve { plan: None, verify_command: None }).await.unwrap();
    let r = wait_until(&e, &rec.id, |r| !r.status.is_live()).await;
    assert!(r.turns.len() as u32 <= budget, "{} turns > budget {budget}", r.turns.len());
}
