use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Idle,
    Thinking,
    Paused,
    Error,
    NeedsAttention,
}

impl JobStatus {
    pub fn dot(self) -> char {
        match self {
            JobStatus::Idle => '\u{25CF}',
            JobStatus::Thinking => '\u{25D0}',
            JobStatus::Paused => '\u{25CB}',
            JobStatus::Error => '\u{25C6}',
            JobStatus::NeedsAttention => '\u{25B2}',
        }
    }

    pub fn color(self, theme: &crate::theme::Theme) -> egui::Color32 {
        match self {
            JobStatus::Idle => theme.status_idle,
            JobStatus::Thinking => theme.status_thinking,
            JobStatus::Paused => theme.status_paused,
            JobStatus::Error => theme.status_error,
            JobStatus::NeedsAttention => theme.status_needs_attention,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub workspace: String,
    pub repo: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub status: JobStatus,
    pub files_changed: u32,
    #[serde(skip, default = "SystemTime::now")]
    pub last_activity: SystemTime,
    /// Inicio del rango de puertos que michi reserva para esta sesion.
    /// Sesion N usa `port_range_start..port_range_start + RANGE_STEP`. Si
    /// vale 0, el job aun no tiene rango asignado (estado legacy o tests).
    /// Se asigna por `port_alloc::assign_next_range` al crear el job.
    #[serde(default)]
    pub port_range_start: u16,
}

impl Job {
    /// Construye un `Job` para una "sesion directa" sobre un repo: Claude
    /// corre en el repo path tal cual, sin crear un git worktree separado.
    /// Util cuando solo quieres conversar con Claude en la branch actual sin
    /// la ceremonia de rama nueva + worktree.
    ///
    /// `branch` se marca como `(directo)` para que el header del job
    /// muestre visiblemente que no hay un worktree dedicado y el flujo de
    /// "cerrar trabajo" sepa que no debe llamar a `git worktree remove`.
    pub fn for_direct_session(workspace: &str, repo: &str, repo_path: &Path) -> Self {
        Self {
            id: format!("job-{}", Uuid::new_v4()),
            workspace: workspace.to_string(),
            repo: repo.to_string(),
            branch: "(directo)".into(),
            worktree_path: repo_path.to_path_buf(),
            status: JobStatus::Idle,
            files_changed: 0,
            last_activity: SystemTime::now(),
            port_range_start: 0,
        }
    }

    /// Construye un `Job` para una "sesion de workspace": Claude corre en el
    /// workspace path con acceso a todos los repos hijos. No tiene repo
    /// asociado ni worktree separado.
    ///
    /// `repo` y `branch` se marcan con `(workspace)` para el render del
    /// sidebar y para que el close flow sepa que no debe llamar a `git
    /// worktree remove`.
    pub fn for_workspace_session(workspace: &str, workspace_path: &Path) -> Self {
        Self {
            id: format!("job-{}", Uuid::new_v4()),
            workspace: workspace.to_string(),
            repo: "(workspace)".into(),
            branch: "(workspace)".into(),
            worktree_path: workspace_path.to_path_buf(),
            status: JobStatus::Idle,
            files_changed: 0,
            last_activity: SystemTime::now(),
            port_range_start: 0,
        }
    }

    pub fn mock_set() -> Vec<Job> {
        let now = SystemTime::now();
        vec![
            Job {
                id: "job-1".into(),
                workspace: "lelemon-workspace".into(),
                repo: "lelemon-app".into(),
                branch: "feat/cors-fix".into(),
                worktree_path: PathBuf::from(
                    "C:/Users/kmilo/Documents/projects/lelemon-workspace-wt/cors-fix",
                ),
                status: JobStatus::Idle,
                files_changed: 3,
                last_activity: now - Duration::from_secs(2 * 60),
                port_range_start: 0,
            },
            Job {
                id: "job-2".into(),
                workspace: "lelemon-workspace".into(),
                repo: "lelemon-studio-web".into(),
                branch: "landing-v2".into(),
                worktree_path: PathBuf::from(
                    "C:/Users/kmilo/Documents/projects/lelemon-workspace-wt/landing-v2",
                ),
                status: JobStatus::Thinking,
                files_changed: 0,
                last_activity: now,
                port_range_start: 0,
            },
            Job {
                id: "job-3".into(),
                workspace: "venpu-workspace".into(),
                repo: "venpu-backend".into(),
                branch: "fix/whatsapp-webhook".into(),
                worktree_path: PathBuf::from(
                    "C:/Users/kmilo/Documents/projects/venpu-workspace-wt/whatsapp-webhook",
                ),
                status: JobStatus::NeedsAttention,
                files_changed: 1,
                last_activity: now - Duration::from_secs(15 * 60),
                port_range_start: 0,
            },
            Job {
                id: "job-4".into(),
                workspace: "venpu-workspace".into(),
                repo: "venpu-admin".into(),
                branch: "feat/dashboard-v2".into(),
                worktree_path: PathBuf::from(
                    "C:/Users/kmilo/Documents/projects/venpu-workspace-wt/dashboard-v2",
                ),
                status: JobStatus::Paused,
                files_changed: 0,
                last_activity: now - Duration::from_secs(26 * 60 * 60),
                port_range_start: 0,
            },
        ]
    }

    /// Una "sesion in-place" es una sesion directa de repo o de workspace:
    /// no hay un git worktree dedicado, asi que el flujo de cerrar el job
    /// debe limitarse a quitar la entrada de memoria (no llamar a
    /// `git worktree remove`). Se detecta por la convencion de marcar la
    /// branch con un nombre entre parentesis.
    pub fn is_in_place_session(&self) -> bool {
        self.branch == "(directo)" || self.branch == "(workspace)"
    }

    pub fn subtitle(&self) -> String {
        // En sesiones in-place (workspace / directo) NO contamos archivos
        // modificados: `git status` sobre el cwd cuenta cambios pre-existentes
        // y cross-repo que NO son del agente. La filosofia de michi es
        // enfocarse en agentes, no en archivos.
        let show_files = !self.is_in_place_session();
        match self.status {
            JobStatus::NeedsAttention => "permiso pendiente".into(),
            JobStatus::Thinking => {
                if show_files {
                    format!("{} cambios \u{B7} pensando", self.files_changed)
                } else {
                    "pensando".into()
                }
            }
            JobStatus::Paused => format!("pausado \u{B7} {}", humanize_elapsed(self.last_activity)),
            JobStatus::Error => {
                if show_files {
                    format!("{} cambios \u{B7} error", self.files_changed)
                } else {
                    "error".into()
                }
            }
            JobStatus::Idle => {
                if show_files {
                    format!(
                        "{} cambios \u{B7} {}",
                        self.files_changed,
                        humanize_elapsed(self.last_activity)
                    )
                } else {
                    humanize_elapsed(self.last_activity)
                }
            }
        }
    }
}

pub fn humanize_elapsed(since: SystemTime) -> String {
    let elapsed = SystemTime::now().duration_since(since).unwrap_or_default();
    humanize_seconds(elapsed.as_secs())
}

fn humanize_seconds(secs: u64) -> String {
    if secs < 30 {
        "ahora".into()
    } else if secs < 60 * 60 {
        format!("hace {} min", secs / 60)
    } else if secs < 60 * 60 * 24 {
        format!("hace {} h", secs / 3600)
    } else {
        let days = secs / 86400;
        if days == 1 {
            "hace 1 dia".into()
        } else {
            format!("hace {} dias", days)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_under_30_seconds_is_now() {
        assert_eq!(humanize_seconds(0), "ahora");
        assert_eq!(humanize_seconds(15), "ahora");
        assert_eq!(humanize_seconds(29), "ahora");
    }

    #[test]
    fn humanize_minutes() {
        assert_eq!(humanize_seconds(30), "hace 0 min");
        assert_eq!(humanize_seconds(60), "hace 1 min");
        assert_eq!(humanize_seconds(15 * 60), "hace 15 min");
        assert_eq!(humanize_seconds(59 * 60), "hace 59 min");
    }

    #[test]
    fn humanize_hours() {
        assert_eq!(humanize_seconds(60 * 60), "hace 1 h");
        assert_eq!(humanize_seconds(5 * 60 * 60), "hace 5 h");
        assert_eq!(humanize_seconds(23 * 60 * 60), "hace 23 h");
    }

    #[test]
    fn humanize_one_day_is_singular() {
        assert_eq!(humanize_seconds(24 * 60 * 60), "hace 1 dia");
        assert_eq!(humanize_seconds(36 * 60 * 60), "hace 1 dia");
    }

    #[test]
    fn humanize_multiple_days_is_plural() {
        assert_eq!(humanize_seconds(2 * 24 * 60 * 60), "hace 2 dias");
        assert_eq!(humanize_seconds(7 * 24 * 60 * 60), "hace 7 dias");
    }

    fn job_with(status: JobStatus, files_changed: u32, last_activity: SystemTime) -> Job {
        Job {
            id: "test".into(),
            workspace: "ws".into(),
            repo: "repo".into(),
            branch: "feat/x".into(),
            worktree_path: PathBuf::new(),
            status,
            files_changed,
            last_activity,
            port_range_start: 0,
        }
    }

    #[test]
    fn subtitle_needs_attention_says_permiso() {
        let job = job_with(JobStatus::NeedsAttention, 1, SystemTime::now());
        assert_eq!(job.subtitle(), "permiso pendiente");
    }

    #[test]
    fn subtitle_thinking_mentions_pensando() {
        let job = job_with(JobStatus::Thinking, 3, SystemTime::now());
        assert_eq!(job.subtitle(), "3 cambios \u{B7} pensando");
    }

    #[test]
    fn subtitle_paused_mentions_pausado() {
        let job = job_with(
            JobStatus::Paused,
            0,
            SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 60 * 60),
        );
        assert_eq!(job.subtitle(), "pausado \u{B7} hace 2 dias");
    }

    #[test]
    fn subtitle_error_mentions_error() {
        let job = job_with(JobStatus::Error, 4, SystemTime::now());
        assert_eq!(job.subtitle(), "4 cambios \u{B7} error");
    }

    #[test]
    fn subtitle_idle_includes_files_and_elapsed() {
        let job = job_with(
            JobStatus::Idle,
            7,
            SystemTime::now() - std::time::Duration::from_secs(5 * 60),
        );
        assert_eq!(job.subtitle(), "7 cambios \u{B7} hace 5 min");
    }

    #[test]
    fn for_direct_session_uses_repo_path_as_worktree_and_marks_branch() {
        let path = PathBuf::from("/some/repo");
        let job = Job::for_direct_session("my-ws", "my-repo", &path);

        assert_eq!(job.workspace, "my-ws");
        assert_eq!(job.repo, "my-repo");
        assert_eq!(
            job.worktree_path, path,
            "una sesion directa usa el repo path como worktree path"
        );
        assert_eq!(job.status, JobStatus::Idle);
        assert_eq!(job.files_changed, 0);
        assert!(job.id.starts_with("job-"));
        assert!(
            job.branch.starts_with('(') || job.branch.contains("direct"),
            "la branch debe marcarse para distinguirla de un worktree real, fue: {:?}",
            job.branch
        );
    }

    #[test]
    fn for_workspace_session_uses_workspace_path_and_marks_no_repo() {
        let path = PathBuf::from("/my/workspace");
        let job = Job::for_workspace_session("my-ws", &path);

        assert_eq!(job.workspace, "my-ws");
        assert_eq!(
            job.worktree_path, path,
            "una sesion de workspace usa el workspace path como cwd"
        );
        assert_eq!(job.status, JobStatus::Idle);
        assert_eq!(job.files_changed, 0);
        assert!(job.id.starts_with("job-"));
        assert!(
            job.repo.starts_with('(') || job.repo == "*" || job.repo.is_empty(),
            "una sesion de workspace no esta atada a un repo, fue: {:?}",
            job.repo
        );
        assert!(
            job.branch.starts_with('(') || job.branch.contains("workspace"),
            "la branch debe marcarse como sesion de workspace, fue: {:?}",
            job.branch
        );
    }

    #[test]
    fn for_direct_session_generates_unique_ids() {
        let path = PathBuf::from("/x");
        let a = Job::for_direct_session("ws", "repo", &path);
        let b = Job::for_direct_session("ws", "repo", &path);
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn subtitle_in_place_idle_omits_files_count() {
        // En sesion in-place no cuenta archivos; mide cosas pre-existentes.
        let job = Job::for_workspace_session("ws", &PathBuf::from("/w"));
        let subtitle = job.subtitle();
        assert!(
            !subtitle.contains("cambios"),
            "in-place idle no debe mostrar conteo de archivos, fue: {subtitle:?}"
        );
    }

    #[test]
    fn subtitle_in_place_thinking_says_pensando_solo() {
        let mut job = Job::for_workspace_session("ws", &PathBuf::from("/w"));
        job.status = JobStatus::Thinking;
        job.files_changed = 58; // valor "envenenado" simulando git status sucio
        assert_eq!(job.subtitle(), "pensando");
    }

    #[test]
    fn subtitle_in_place_error_says_error_solo() {
        let mut job = Job::for_direct_session("ws", "repo", &PathBuf::from("/r"));
        job.status = JobStatus::Error;
        job.files_changed = 99;
        assert_eq!(job.subtitle(), "error");
    }

    #[test]
    fn subtitle_worktree_sigue_mostrando_cambios() {
        // Para jobs con worktree real, los cambios SI son del agente porque
        // el worktree es exclusivo de la sesion.
        let job = job_with(JobStatus::Idle, 3, SystemTime::now());
        assert!(
            job.subtitle().contains("3 cambios"),
            "worktree real conserva el badge, fue: {:?}",
            job.subtitle()
        );
    }

    #[test]
    fn is_in_place_session_true_for_direct_and_workspace() {
        let direct = Job::for_direct_session("ws", "repo", &PathBuf::from("/r"));
        let workspace = Job::for_workspace_session("ws", &PathBuf::from("/w"));
        assert!(direct.is_in_place_session());
        assert!(workspace.is_in_place_session());
    }

    #[test]
    fn is_in_place_session_false_for_real_worktree() {
        let job = job_with(JobStatus::Idle, 0, SystemTime::now());
        assert!(!job.is_in_place_session());
    }

    #[test]
    fn status_dot_has_unique_char_per_variant() {
        let chars = [
            JobStatus::Idle.dot(),
            JobStatus::Thinking.dot(),
            JobStatus::Paused.dot(),
            JobStatus::Error.dot(),
            JobStatus::NeedsAttention.dot(),
        ];
        for (i, a) in chars.iter().enumerate() {
            for (j, b) in chars.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "dot collision at {i} vs {j}");
                }
            }
        }
    }
}
