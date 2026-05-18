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

    pub fn color(self) -> egui::Color32 {
        match self {
            JobStatus::Idle => egui::Color32::from_rgb(82, 196, 26),
            JobStatus::Thinking => egui::Color32::from_rgb(247, 201, 72),
            JobStatus::Paused => egui::Color32::from_rgb(140, 140, 140),
            JobStatus::Error => egui::Color32::from_rgb(220, 76, 76),
            JobStatus::NeedsAttention => egui::Color32::from_rgb(64, 156, 255),
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
                worktree_path: PathBuf::from("C:/Users/kmilo/Documents/projects/lelemon-workspace-wt/cors-fix"),
                status: JobStatus::Idle,
                files_changed: 3,
                last_activity: now - Duration::from_secs(2 * 60),
            },
            Job {
                id: "job-2".into(),
                workspace: "lelemon-workspace".into(),
                repo: "lelemon-studio-web".into(),
                branch: "landing-v2".into(),
                worktree_path: PathBuf::from("C:/Users/kmilo/Documents/projects/lelemon-workspace-wt/landing-v2"),
                status: JobStatus::Thinking,
                files_changed: 0,
                last_activity: now,
            },
            Job {
                id: "job-3".into(),
                workspace: "venpu-workspace".into(),
                repo: "venpu-backend".into(),
                branch: "fix/whatsapp-webhook".into(),
                worktree_path: PathBuf::from("C:/Users/kmilo/Documents/projects/venpu-workspace-wt/whatsapp-webhook"),
                status: JobStatus::NeedsAttention,
                files_changed: 1,
                last_activity: now - Duration::from_secs(15 * 60),
            },
            Job {
                id: "job-4".into(),
                workspace: "venpu-workspace".into(),
                repo: "venpu-admin".into(),
                branch: "feat/dashboard-v2".into(),
                worktree_path: PathBuf::from("C:/Users/kmilo/Documents/projects/venpu-workspace-wt/dashboard-v2"),
                status: JobStatus::Paused,
                files_changed: 0,
                last_activity: now - Duration::from_secs(26 * 60 * 60),
            },
        ]
    }

    pub fn subtitle(&self) -> String {
        match self.status {
            JobStatus::NeedsAttention => "permiso pendiente".into(),
            JobStatus::Thinking => format!(
                "{} cambios \u{B7} pensando",
                self.files_changed
            ),
            JobStatus::Paused => {
                format!("pausado \u{B7} {}", humanize_elapsed(self.last_activity))
            }
            JobStatus::Error => format!(
                "{} cambios \u{B7} error",
                self.files_changed
            ),
            JobStatus::Idle => format!(
                "{} cambios \u{B7} {}",
                self.files_changed,
                humanize_elapsed(self.last_activity)
            ),
        }
    }
}

pub fn humanize_elapsed(since: SystemTime) -> String {
    let elapsed = SystemTime::now()
        .duration_since(since)
        .unwrap_or_default();
    let secs = elapsed.as_secs();
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
