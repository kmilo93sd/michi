//! Persistencia de `AppState` en `~/.michi/state.json`.
//!
//! La app lee este archivo al boot y lo escribe debounced cada vez que
//! algo cambia. El write es atómico: se escribe a `state.json.tmp` y se
//! renombra al destino final, así un crash a mitad de write nunca deja
//! el archivo corrupto.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::state::{Job, Workspace};

/// Estado persistido de una sesion managed (la que michi lanza/controla), por
/// job-id. Permite que Detener/Reabrir sobreviva a reiniciar michi: al reabrir
/// se usa el `claude_session_id` para `claude --resume`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedSession {
    /// session_id de Claude (generado por michi, o el de la externa traida).
    pub claude_session_id: String,
    /// Si ya se lanzo (1a vez se crea con --session-id; al reabrir, --resume).
    pub started: bool,
    /// Forzar nativo (traidas: su historial esta indexado por la cwd del host).
    pub native: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AppState {
    #[serde(default)]
    pub workspaces: Vec<Workspace>,

    #[serde(default)]
    pub jobs: Vec<Job>,

    #[serde(default)]
    pub selected_job_id: Option<String>,

    #[serde(default)]
    pub collapsed_workspaces: HashSet<String>,

    /// Estado de sesiones managed por job-id (session_id + modo). Persistido
    /// para que Reabrir tras reiniciar michi recupere la conversacion (--resume).
    #[serde(default)]
    pub managed: HashMap<String, ManagedSession>,
}

impl AppState {
    /// `~/.michi/state.json`.
    pub fn default_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("no se pudo obtener home dir")?;
        Ok(home.join(".michi").join("state.json"))
    }

    /// Lee desde el path canónico. Si el archivo no existe devuelve default
    /// vacío. Si el parsing falla, log warn y default vacío (no romper la app
    /// al boot por un JSON corrupto).
    pub fn load_or_default() -> Self {
        match Self::try_load_from(Self::default_path()) {
            Ok(s) => s,
            Err(e) => {
                warn!("no se pudo cargar state.json, usando default: {e:#}");
                Self::default()
            }
        }
    }

    fn try_load_from(path_result: Result<PathBuf>) -> Result<Self> {
        let path = path_result?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw =
            fs::read_to_string(&path).with_context(|| format!("leyendo {}", path.display()))?;
        let state: Self =
            serde_json::from_str(&raw).with_context(|| format!("parseando {}", path.display()))?;
        Ok(state)
    }

    /// Escribe el state al path canónico de forma atómica (tmp + rename).
    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::default_path()?)
    }

    pub fn save_to(&self, path: &std::path::Path) -> Result<()> {
        let parent = path.parent().context("state path sin parent dir")?;
        fs::create_dir_all(parent).with_context(|| format!("creando {}", parent.display()))?;
        let json = serde_json::to_string_pretty(self).context("serializando AppState a JSON")?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &json).with_context(|| format!("escribiendo {}", tmp.display()))?;
        fs::rename(&tmp, path)
            .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
        info!(path = %path.display(), bytes = json.len(), "state guardado");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Job, JobStatus, Repo};
    use std::path::PathBuf;
    use std::time::SystemTime;
    use tempfile::TempDir;

    fn sample_state() -> AppState {
        let mut collapsed_ws = HashSet::new();
        collapsed_ws.insert("ws-a".into());

        AppState {
            workspaces: vec![Workspace {
                id: "ws-a".into(),
                name: "alpha".into(),
                path: PathBuf::from("/tmp/alpha"),
                claude_md_present: true,
                specs_count: 3,
                skills_count: 1,
                prep_dismissed: false,
                repos: vec![Repo {
                    id: "repo-a".into(),
                    name: "alpha-app".into(),
                    path: PathBuf::from("/tmp/alpha/alpha-app"),
                    claude_md_present: false,
                    skills_count: 0,
                }],
            }],
            jobs: vec![Job {
                id: "job-1".into(),
                workspace: "alpha".into(),
                repo: "alpha-app".into(),
                branch: "feat/x".into(),
                worktree_path: PathBuf::from("/tmp/alpha-wt/feat-x"),
                status: JobStatus::Idle,
                files_changed: 2,
                last_activity: SystemTime::now(),
                port_range_start: 0,
            }],
            selected_job_id: Some("job-1".into()),
            collapsed_workspaces: collapsed_ws,
            managed: {
                let mut m = HashMap::new();
                m.insert(
                    "job-1".to_string(),
                    ManagedSession {
                        claude_session_id: "sess-abc".into(),
                        started: true,
                        native: false,
                    },
                );
                m
            },
        }
    }

    #[test]
    fn default_state_is_empty() {
        let s = AppState::default();
        assert!(s.workspaces.is_empty());
        assert!(s.jobs.is_empty());
        assert!(s.selected_job_id.is_none());
        assert!(s.collapsed_workspaces.is_empty());
        assert!(s.managed.is_empty());
    }

    #[test]
    fn managed_session_serializes_roundtrip() {
        let mut m = HashMap::new();
        m.insert(
            "j".to_string(),
            ManagedSession {
                claude_session_id: "s1".into(),
                started: true,
                native: true,
            },
        );
        let state = AppState {
            managed: m,
            ..AppState::default()
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: AppState = serde_json::from_str(&json).unwrap();
        let got = back.managed.get("j").expect("managed roundtrip");
        assert_eq!(got.claude_session_id, "s1");
        assert!(got.started && got.native);
    }

    #[test]
    fn save_and_load_roundtrip() -> Result<()> {
        let tmp = TempDir::new()?;
        let path = tmp.path().join("nested").join("state.json");

        let original = sample_state();
        original.save_to(&path)?;
        assert!(path.exists(), "state.json debe existir tras save");
        assert!(
            !path.with_extension("json.tmp").exists(),
            "el archivo tmp no debe quedar"
        );

        let raw = fs::read_to_string(&path)?;
        let loaded: AppState = serde_json::from_str(&raw)?;

        assert_eq!(loaded.workspaces.len(), 1);
        assert_eq!(loaded.workspaces[0].name, "alpha");
        assert_eq!(loaded.workspaces[0].repos.len(), 1);
        assert_eq!(loaded.workspaces[0].repos[0].name, "alpha-app");
        assert_eq!(loaded.jobs.len(), 1);
        assert_eq!(loaded.jobs[0].branch, "feat/x");
        assert_eq!(loaded.jobs[0].status, JobStatus::Idle);
        assert_eq!(loaded.selected_job_id.as_deref(), Some("job-1"));
        assert!(loaded.collapsed_workspaces.contains("ws-a"));
        let managed = loaded.managed.get("job-1").expect("managed job-1 persiste");
        assert_eq!(managed.claude_session_id, "sess-abc");
        assert!(managed.started);
        assert!(!managed.native);
        Ok(())
    }

    #[test]
    fn save_is_atomic_no_tmp_leftover() -> Result<()> {
        let tmp = TempDir::new()?;
        let path = tmp.path().join("state.json");
        sample_state().save_to(&path)?;
        let tmp_path = path.with_extension("json.tmp");
        assert!(!tmp_path.exists(), "tmp file no debe quedar tras rename");
        Ok(())
    }

    #[test]
    fn load_missing_file_returns_default() {
        let s = AppState::try_load_from(Ok(PathBuf::from("/this/does/not/exist/state.json")))
            .expect("no existe debe ser Ok(default)");
        assert!(s.workspaces.is_empty());
        assert!(s.jobs.is_empty());
    }

    #[test]
    fn load_corrupt_json_returns_err() -> Result<()> {
        let tmp = TempDir::new()?;
        let path = tmp.path().join("state.json");
        fs::write(&path, "{not valid json")?;
        let err = AppState::try_load_from(Ok(path)).unwrap_err();
        assert!(err.to_string().contains("parseando"), "{err}");
        Ok(())
    }

    #[test]
    fn missing_fields_use_defaults() -> Result<()> {
        let tmp = TempDir::new()?;
        let path = tmp.path().join("state.json");
        fs::write(&path, "{}")?;
        let loaded = AppState::try_load_from(Ok(path))?;
        assert!(loaded.workspaces.is_empty());
        assert!(loaded.selected_job_id.is_none());
        Ok(())
    }
}
