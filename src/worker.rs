//! Background workers para operaciones blocking (git, fs).
//!
//! El thread UI de egui no debe bloquearse nunca. Las operaciones blocking
//! corren en `std::thread::spawn` y se comunican con el UI via
//! `std::sync::mpsc`. La app drena los resultados en cada frame.
//!
//! Tokio se reserva para Fase 4 (PTY async + concurrencia más sofisticada).

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::SystemTime;

use anyhow::{Context, Result};
use tracing::{error, info};
use uuid::Uuid;

use crate::git::worktree;
use crate::state::{Job, JobStatus};

/// Resultado de una operación de worker. El UI drena estos eventos en cada
/// frame y actualiza su estado.
#[derive(Debug)]
pub enum WorkerEvent {
    WorktreeCreated(Job),
    WorktreeFailed { message: String },
}

/// Parámetros para crear un worktree desde el modal "Nuevo trabajo".
#[derive(Debug, Clone)]
pub struct CreateWorktreeRequest {
    pub workspace_name: String,
    pub workspace_path: PathBuf,
    pub repo_name: String,
    pub repo_path: PathBuf,
    pub branch: String,
    pub base_branch: String,
}

/// Spawnea un thread que crea el worktree y empuja el resultado al `tx`.
///
/// El UI thread NO se bloquea: solo encola el job y sigue dibujando. Cuando
/// el thread termina (segundos típicamente), el evento llega al canal y el
/// UI lo procesa en el próximo frame.
pub fn spawn_create_worktree(req: CreateWorktreeRequest, tx: Sender<WorkerEvent>) {
    thread::spawn(move || {
        let event = match create_worktree_blocking(&req) {
            Ok(job) => {
                info!(job_id = %job.id, "worktree creado por worker");
                WorkerEvent::WorktreeCreated(job)
            }
            Err(e) => {
                error!("worker fallo al crear worktree: {e:#}");
                WorkerEvent::WorktreeFailed {
                    message: format!("{e:#}"),
                }
            }
        };
        if tx.send(event).is_err() {
            // El UI cerró el receiver; no hay nada que hacer.
        }
    });
}

fn create_worktree_blocking(req: &CreateWorktreeRequest) -> Result<Job> {
    let target = worktree::compute_worktree_path(&req.workspace_path, &req.branch)
        .context("computando path del worktree")?;

    worktree::create(&req.repo_path, &req.branch, &req.base_branch, &target)
        .context("creando worktree con git")?;

    Ok(Job {
        id: new_job_id(),
        workspace: req.workspace_name.clone(),
        repo: req.repo_name.clone(),
        branch: req.branch.clone(),
        worktree_path: target,
        status: JobStatus::Idle,
        files_changed: 0,
        last_activity: SystemTime::now(),
    })
}

fn new_job_id() -> String {
    format!("job-{}", Uuid::new_v4())
}

/// Helper para los tests: verifica que un worktree ya existe en disco.
#[cfg(test)]
pub(crate) fn worktree_exists_for(req: &CreateWorktreeRequest) -> Result<bool> {
    let target = worktree::compute_worktree_path(&req.workspace_path, &req.branch)?;
    Ok(target.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Duration;
    use tempfile::TempDir;

    fn init_repo_with_commit(dir: &Path) -> Result<()> {
        run_git(dir, &["init", "--initial-branch=main"])?;
        run_git(dir, &["config", "user.email", "t@t.t"])?;
        run_git(dir, &["config", "user.name", "T"])?;
        run_git(dir, &["commit", "--allow-empty", "-m", "init"])?;
        Ok(())
    }

    fn run_git(repo: &Path, args: &[&str]) -> Result<()> {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()?;
        if !out.status.success() {
            anyhow::bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    #[test]
    fn spawn_create_worktree_emits_created_event() -> Result<()> {
        let parent = TempDir::new()?;
        let workspace_path = parent.path().join("workspace");
        std::fs::create_dir_all(&workspace_path)?;
        let repo_path = workspace_path.join("my-repo");
        std::fs::create_dir_all(&repo_path)?;
        init_repo_with_commit(&repo_path)?;

        let req = CreateWorktreeRequest {
            workspace_name: "workspace".into(),
            workspace_path: workspace_path.clone(),
            repo_name: "my-repo".into(),
            repo_path: repo_path.clone(),
            branch: "feat/test".into(),
            base_branch: "main".into(),
        };

        let (tx, rx) = mpsc::channel();
        spawn_create_worktree(req.clone(), tx);

        let event = rx
            .recv_timeout(Duration::from_secs(10))
            .context("worker no respondio")?;

        match event {
            WorkerEvent::WorktreeCreated(job) => {
                assert_eq!(job.workspace, "workspace");
                assert_eq!(job.repo, "my-repo");
                assert_eq!(job.branch, "feat/test");
                assert_eq!(job.status, JobStatus::Idle);
                assert!(job.id.starts_with("job-"));
                assert!(worktree_exists_for(&req)?);

                // cleanup
                worktree::remove(&repo_path, &job.worktree_path, true)?;
            }
            WorkerEvent::WorktreeFailed { message } => {
                panic!("worker fallo: {message}");
            }
        }
        Ok(())
    }

    #[test]
    fn spawn_create_worktree_emits_failed_event_on_invalid_repo() -> Result<()> {
        let parent = TempDir::new()?;
        let bad_repo_path = parent.path().join("not-a-repo");
        std::fs::create_dir_all(&bad_repo_path)?;

        let req = CreateWorktreeRequest {
            workspace_name: "workspace".into(),
            workspace_path: parent.path().to_path_buf(),
            repo_name: "not-a-repo".into(),
            repo_path: bad_repo_path,
            branch: "feat/x".into(),
            base_branch: "main".into(),
        };

        let (tx, rx) = mpsc::channel();
        spawn_create_worktree(req, tx);

        let event = rx
            .recv_timeout(Duration::from_secs(10))
            .context("worker no respondio")?;

        assert!(matches!(event, WorkerEvent::WorktreeFailed { .. }));
        Ok(())
    }

    #[test]
    fn new_job_id_is_unique_and_prefixed() {
        let a = new_job_id();
        let b = new_job_id();
        assert!(a.starts_with("job-"));
        assert!(b.starts_with("job-"));
        assert_ne!(a, b);
    }
}
