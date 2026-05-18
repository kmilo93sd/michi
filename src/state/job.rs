use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

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
}

impl Job {
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
            },
        ]
    }

    pub fn subtitle(&self) -> String {
        match self.status {
            JobStatus::NeedsAttention => "permiso pendiente".into(),
            JobStatus::Thinking => format!("{} cambios \u{B7} pensando", self.files_changed),
            JobStatus::Paused => {
                format!("pausado \u{B7} {}", humanize_elapsed(self.last_activity))
            }
            JobStatus::Error => format!("{} cambios \u{B7} error", self.files_changed),
            JobStatus::Idle => format!(
                "{} cambios \u{B7} {}",
                self.files_changed,
                humanize_elapsed(self.last_activity)
            ),
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
