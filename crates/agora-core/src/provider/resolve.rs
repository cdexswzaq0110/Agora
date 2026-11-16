//! Find the real CLI executables.
//!
//! On Windows, npm installs `claude` / `codex` as `.cmd` shims. `CreateProcess` will not run a
//! `.cmd` without `cmd.exe` (and quoting a prompt through `cmd.exe` is fragile), so we resolve
//! the native binary the shim points at.

use std::env;
use std::path::{Path, PathBuf};

fn path_dirs() -> Vec<PathBuf> {
    env::var_os("PATH").map(|p| env::split_paths(&p).collect()).unwrap_or_default()
}

fn exists(p: &Path) -> bool {
    p.is_file()
}

/// First directory on PATH that contains any of `names`.
pub fn find_in_path(names: &[&str]) -> Option<PathBuf> {
    for dir in path_dirs() {
        for n in names {
            let c = dir.join(n);
            if exists(&c) {
                return Some(c);
            }
        }
    }
    None
}

fn shim_dirs(shim: &str) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = path_dirs().into_iter().filter(|d| d.join(shim).is_file()).collect();
    if let Some(appdata) = env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("npm"));
    }
    dirs
}

pub fn resolve_claude(override_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = override_path.filter(|p| !p.trim().is_empty()) {
        let p = PathBuf::from(p.trim());
        return exists(&p).then_some(p);
    }
    if cfg!(windows) {
        if let Some(p) = find_in_path(&["claude.exe"]) {
            return Some(p);
        }
        for d in shim_dirs("claude.cmd") {
            let c = d.join("node_modules/@anthropic-ai/claude-code/bin/claude.exe");
            if exists(&c) {
                return Some(c);
            }
        }
        if let Some(home) = env::var_os("USERPROFILE") {
            let c = PathBuf::from(home).join(".local/bin/claude.exe");
            if exists(&c) {
                return Some(c);
            }
        }
        None
    } else {
        find_in_path(&["claude"])
    }
}

pub fn resolve_codex(override_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = override_path.filter(|p| !p.trim().is_empty()) {
        let p = PathBuf::from(p.trim());
        return exists(&p).then_some(p);
    }
    if cfg!(windows) {
        if let Some(p) = find_in_path(&["codex.exe"]) {
            return Some(p);
        }
        for d in shim_dirs("codex.cmd") {
            let base = d.join("node_modules/@openai/codex");
            // Platform package layout: node_modules/@openai/codex-win32-<arch>/vendor/<triple>/bin/codex.exe
            for nested in [base.join("node_modules/@openai"), d.join("node_modules/@openai")] {
                if let Ok(rd) = std::fs::read_dir(&nested) {
                    for pkg in rd.flatten() {
                        if !pkg.file_name().to_string_lossy().starts_with("codex-win32") {
                            continue;
                        }
                        if let Ok(vendors) = std::fs::read_dir(pkg.path().join("vendor")) {
                            for v in vendors.flatten() {
                                let c = v.path().join("bin/codex.exe");
                                if exists(&c) {
                                    return Some(c);
                                }
                            }
                        }
                    }
                }
            }
            // Older layout: vendor/ directly under the main package.
            if let Ok(vendors) = std::fs::read_dir(base.join("vendor")) {
                for v in vendors.flatten() {
                    for c in [v.path().join("bin/codex.exe"), v.path().join("codex/codex.exe")] {
                        if exists(&c) {
                            return Some(c);
                        }
                    }
                }
            }
        }
        None
    } else {
        find_in_path(&["codex"])
    }
}

/// `CODEX_HOME` or `~/.codex`.
pub fn codex_home() -> Option<PathBuf> {
    if let Some(h) = env::var_os("CODEX_HOME") {
        return Some(PathBuf::from(h));
    }
    let home = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".codex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_must_exist() {
        assert!(resolve_claude(Some("Z:/definitely/not/here/claude.exe")).is_none());
        assert!(resolve_codex(Some("Z:/definitely/not/here/codex.exe")).is_none());
    }
}
