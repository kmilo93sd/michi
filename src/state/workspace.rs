use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    /// Construye un `Workspace` a partir de una carpeta del filesystem.
    ///
    /// - `name` se toma del nombre de la carpeta (`Documents/projects/foo` → `foo`)
    /// - `repos` se descubre escaneando subdirs que contengan `.git/`
    /// - `claude_md_present` chequea si existe `<path>/CLAUDE.md`
    /// - `specs_count` cuenta subdirs de `<path>/specs/`
    /// - `skills_count` cuenta subdirs de `.claude/skills/` y `.agents/skills/`
    pub fn from_path(path: &Path) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());

        let repos = discover_repos(path);
        let workspace_skills = count_skills(path);
        let repos_skills: usize = repos.iter().map(|r| r.skills_count).sum();

        Self {
            id: format!("ws-{}", Uuid::new_v4()),
            name,
            path: path.to_path_buf(),
            claude_md_present: path.join("CLAUDE.md").exists(),
            specs_count: count_subdirs(&path.join("specs")),
            skills_count: workspace_skills + repos_skills,
            repos,
        }
    }

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
            // Workspace mock #2
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

fn count_subdirs(path: &Path) -> usize {
    match std::fs::read_dir(path) {
        Ok(entries) => entries.flatten().filter(|e| e.path().is_dir()).count(),
        Err(_) => 0,
    }
}

fn count_skills(workspace_path: &Path) -> usize {
    let mut total = 0;
    for sub in [".claude/skills", ".agents/skills"] {
        total += count_subdirs(&workspace_path.join(sub));
    }
    total
}

fn discover_repos(workspace_path: &Path) -> Vec<Repo> {
    let mut repos = Vec::new();
    let Ok(entries) = std::fs::read_dir(workspace_path) else {
        return repos;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        if !p.join(".git").exists() {
            continue;
        }
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        repos.push(Repo {
            id: format!("repo-{}", Uuid::new_v4()),
            name,
            path: p.clone(),
            claude_md_present: p.join("CLAUDE.md").exists(),
            skills_count: count_skills(&p),
        });
    }
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    repos
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn from_path_uses_dir_name() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("my-workspace");
        std::fs::create_dir_all(&path).unwrap();
        let ws = Workspace::from_path(&path);
        assert_eq!(ws.name, "my-workspace");
        assert_eq!(ws.repos.len(), 0);
        assert!(!ws.claude_md_present);
    }

    #[test]
    fn discover_repos_finds_git_subdirs() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(ws.join("repo-a/.git")).unwrap();
        std::fs::create_dir_all(ws.join("repo-b/.git")).unwrap();
        std::fs::create_dir_all(ws.join("not-a-repo")).unwrap();
        std::fs::write(ws.join("loose-file.txt"), "x").unwrap();

        let workspace = Workspace::from_path(&ws);
        assert_eq!(workspace.repos.len(), 2);
        assert_eq!(workspace.repos[0].name, "repo-a");
        assert_eq!(workspace.repos[1].name, "repo-b");
    }

    #[test]
    fn from_path_counts_claude_md_and_specs() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("CLAUDE.md"), "x").unwrap();
        std::fs::create_dir_all(ws.join("specs/spec-1")).unwrap();
        std::fs::create_dir_all(ws.join("specs/spec-2")).unwrap();

        let workspace = Workspace::from_path(&ws);
        assert!(workspace.claude_md_present);
        assert_eq!(workspace.specs_count, 2);
    }

    #[test]
    fn skills_count_aggregates_workspace_and_repos() {
        // Workspace layout:
        //   ws/.claude/skills/{a, b}                  -> 2 skills del workspace
        //   ws/repo-1/.git                            -> repo descubrible
        //   ws/repo-1/.claude/skills/{c, d, e}        -> 3 skills del repo-1
        //   ws/repo-2/.git
        //   ws/repo-2/.agents/skills/{f}              -> 1 skill del repo-2
        // Total agregado esperado: 2 + 3 + 1 = 6
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(ws.join(".claude/skills/a")).unwrap();
        std::fs::create_dir_all(ws.join(".claude/skills/b")).unwrap();
        std::fs::create_dir_all(ws.join("repo-1/.git")).unwrap();
        std::fs::create_dir_all(ws.join("repo-1/.claude/skills/c")).unwrap();
        std::fs::create_dir_all(ws.join("repo-1/.claude/skills/d")).unwrap();
        std::fs::create_dir_all(ws.join("repo-1/.claude/skills/e")).unwrap();
        std::fs::create_dir_all(ws.join("repo-2/.git")).unwrap();
        std::fs::create_dir_all(ws.join("repo-2/.agents/skills/f")).unwrap();

        let workspace = Workspace::from_path(&ws);
        assert_eq!(workspace.repos.len(), 2);
        assert_eq!(workspace.repos[0].skills_count, 3, "repo-1 debe contar 3");
        assert_eq!(workspace.repos[1].skills_count, 1, "repo-2 debe contar 1");
        assert_eq!(
            workspace.skills_count, 6,
            "el agregado del workspace debe sumar workspace + repos"
        );
    }

    #[test]
    fn skills_count_is_zero_when_no_skills_anywhere() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(ws.join("repo-1/.git")).unwrap();
        let workspace = Workspace::from_path(&ws);
        assert_eq!(workspace.skills_count, 0);
    }
}
