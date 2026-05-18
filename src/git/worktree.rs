//! Operaciones de `git worktree`. Shellea al binario `git` (no usa `git2`)
//! porque (a) el mismo binario que el usuario tiene resuelve config global,
//! credenciales y SSH agents, y (b) la complejidad de `git2` no se justifica
//! en el POC.
//!
//! Todas las funciones son síncronas. El caller debe envolverlas en
//! `tokio::task::spawn_blocking` cuando se invoquen desde el thread UI
//! (ver `RUST_GUIDELINES.md`).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use tracing::{debug, info};

/// Calcula el path canónico de un worktree para `(workspace, branch)`.
///
/// Convención del POC: el worktree vive en una carpeta hermana del workspace
/// con sufijo `-wt`, y la branch se convierte a slug filesystem-safe.
///
/// ```text
/// workspace_dir = ".../lelemon-workspace"
/// branch        = "feat/cors-fix"
/// resultado     = ".../lelemon-workspace-wt/feat-cors-fix"
/// ```
pub fn compute_worktree_path(workspace_dir: &Path, branch: &str) -> Result<PathBuf> {
    let parent = workspace_dir
        .parent()
        .with_context(|| format!("{} no tiene parent", workspace_dir.display()))?;
    let name = workspace_dir
        .file_name()
        .with_context(|| format!("{} no tiene file_name", workspace_dir.display()))?;
    let wt_root = parent.join(format!("{}-wt", name.to_string_lossy()));
    Ok(wt_root.join(branch_slug(branch)))
}

/// Convierte un branch name a un slug seguro para filesystem.
/// `feat/cors-fix` → `feat-cors-fix`. `bug_123/x y` → `bug-123-x-y`.
fn branch_slug(branch: &str) -> String {
    branch
        .chars()
        .map(|c| match c {
            '/' | '\\' | ' ' | '_' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            other => other,
        })
        .collect()
}

/// Crea un nuevo worktree con una rama nueva basada en `base`.
///
/// Ejecuta `git -C <repo_path> worktree add -b <branch> <target> <base>`.
/// El directorio padre de `target` se crea si no existe.
pub fn create(repo_path: &Path, branch: &str, base: &str, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creando parent de {}", target.display()))?;
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["worktree", "add", "-b", branch])
        .arg(target)
        .arg(base)
        .output()
        .with_context(|| format!("ejecutando git worktree add en {}", repo_path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree add fallo: {}", stderr.trim());
    }

    info!(
        target = %target.display(),
        branch = branch,
        base = base,
        "worktree creado"
    );
    Ok(())
}

/// Elimina un worktree previamente creado.
///
/// Ejecuta `git -C <repo_path> worktree remove [--force] <worktree_path>`.
/// Sin `force`, git falla si hay cambios sin commitear en el worktree.
pub fn remove(repo_path: &Path, worktree_path: &Path, force: bool) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo_path).args(["worktree", "remove"]);
    if force {
        cmd.arg("--force");
    }
    cmd.arg(worktree_path);

    let output = cmd
        .output()
        .with_context(|| format!("ejecutando git worktree remove en {}", repo_path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree remove fallo: {}", stderr.trim());
    }

    info!(worktree = %worktree_path.display(), force = force, "worktree removido");
    Ok(())
}

/// Información de un worktree retornada por `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub bare: bool,
    pub detached: bool,
}

/// Lista los worktrees registrados para el repo `repo_path`.
///
/// Parsea `git worktree list --porcelain`. Cada bloque del output describe un
/// worktree con líneas `key value` separadas por blanco.
pub fn list(repo_path: &Path) -> Result<Vec<WorktreeInfo>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .with_context(|| format!("ejecutando git worktree list en {}", repo_path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree list fallo: {}", stderr.trim());
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let infos = parse_worktree_list(&raw);
    debug!(count = infos.len(), "worktree list parsed");
    Ok(infos)
}

fn parse_worktree_list(raw: &str) -> Vec<WorktreeInfo> {
    let mut out = Vec::new();
    let mut current: Option<WorktreeInfo> = None;

    for line in raw.lines() {
        if line.is_empty() {
            if let Some(info) = current.take() {
                out.push(info);
            }
            continue;
        }

        let (key, rest) = match line.split_once(' ') {
            Some(kv) => kv,
            None => (line, ""),
        };

        match key {
            "worktree" => {
                if let Some(info) = current.take() {
                    out.push(info);
                }
                current = Some(WorktreeInfo {
                    path: PathBuf::from(rest),
                    branch: None,
                    head: None,
                    bare: false,
                    detached: false,
                });
            }
            "HEAD" => {
                if let Some(info) = current.as_mut() {
                    info.head = Some(rest.to_string());
                }
            }
            "branch" => {
                if let Some(info) = current.as_mut() {
                    let cleaned = rest.strip_prefix("refs/heads/").unwrap_or(rest);
                    info.branch = Some(cleaned.to_string());
                }
            }
            "bare" => {
                if let Some(info) = current.as_mut() {
                    info.bare = true;
                }
            }
            "detached" => {
                if let Some(info) = current.as_mut() {
                    info.detached = true;
                }
            }
            _ => {}
        }
    }

    if let Some(info) = current.take() {
        out.push(info);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo_with_commit() -> Result<TempDir> {
        let dir = TempDir::new()?;
        let path = dir.path();

        run_git(path, &["init", "--initial-branch=main"])?;
        run_git(path, &["config", "user.email", "test@example.com"])?;
        run_git(path, &["config", "user.name", "Test"])?;
        run_git(path, &["commit", "--allow-empty", "-m", "init"])?;

        Ok(dir)
    }

    fn run_git(repo: &Path, args: &[&str]) -> Result<()> {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()?;
        if !out.status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    #[test]
    fn branch_slug_replaces_separators() {
        assert_eq!(branch_slug("feat/cors-fix"), "feat-cors-fix");
        assert_eq!(branch_slug("bug_123/x"), "bug-123-x");
        assert_eq!(branch_slug("a b/c"), "a-b-c");
    }

    #[test]
    fn branch_slug_keeps_letters_digits_dashes() {
        assert_eq!(branch_slug("hotfix-2026"), "hotfix-2026");
        assert_eq!(branch_slug("v1.2.3"), "v1.2.3");
    }

    #[test]
    fn compute_worktree_path_uses_workspace_parent() -> Result<()> {
        let ws = PathBuf::from("/tmp/repos/lelemon-workspace");
        let result = compute_worktree_path(&ws, "feat/cors-fix")?;
        assert_eq!(
            result,
            PathBuf::from("/tmp/repos/lelemon-workspace-wt/feat-cors-fix")
        );
        Ok(())
    }

    #[test]
    fn create_and_remove_worktree_roundtrip() -> Result<()> {
        let repo = init_repo_with_commit()?;
        let parent = repo.path().parent().unwrap();
        let target = parent.join("wt-test");

        create(repo.path(), "feat/test", "main", &target)?;
        assert!(target.exists(), "worktree path debe existir");

        let listed = list(repo.path())?;
        assert!(
            listed
                .iter()
                .any(|w| w.branch.as_deref() == Some("feat/test")),
            "feat/test debe aparecer en list, got: {:?}",
            listed
        );

        remove(repo.path(), &target, false)?;
        assert!(
            !target.exists(),
            "worktree path debe desaparecer tras remove"
        );
        Ok(())
    }

    #[test]
    fn create_fails_when_branch_already_exists() -> Result<()> {
        let repo = init_repo_with_commit()?;
        let parent = repo.path().parent().unwrap();
        let target1 = parent.join("wt-dup-1");
        let target2 = parent.join("wt-dup-2");

        create(repo.path(), "feat/dup", "main", &target1)?;
        let err = create(repo.path(), "feat/dup", "main", &target2).unwrap_err();
        assert!(
            err.to_string().contains("dup") || err.to_string().contains("already"),
            "error debe mencionar la branch duplicada: {err}"
        );

        // cleanup
        remove(repo.path(), &target1, true)?;
        Ok(())
    }

    #[test]
    fn remove_with_force_succeeds_with_dirty_worktree() -> Result<()> {
        let repo = init_repo_with_commit()?;
        let parent = repo.path().parent().unwrap();
        let target = parent.join("wt-dirty");

        create(repo.path(), "feat/dirty", "main", &target)?;
        std::fs::write(target.join("dirty.txt"), "uncommitted")?;

        // sin force, debería fallar porque hay archivos sin trackear
        assert!(remove(repo.path(), &target, false).is_err());
        // con force, OK
        remove(repo.path(), &target, true)?;
        assert!(!target.exists());
        Ok(())
    }

    #[test]
    fn parse_worktree_list_handles_multiple_entries() {
        let raw = "\
worktree /tmp/repo
HEAD abcdef1234
branch refs/heads/main

worktree /tmp/repo-wt/feat
HEAD 9876543210
branch refs/heads/feat/x

worktree /tmp/repo-wt/detached
HEAD 1111111111
detached
";
        let parsed = parse_worktree_list(raw);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].branch.as_deref(), Some("main"));
        assert_eq!(parsed[1].branch.as_deref(), Some("feat/x"));
        assert!(parsed[2].detached);
        assert!(parsed[2].branch.is_none());
    }

    #[test]
    fn parse_worktree_list_handles_bare() {
        let raw = "\
worktree /tmp/bare-repo
bare

worktree /tmp/bare-repo-wt/main
HEAD abc
branch refs/heads/main
";
        let parsed = parse_worktree_list(raw);
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].bare);
        assert!(!parsed[1].bare);
    }
}
