//! 程式庫快照 — a small, deterministic picture of the project (file tree, README head, recent
//! commits) put into planning/discussion prompts so proposals start grounded. The CLI agents
//! can still read any file themselves; this just saves them the first exploratory turns.

use std::path::Path;

use crate::compact::truncate_chars;

const SKIP_DIRS: &[&str] = &[
    ".git", "node_modules", "target", "dist", "build", ".venv", "venv", "__pycache__", ".next", ".idea", ".vscode",
    ".pytest_cache", ".mypy_cache", "coverage", ".gradle", "bin", "obj",
];
pub const MAX_ENTRIES: usize = 120;
pub const README_CHARS: usize = 1500;

fn walk(dir: &Path, depth: usize, max_depth: usize, prefix: &str, out: &mut Vec<String>) {
    if out.len() >= MAX_ENTRIES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| (!e.path().is_dir(), e.file_name()));
    for e in entries {
        if out.len() >= MAX_ENTRIES {
            out.push(format!("{prefix}…（其餘省略）"));
            return;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let is_dir = e.path().is_dir();
        if is_dir && (SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.')) {
            continue;
        }
        out.push(format!("{prefix}{name}{}", if is_dir { "/" } else { "" }));
        if is_dir && depth < max_depth {
            walk(&e.path(), depth + 1, max_depth, &format!("{prefix}  "), out);
        }
    }
}

pub fn file_tree(root: &Path, max_depth: usize) -> String {
    let mut out = Vec::new();
    walk(root, 1, max_depth, "", &mut out);
    out.join("\n")
}

fn readme(root: &Path) -> Option<String> {
    for n in ["README.md", "README.zh-TW.md", "README", "readme.md", "README.txt"] {
        if let Ok(s) = std::fs::read_to_string(root.join(n)) {
            return Some(truncate_chars(s.trim(), README_CHARS));
        }
    }
    None
}

/// Markdown snapshot, bounded to a few thousand characters.
pub async fn snapshot(root: &Path) -> String {
    let mut s = String::new();
    let tree = file_tree(root, 2);
    if !tree.is_empty() {
        s.push_str("### 檔案樹（深度 2）\n```\n");
        s.push_str(&tree);
        s.push_str("\n```\n");
    }
    if let Some(r) = readme(root) {
        s.push_str("### README 開頭\n");
        s.push_str(&r);
        s.push('\n');
    }
    if crate::git::is_repo(root).await {
        if let Ok(log) = crate::git::git(root, &["log", "--oneline", "-8"]).await {
            if !log.is_empty() {
                s.push_str("### 最近的 commit\n```\n");
                s.push_str(&log);
                s.push_str("\n```\n");
            }
        }
        if let Ok(st) = crate::git::git(root, &["status", "--short"]).await {
            let n = st.lines().count();
            if n > 0 {
                s.push_str(&format!("（工作目錄有 {n} 個未 commit 的變更）\n"));
            }
        }
    }
    truncate_chars(&s, 6000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn snapshot_skips_noise_and_includes_readme() {
        let d = std::env::temp_dir().join(format!("agora-repo-{}", crate::model::random_hex(4)));
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("node_modules/x")).unwrap();
        std::fs::write(d.join("src/main.rs"), "fn main(){}").unwrap();
        std::fs::write(d.join("README.md"), "# Demo\n說明").unwrap();
        let s = snapshot(&d).await;
        assert!(s.contains("src/") && s.contains("main.rs"));
        assert!(!s.contains("node_modules"));
        assert!(s.contains("# Demo"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
