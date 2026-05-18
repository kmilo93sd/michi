use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub repos: Vec<Repo>,
    #[serde(default)]
    pub claude_md_present: bool,
    #[serde(default)]
    pub specs_count: usize,
    #[serde(default)]
    pub skills_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub claude_md_present: bool,
    #[serde(default)]
    pub skills_count: usize,
}

impl Workspace {
    pub fn mock_set() -> Vec<Workspace> {
        vec![
            Workspace {
                id: "ws-lelemon".into(),
                name: "lelemon-workspace".into(),
                path: PathBuf::from("C:/Users/kmilo/Documents/projects/lelemon-workspace"),
                claude_md_present: true,
                specs_count: 28,
                skills_count: 12,
                repos: vec![
                    Repo {
                        id: "repo-lelemon-app".into(),
                        name: "lelemon-app".into(),
                        path: PathBuf::from(
                            "C:/Users/kmilo/Documents/projects/lelemon-workspace/lelemon-app",
                        ),
                        claude_md_present: true,
                        skills_count: 4,
                    },
                    Repo {
                        id: "repo-lelemon-studio-web".into(),
                        name: "lelemon-studio-web".into(),
                        path: PathBuf::from(
                            "C:/Users/kmilo/Documents/projects/lelemon-workspace/lelemon-studio-web",
                        ),
                        claude_md_present: false,
                        skills_count: 2,
                    },
                    Repo {
                        id: "repo-chiringeek-web".into(),
                        name: "chiringeek-web".into(),
                        path: PathBuf::from(
                            "C:/Users/kmilo/Documents/projects/lelemon-workspace/chiringeek-web",
                        ),
                        claude_md_present: true,
                        skills_count: 0,
                    },
                ],
            },
            Workspace {
                id: "ws-venpu".into(),
                name: "venpu-workspace".into(),
                path: PathBuf::from("C:/Users/kmilo/Documents/projects/venpu-workspace"),
                claude_md_present: true,
                specs_count: 15,
                skills_count: 8,
                repos: vec![
                    Repo {
                        id: "repo-venpu-backend".into(),
                        name: "venpu-backend".into(),
                        path: PathBuf::from(
                            "C:/Users/kmilo/Documents/projects/venpu-workspace/venpu-backend",
                        ),
                        claude_md_present: true,
                        skills_count: 3,
                    },
                    Repo {
                        id: "repo-venpu-admin".into(),
                        name: "venpu-admin".into(),
                        path: PathBuf::from(
                            "C:/Users/kmilo/Documents/projects/venpu-workspace/venpu-admin",
                        ),
                        claude_md_present: false,
                        skills_count: 0,
                    },
                ],
            },
        ]
    }
}
