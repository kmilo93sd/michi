//! Deteccion y preparacion de workspaces "pelados".
//!
//! Un workspace pelado es una carpeta que no tiene la config minima que
//! Claude Code aprovecha al arrancar: `CLAUDE.md`, `.claude/`, `.mcp.json`,
//! `specs/`. michi puede prepararla en un click, creando scaffolding
//! razonable + opcionalmente `git init` cuando no hay repos hijos.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Snapshot del estado de "preparacion" de un workspace: que falta y que
/// hay para saber si conviene hacer git init.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePreparationStatus {
    pub has_claude_md: bool,
    pub has_claude_dir: bool,
    pub has_mcp_json: bool,
    pub has_specs_dir: bool,
    /// El workspace mismo es un repo git (tiene `.git/` en root).
    pub has_root_git: bool,
    /// El workspace contiene al menos un subdir con `.git/` propio
    /// (caso `lelemon-workspace`: agrupa repos hijos).
    pub has_child_git_dirs: bool,
}

impl WorkspacePreparationStatus {
    /// `true` si al workspace le faltan TODOS los artefactos de contexto.
    /// Bajo este flag el banner "Preparar workspace" aparece en la card.
    pub fn is_bare(&self) -> bool {
        !self.has_claude_md && !self.has_claude_dir && !self.has_mcp_json
    }

    /// `true` si tiene sentido ofrecer `git init` automatico: no hay git
    /// en root y no hay subdirs con `.git/` (que se confundirian con el git
    /// del padre). Para `lelemon-workspace` esto es `false`.
    pub fn can_git_init(&self) -> bool {
        !self.has_root_git && !self.has_child_git_dirs
    }
}

/// Inspecciona la carpeta y devuelve el snapshot. No falla por permisos:
/// trata cualquier error de IO como "no esta presente".
pub fn inspect(path: &Path) -> WorkspacePreparationStatus {
    WorkspacePreparationStatus {
        has_claude_md: path.join("CLAUDE.md").is_file(),
        has_claude_dir: path.join(".claude").is_dir(),
        has_mcp_json: path.join(".mcp.json").is_file(),
        has_specs_dir: path.join("specs").is_dir(),
        has_root_git: path.join(".git").exists(),
        has_child_git_dirs: detect_child_git(path),
    }
}

fn detect_child_git(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        // Saltar el .git del root: ese se reporta por separado.
        if p.file_name().and_then(|n| n.to_str()) == Some(".git") {
            continue;
        }
        if p.join(".git").exists() {
            return true;
        }
    }
    false
}

/// Que items crear al preparar. Default = todos salvo `git_init`, que
/// queremos opt-in despues de chequear `can_git_init`.
#[derive(Debug, Clone, Copy)]
pub struct PrepareOpts {
    pub claude_md: bool,
    pub claude_dir: bool,
    pub mcp_json: bool,
    pub specs_dir: bool,
    pub git_init: bool,
}

impl PrepareOpts {
    /// Conjunto recomendado a partir del status. Marca todo lo que falta
    /// y habilita `git_init` solo si `can_git_init`.
    pub fn recommended_for(status: &WorkspacePreparationStatus) -> Self {
        Self {
            claude_md: !status.has_claude_md,
            claude_dir: !status.has_claude_dir,
            mcp_json: !status.has_mcp_json,
            specs_dir: !status.has_specs_dir,
            git_init: status.can_git_init(),
        }
    }
}

/// Resultado de la preparacion: paths creados (orden de creacion). Sirve
/// para mostrar feedback al usuario.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreparedReport {
    pub created: Vec<PathBuf>,
    pub git_initialized: bool,
}

/// Aplica `opts` al `path`. Nunca sobreescribe archivos existentes: si
/// `CLAUDE.md` ya existe, no lo toca aunque `opts.claude_md = true`.
pub fn prepare(path: &Path, opts: PrepareOpts) -> Result<PreparedReport> {
    let mut report = PreparedReport::default();

    if opts.claude_md {
        let target = path.join("CLAUDE.md");
        if !target.exists() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("workspace");
            fs::write(&target, claude_md_template(name))
                .with_context(|| format!("escribiendo {}", target.display()))?;
            report.created.push(target);
        }
    }

    if opts.claude_dir {
        let dir = path.join(".claude");
        if !dir.exists() {
            fs::create_dir_all(dir.join("skills"))
                .with_context(|| format!("creando {}/skills", dir.display()))?;
            fs::create_dir_all(dir.join("agents"))
                .with_context(|| format!("creando {}/agents", dir.display()))?;
            report.created.push(dir);
        }
    }

    if opts.specs_dir {
        let dir = path.join("specs");
        if !dir.exists() {
            fs::create_dir_all(&dir).with_context(|| format!("creando {}", dir.display()))?;
            report.created.push(dir);
        }
    }

    if opts.mcp_json {
        let target = path.join(".mcp.json");
        if !target.exists() {
            fs::write(&target, "{}\n")
                .with_context(|| format!("escribiendo {}", target.display()))?;
            report.created.push(target);
        }
    }

    if opts.git_init && !path.join(".git").exists() {
        let output = std::process::Command::new("git")
            .arg("init")
            .arg("--quiet")
            .current_dir(path)
            .output()
            .with_context(|| format!("git init en {}", path.display()))?;
        if !output.status.success() {
            anyhow::bail!(
                "git init fallo: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        report.git_initialized = true;
    }

    Ok(report)
}

fn claude_md_template(workspace_name: &str) -> String {
    format!(
        "# {workspace_name}\n\n\
         ## Que es\n\
         <!-- describi brevemente el proposito del workspace -->\n\n\
         ## Stack\n\
         <!-- tecnologias, frameworks, lenguajes -->\n\n\
         ## Comandos comunes\n\
         ```bash\n\
         # ...\n\
         ```\n\n\
         ## Convenciones\n\
         <!-- idioma del codigo, commits, estilo -->\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ws() -> TempDir {
        TempDir::new().unwrap()
    }

    #[test]
    fn empty_dir_is_bare() {
        let tmp = ws();
        let s = inspect(tmp.path());
        assert!(!s.has_claude_md);
        assert!(!s.has_claude_dir);
        assert!(!s.has_mcp_json);
        assert!(!s.has_specs_dir);
        assert!(!s.has_root_git);
        assert!(!s.has_child_git_dirs);
        assert!(s.is_bare());
        assert!(s.can_git_init());
    }

    #[test]
    fn dir_with_claude_md_is_not_bare() {
        let tmp = ws();
        fs::write(tmp.path().join("CLAUDE.md"), "# x").unwrap();
        let s = inspect(tmp.path());
        assert!(s.has_claude_md);
        assert!(!s.is_bare());
    }

    #[test]
    fn dir_with_only_claude_dir_is_not_bare() {
        let tmp = ws();
        fs::create_dir_all(tmp.path().join(".claude/skills")).unwrap();
        let s = inspect(tmp.path());
        assert!(s.has_claude_dir);
        assert!(!s.is_bare());
    }

    #[test]
    fn dir_with_only_mcp_json_is_not_bare() {
        let tmp = ws();
        fs::write(tmp.path().join(".mcp.json"), "{}").unwrap();
        let s = inspect(tmp.path());
        assert!(s.has_mcp_json);
        assert!(!s.is_bare());
    }

    #[test]
    fn detects_root_git() {
        let tmp = ws();
        fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let s = inspect(tmp.path());
        assert!(s.has_root_git);
        assert!(!s.has_child_git_dirs);
        assert!(!s.can_git_init(), "no proponer git init si ya hay .git");
    }

    #[test]
    fn detects_child_git_subdirs() {
        let tmp = ws();
        fs::create_dir_all(tmp.path().join("repo-a/.git")).unwrap();
        fs::create_dir_all(tmp.path().join("repo-b/.git")).unwrap();
        let s = inspect(tmp.path());
        assert!(!s.has_root_git);
        assert!(s.has_child_git_dirs);
        assert!(
            !s.can_git_init(),
            "con repos hijos git, evitar git init en el padre"
        );
    }

    #[test]
    fn child_git_detection_ignores_root_git_subdir() {
        let tmp = ws();
        // Solo `.git` del root, ningun subdir con git propio.
        fs::create_dir_all(tmp.path().join(".git")).unwrap();
        let s = inspect(tmp.path());
        assert!(s.has_root_git);
        assert!(
            !s.has_child_git_dirs,
            "el .git del root no cuenta como child git"
        );
    }

    #[test]
    fn recommended_marks_all_missing() {
        let tmp = ws();
        let s = inspect(tmp.path());
        let opts = PrepareOpts::recommended_for(&s);
        assert!(opts.claude_md);
        assert!(opts.claude_dir);
        assert!(opts.mcp_json);
        assert!(opts.specs_dir);
        assert!(opts.git_init);
    }

    #[test]
    fn recommended_skips_existing_items() {
        let tmp = ws();
        fs::write(tmp.path().join("CLAUDE.md"), "x").unwrap();
        fs::create_dir_all(tmp.path().join("specs")).unwrap();
        let s = inspect(tmp.path());
        let opts = PrepareOpts::recommended_for(&s);
        assert!(!opts.claude_md);
        assert!(opts.claude_dir);
        assert!(opts.mcp_json);
        assert!(!opts.specs_dir);
    }

    #[test]
    fn recommended_disables_git_init_when_child_git_present() {
        let tmp = ws();
        fs::create_dir_all(tmp.path().join("repo-a/.git")).unwrap();
        let s = inspect(tmp.path());
        let opts = PrepareOpts::recommended_for(&s);
        assert!(!opts.git_init);
    }

    #[test]
    fn prepare_creates_all_items_when_bare() {
        let tmp = ws();
        let opts = PrepareOpts {
            claude_md: true,
            claude_dir: true,
            mcp_json: true,
            specs_dir: true,
            git_init: false,
        };
        let report = prepare(tmp.path(), opts).unwrap();
        assert!(tmp.path().join("CLAUDE.md").is_file());
        assert!(tmp.path().join(".claude/skills").is_dir());
        assert!(tmp.path().join(".claude/agents").is_dir());
        assert!(tmp.path().join("specs").is_dir());
        assert!(tmp.path().join(".mcp.json").is_file());
        assert_eq!(report.created.len(), 4);
        assert!(!report.git_initialized);
    }

    #[test]
    fn prepare_does_not_overwrite_existing_claude_md() {
        let tmp = ws();
        fs::write(tmp.path().join("CLAUDE.md"), "MIO").unwrap();
        let opts = PrepareOpts {
            claude_md: true,
            claude_dir: false,
            mcp_json: false,
            specs_dir: false,
            git_init: false,
        };
        let report = prepare(tmp.path(), opts).unwrap();
        let content = fs::read_to_string(tmp.path().join("CLAUDE.md")).unwrap();
        assert_eq!(content, "MIO", "no debe pisar el archivo del user");
        assert!(report.created.is_empty());
    }

    #[test]
    fn prepare_claude_md_uses_workspace_name() {
        let tmp = ws();
        let path = tmp.path().join("mi-workspace");
        fs::create_dir_all(&path).unwrap();
        let opts = PrepareOpts {
            claude_md: true,
            claude_dir: false,
            mcp_json: false,
            specs_dir: false,
            git_init: false,
        };
        prepare(&path, opts).unwrap();
        let content = fs::read_to_string(path.join("CLAUDE.md")).unwrap();
        assert!(
            content.starts_with("# mi-workspace"),
            "el titulo debe usar el dir name, fue: {content:?}"
        );
    }

    #[test]
    fn prepare_with_git_init_creates_dot_git() {
        let tmp = ws();
        let opts = PrepareOpts {
            claude_md: false,
            claude_dir: false,
            mcp_json: false,
            specs_dir: false,
            git_init: true,
        };
        let report = prepare(tmp.path(), opts).unwrap();
        assert!(tmp.path().join(".git").exists());
        assert!(report.git_initialized);
    }

    #[test]
    fn prepare_git_init_is_idempotent_when_already_git() {
        let tmp = ws();
        // primer init crea
        let _ = prepare(
            tmp.path(),
            PrepareOpts {
                claude_md: false,
                claude_dir: false,
                mcp_json: false,
                specs_dir: false,
                git_init: true,
            },
        )
        .unwrap();
        // segundo init no debe re-inicializar
        let report = prepare(
            tmp.path(),
            PrepareOpts {
                claude_md: false,
                claude_dir: false,
                mcp_json: false,
                specs_dir: false,
                git_init: true,
            },
        )
        .unwrap();
        assert!(
            !report.git_initialized,
            "git init solo se ejecuta si no hay .git ya"
        );
    }
}
