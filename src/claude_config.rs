//! Inventario de configuracion que Claude Code ve al arrancar en un cwd.
//!
//! Cuando Claude arranca, carga la UNION de:
//! - `~/.claude/` (globales del usuario: skills, agents, mcps).
//! - `<cwd>/.claude/` y `<cwd>/CLAUDE.md` (workspace).
//! - `<cwd>/<repo>/.claude/` por cada subdir-repo (si el cwd los contiene).
//!
//! Este modulo cuenta lo que hay en cada nivel para que el sidebar pueda
//! mostrar totales realistas + breakdown en tooltip.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Snapshot del inventario de Claude para un scope (global / workspace / repo).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaudeInventory {
    pub skills: usize,
    pub agents: usize,
    pub mcps: usize,
    /// Nombres de los MCPs para mostrar en el tooltip detallado.
    pub mcp_names: Vec<String>,
}

impl ClaudeInventory {
    fn add(&mut self, other: &ClaudeInventory) {
        self.skills += other.skills;
        self.agents += other.agents;
        self.mcps += other.mcps;
        for n in &other.mcp_names {
            if !self.mcp_names.contains(n) {
                self.mcp_names.push(n.clone());
            }
        }
    }
}

/// Inventario agregado para un workspace especifico: globales del usuario +
/// del workspace + suma de sus repos hijos. Mantenemos las piezas por
/// separado para el tooltip breakdown.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceTotals {
    pub globals: ClaudeInventory,
    pub workspace: ClaudeInventory,
    pub repos: ClaudeInventory,
}

impl WorkspaceTotals {
    pub fn total_skills(&self) -> usize {
        self.globals.skills + self.workspace.skills + self.repos.skills
    }

    pub fn total_mcps(&self) -> usize {
        // MCPs locales pueden tener mismo nombre que globales y redefinirlos.
        // Para el conteo tomamos el "set" union por nombre.
        let mut names: Vec<&String> = self.globals.mcp_names.iter().collect();
        for n in &self.workspace.mcp_names {
            if !names.contains(&n) {
                names.push(n);
            }
        }
        names.len()
    }

    pub fn total_agents(&self) -> usize {
        self.globals.agents + self.workspace.agents + self.repos.agents
    }
}

/// Inventario de `~/.claude/`: skills + agents en sus subdirs, y MCPs del
/// `~/.claude.json`. Si el archivo no existe, devuelve un inventario vacio
/// sin error (caso comun de usuario nuevo).
pub fn read_globals() -> ClaudeInventory {
    let Some(home) = dirs::home_dir() else {
        return ClaudeInventory::default();
    };
    let claude_dir = home.join(".claude");
    let (mcps, mcp_names) = read_mcp_servers(&home.join(".claude.json"));
    ClaudeInventory {
        skills: count_subdirs(&claude_dir.join("skills")),
        agents: count_subdirs(&claude_dir.join("agents")),
        mcps,
        mcp_names,
    }
}

/// Inventario del path del workspace: `<path>/.claude/skills`,
/// `<path>/.claude/agents`, `<path>/.agents/skills` (convencion legacy), y
/// `<path>/.mcp.json`. NO incluye repos hijos.
pub fn read_workspace(path: &Path) -> ClaudeInventory {
    let claude_dir = path.join(".claude");
    let agents_legacy = path.join(".agents");
    let (mcps, mcp_names) = read_mcp_servers(&path.join(".mcp.json"));
    ClaudeInventory {
        skills: count_subdirs(&claude_dir.join("skills"))
            + count_subdirs(&agents_legacy.join("skills")),
        agents: count_subdirs(&claude_dir.join("agents")),
        mcps,
        mcp_names,
    }
}

/// Inventario sumado de todos los `repo_paths` (subdirs git del workspace).
/// Lo que cuenta es `<repo>/.claude/...`; los MCPs a nivel repo no son una
/// convencion comun de Claude Code, asi que no los leemos.
pub fn read_repos(repo_paths: &[PathBuf]) -> ClaudeInventory {
    let mut total = ClaudeInventory::default();
    for r in repo_paths {
        let repo_inv = ClaudeInventory {
            skills: count_subdirs(&r.join(".claude").join("skills"))
                + count_subdirs(&r.join(".agents").join("skills")),
            agents: count_subdirs(&r.join(".claude").join("agents")),
            mcps: 0,
            mcp_names: Vec::new(),
        };
        total.add(&repo_inv);
    }
    total
}

/// Conveniencia: arma el `WorkspaceTotals` completo para un workspace dado
/// `(workspace_path, repos)` reutilizando los globales (que el caller puede
/// haber cacheado).
pub fn totals_for(
    workspace_path: &Path,
    repo_paths: &[PathBuf],
    globals: &ClaudeInventory,
) -> WorkspaceTotals {
    WorkspaceTotals {
        globals: globals.clone(),
        workspace: read_workspace(workspace_path),
        repos: read_repos(repo_paths),
    }
}

fn count_subdirs(path: &Path) -> usize {
    match fs::read_dir(path) {
        Ok(entries) => entries.flatten().filter(|e| e.path().is_dir()).count(),
        Err(_) => 0,
    }
}

/// Lee la sección `mcpServers` de un `.mcp.json` o `~/.claude.json` (mismo
/// formato). Devuelve `(count, names)`. Si el archivo no existe o no parsea,
/// devuelve `(0, vec![])` sin emitir error (la app no debe morir por config
/// invalida del user).
fn read_mcp_servers(path: &Path) -> (usize, Vec<String>) {
    let Ok(raw) = fs::read_to_string(path) else {
        return (0, Vec::new());
    };
    #[derive(Deserialize)]
    struct McpFile {
        #[serde(default, rename = "mcpServers")]
        mcp_servers: serde_json::Value,
    }
    let Ok(parsed) = serde_json::from_str::<McpFile>(&raw) else {
        return (0, Vec::new());
    };
    let Some(obj) = parsed.mcp_servers.as_object() else {
        return (0, Vec::new());
    };
    let mut names: Vec<String> = obj.keys().cloned().collect();
    names.sort();
    (names.len(), names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_workspace_counts_skills_in_dot_claude() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path();
        fs::create_dir_all(p.join(".claude/skills/a")).unwrap();
        fs::create_dir_all(p.join(".claude/skills/b")).unwrap();
        fs::create_dir_all(p.join(".claude/skills/c")).unwrap();
        let inv = read_workspace(p);
        assert_eq!(inv.skills, 3);
    }

    #[test]
    fn read_workspace_also_counts_legacy_agents_skills() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path();
        fs::create_dir_all(p.join(".claude/skills/a")).unwrap();
        fs::create_dir_all(p.join(".agents/skills/x")).unwrap();
        fs::create_dir_all(p.join(".agents/skills/y")).unwrap();
        let inv = read_workspace(p);
        assert_eq!(inv.skills, 3, "suma .claude/skills + .agents/skills");
    }

    #[test]
    fn read_workspace_counts_agents() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path();
        fs::create_dir_all(p.join(".claude/agents/foo")).unwrap();
        fs::create_dir_all(p.join(".claude/agents/bar")).unwrap();
        let inv = read_workspace(p);
        assert_eq!(inv.agents, 2);
    }

    #[test]
    fn read_workspace_reads_mcps_from_dot_mcp_json() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path();
        fs::write(
            p.join(".mcp.json"),
            r#"{"mcpServers":{"playwright":{},"github":{},"context7":{}}}"#,
        )
        .unwrap();
        let inv = read_workspace(p);
        assert_eq!(inv.mcps, 3);
        assert_eq!(
            inv.mcp_names,
            vec!["context7".to_string(), "github".into(), "playwright".into()],
            "names ordenados alfabeticamente"
        );
    }

    #[test]
    fn read_workspace_empty_when_nothing_present() {
        let tmp = TempDir::new().unwrap();
        let inv = read_workspace(tmp.path());
        assert_eq!(inv.skills, 0);
        assert_eq!(inv.agents, 0);
        assert_eq!(inv.mcps, 0);
        assert!(inv.mcp_names.is_empty());
    }

    #[test]
    fn read_workspace_handles_corrupt_mcp_json() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(".mcp.json"), "{not valid").unwrap();
        let inv = read_workspace(tmp.path());
        assert_eq!(inv.mcps, 0, "no debe romper la app por config invalida");
    }

    #[test]
    fn read_repos_sums_skills_across_all_repos() {
        let tmp = TempDir::new().unwrap();
        let r1 = tmp.path().join("repo-a");
        let r2 = tmp.path().join("repo-b");
        fs::create_dir_all(r1.join(".claude/skills/x")).unwrap();
        fs::create_dir_all(r1.join(".claude/skills/y")).unwrap();
        fs::create_dir_all(r2.join(".claude/skills/z")).unwrap();
        let inv = read_repos(&[r1, r2]);
        assert_eq!(inv.skills, 3);
    }

    #[test]
    fn totals_for_combines_globals_workspace_repos() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().join("ws");
        let repo = ws.join("repo-x");
        fs::create_dir_all(ws.join(".claude/skills/ws-a")).unwrap();
        fs::create_dir_all(ws.join(".claude/skills/ws-b")).unwrap();
        fs::create_dir_all(repo.join(".claude/skills/repo-a")).unwrap();

        let globals = ClaudeInventory {
            skills: 5,
            ..Default::default()
        };
        let totals = totals_for(&ws, &[repo], &globals);

        assert_eq!(totals.globals.skills, 5);
        assert_eq!(totals.workspace.skills, 2);
        assert_eq!(totals.repos.skills, 1);
        assert_eq!(totals.total_skills(), 8);
    }

    #[test]
    fn total_mcps_deduplicates_by_name_between_global_and_workspace() {
        // Si un MCP esta definido global Y en el workspace (mismo nombre),
        // el workspace lo "redefine" y el total es el UNION (sin doble conteo).
        let globals = ClaudeInventory {
            mcps: 2,
            mcp_names: vec!["playwright".into(), "github".into()],
            ..Default::default()
        };
        let workspace = ClaudeInventory {
            mcps: 2,
            mcp_names: vec!["playwright".into(), "betterstack".into()],
            ..Default::default()
        };
        let totals = WorkspaceTotals {
            globals,
            workspace,
            repos: ClaudeInventory::default(),
        };
        assert_eq!(
            totals.total_mcps(),
            3,
            "playwright se cuenta una vez aunque este en ambos"
        );
    }

    #[test]
    fn total_skills_just_sums_all_three() {
        let globals = ClaudeInventory {
            skills: 10,
            ..Default::default()
        };
        let workspace = ClaudeInventory {
            skills: 20,
            ..Default::default()
        };
        let repos = ClaudeInventory {
            skills: 5,
            ..Default::default()
        };
        let totals = WorkspaceTotals {
            globals,
            workspace,
            repos,
        };
        assert_eq!(totals.total_skills(), 35);
    }
}
