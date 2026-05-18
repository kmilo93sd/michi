//! `git status` queries. Hoy solo expone el contador de archivos modificados
//! que se usa para el subtitulo de las cards del sidebar. En el futuro este
//! modulo crecera con `git diff`, listar archivos sin tracking, etc.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Cuenta los archivos con cambios pendientes (modificados, untracked, staged)
/// en `worktree_path`. Ejecuta `git status --porcelain` y cuenta lineas no
/// vacias del output. Cada linea representa un archivo.
pub fn count_changed_files(worktree_path: &Path) -> Result<u32> {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["status", "--porcelain"])
        .output()
        .with_context(|| format!("ejecutando git status en {}", worktree_path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git status fallo: {}", stderr.trim());
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let count = raw.lines().filter(|l| !l.trim().is_empty()).count() as u32;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo() -> Result<TempDir> {
        let dir = TempDir::new()?;
        let p = dir.path();
        run(p, &["init", "--initial-branch=main"])?;
        run(p, &["config", "user.email", "t@t.t"])?;
        run(p, &["config", "user.name", "T"])?;
        run(p, &["commit", "--allow-empty", "-m", "init"])?;
        Ok(dir)
    }

    fn run(repo: &Path, args: &[&str]) -> Result<()> {
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
    fn clean_repo_has_zero_changes() -> Result<()> {
        let dir = init_repo()?;
        assert_eq!(count_changed_files(dir.path())?, 0);
        Ok(())
    }

    #[test]
    fn untracked_files_are_counted() -> Result<()> {
        let dir = init_repo()?;
        std::fs::write(dir.path().join("a.txt"), "hi")?;
        std::fs::write(dir.path().join("b.txt"), "bye")?;
        assert_eq!(count_changed_files(dir.path())?, 2);
        Ok(())
    }

    #[test]
    fn modified_tracked_file_is_counted() -> Result<()> {
        let dir = init_repo()?;
        std::fs::write(dir.path().join("a.txt"), "hi")?;
        run(dir.path(), &["add", "a.txt"])?;
        run(dir.path(), &["commit", "-m", "add a"])?;
        std::fs::write(dir.path().join("a.txt"), "modified")?;
        assert_eq!(count_changed_files(dir.path())?, 1);
        Ok(())
    }

    #[test]
    fn non_git_dir_returns_err() -> Result<()> {
        let dir = TempDir::new()?;
        assert!(count_changed_files(dir.path()).is_err());
        Ok(())
    }
}
