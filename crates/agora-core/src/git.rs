//! Git worktree isolation (工作區隔離) for sessions that may write files.
//!
//! A write session in a git repo gets its own worktree on branch `agora/<session>` created from
//! `HEAD`, under the Agora data directory. The CLI agents edit there; the user's working tree
//! is untouched until they press "套用到專案".
//!
//! Agora never commits in the worktree: the diff is `git add -A` + `git diff --cached <base>`.
//! That means no author identity is needed and the project's commit hooks never run on
//! Agora's behalf. "Apply" pipes that diff into `git apply --3way` at the repo top level, so
//! the result lands as ordinary uncommitted changes the user reviews and commits.

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::provider::proc::no_window;

/// Patch text kept for the UI; larger diffs are truncated (the stat list stays complete).
pub const MAX_PATCH_BYTES: usize = 400_000;

fn git_cmd(cwd: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    no_window(&mut c);
    c
}

/// Run git and return stdout (bytes, lossless) or an error carrying stderr.
pub async fn git_bytes(cwd: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let mut c = git_cmd(cwd);
    c.args(args);
    let out = tokio::time::timeout(Duration::from_secs(120), c.output())
        .await
        .map_err(|_| anyhow!("git {} 逾時", args.join(" ")))?
        .with_context(|| "無法執行 git（是否已安裝並在 PATH 中？）")?;
    if !out.status.success() {
        bail!("git {} 失敗：{}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

pub async fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8_lossy(&git_bytes(cwd, args).await?).trim().to_string())
}

pub async fn is_repo(path: &Path) -> bool {
    git(path, &["rev-parse", "--is-inside-work-tree"]).await.is_ok_and(|s| s == "true")
}

pub async fn toplevel(path: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(git(path, &["rev-parse", "--show-toplevel"]).await?))
}

#[derive(Clone, Debug)]
pub struct Worktree {
    /// Worktree root.
    pub root: PathBuf,
    /// Where the agents should work (root + the project's prefix inside the repo).
    pub workdir: PathBuf,
    pub branch: String,
    pub base: String,
}

/// Create `<wt_root>` as a worktree of the repo containing `project`, on a new branch.
pub async fn create_worktree(project: &Path, wt_root: &Path, branch: &str) -> Result<Worktree> {
    let base = git(project, &["rev-parse", "HEAD"]).await.context("這個 repo 還沒有任何 commit，無法建立隔離工作區")?;
    let prefix = git(project, &["rev-parse", "--show-prefix"]).await.unwrap_or_default();
    if let Some(parent) = wt_root.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let wt = wt_root.display().to_string();
    git(project, &["worktree", "add", "-b", branch, &wt, &base]).await?;
    let workdir = if prefix.is_empty() { wt_root.to_path_buf() } else { wt_root.join(prefix.trim_end_matches('/')) };
    Ok(Worktree { root: wt_root.to_path_buf(), workdir, branch: branch.to_string(), base })
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DiffFile {
    pub path: String,
    /// `None` for binary files.
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiffSummary {
    pub files: Vec<DiffFile>,
    pub added: u64,
    pub removed: u64,
    pub patch: String,
    pub truncated: bool,
}

pub fn parse_numstat(s: &str) -> Vec<DiffFile> {
    s.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            let a = it.next()?;
            let r = it.next()?;
            let p = it.next()?;
            Some(DiffFile { path: p.to_string(), added: a.parse().ok(), removed: r.parse().ok() })
        })
        .collect()
}

/// Everything changed in the worktree (including new files) relative to `base`.
pub async fn diff(wt_root: &Path, base: &str) -> Result<DiffSummary> {
    git(wt_root, &["add", "-A"]).await?;
    let numstat = git(wt_root, &["diff", "--cached", "--numstat", base]).await?;
    let files = parse_numstat(&numstat);
    let patch_bytes = git_bytes(wt_root, &["diff", "--cached", base]).await?;
    let truncated = patch_bytes.len() > MAX_PATCH_BYTES;
    let patch = String::from_utf8_lossy(&patch_bytes[..patch_bytes.len().min(MAX_PATCH_BYTES)]).to_string();
    Ok(DiffSummary {
        added: files.iter().filter_map(|f| f.added).sum(),
        removed: files.iter().filter_map(|f| f.removed).sum(),
        files,
        patch,
        truncated,
    })
}

/// Apply the worktree's changes to the original repo's working tree (`git apply --3way`).
pub async fn apply(project: &Path, wt_root: &Path, base: &str) -> Result<usize> {
    git(wt_root, &["add", "-A"]).await?;
    let patch = git_bytes(wt_root, &["diff", "--cached", "--binary", base]).await?;
    if patch.is_empty() {
        return Ok(0);
    }
    let top = toplevel(project).await?;
    let mut c = git_cmd(&top);
    c.args(["apply", "--3way", "--whitespace=nowarn", "-"]).stdin(Stdio::piped());
    let mut child = c.spawn().context("無法執行 git apply")?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&patch).await?;
        stdin.shutdown().await?;
    }
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        bail!(
            "套用失敗（可能和你工作目錄裡未 commit 的修改衝突）：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(parse_numstat(&git(wt_root, &["diff", "--cached", "--numstat", base]).await?).len())
}

/// Remove the worktree and its branch. Idempotent: missing pieces are not errors.
pub async fn remove(project: &Path, wt_root: &Path, branch: &str) -> Result<()> {
    let wt = wt_root.display().to_string();
    let _ = git(project, &["worktree", "remove", "--force", &wt]).await;
    let _ = git(project, &["worktree", "prune"]).await;
    let _ = git(project, &["branch", "-D", branch]).await;
    if wt_root.exists() {
        let _ = std::fs::remove_dir_all(wt_root);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_parses_text_and_binary() {
        let f = parse_numstat("3\t1\tsrc/a.rs\n-\t-\timg.png\n");
        assert_eq!(f[0], DiffFile { path: "src/a.rs".into(), added: Some(3), removed: Some(1) });
        assert_eq!(f[1].added, None);
    }

    async fn init_repo(dir: &Path) {
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/hello.txt"), "hello\n").unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["add", "-A"],
            vec!["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "init"],
        ] {
            git(dir, &args).await.unwrap();
        }
    }

    #[tokio::test]
    async fn worktree_roundtrip_diff_apply_remove() {
        let base_dir = std::env::temp_dir().join(format!("agora-git-{}", crate::model::random_hex(4)));
        let repo = base_dir.join("repo");
        init_repo(&repo).await;
        let project = repo.join("sub");
        assert!(is_repo(&project).await);

        let wt_root = base_dir.join("wt");
        let wt = create_worktree(&project, &wt_root, "agora/test").await.unwrap();
        assert!(wt.workdir.ends_with("sub"), "agents work in the project's subdirectory: {:?}", wt.workdir);

        // Simulate an agent: edit a file and add a new one inside the worktree.
        std::fs::write(wt.workdir.join("hello.txt"), "hello\nworld\n").unwrap();
        std::fs::write(wt.workdir.join("new.txt"), "fresh\n").unwrap();
        // The user's working tree is untouched.
        assert_eq!(std::fs::read_to_string(project.join("hello.txt")).unwrap(), "hello\n");

        let d = diff(&wt.root, &wt.base).await.unwrap();
        assert_eq!(d.files.len(), 2, "{:?}", d.files);
        assert_eq!(d.added, 2);
        assert!(d.patch.contains("+world"));

        let n = apply(&project, &wt.root, &wt.base).await.unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read_to_string(project.join("hello.txt")).unwrap().replace("\r\n", "\n"), "hello\nworld\n");
        assert!(project.join("new.txt").exists());

        remove(&project, &wt.root, &wt.branch).await.unwrap();
        assert!(!wt_root.exists());
        assert!(git(&repo, &["rev-parse", "--verify", "agora/test"]).await.is_err(), "branch deleted");
        let _ = std::fs::remove_dir_all(&base_dir);
    }
}
