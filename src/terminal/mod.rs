//! Terminal embebido por job, basado en `egui_term` (alacritty_terminal + egui).
//!
//! Cada job tiene su propio `TerminalBackend` con su PTY. El backend vive
//! dentro de `App` en un `HashMap<JobId, JobTerminal>` y se crea perezosamente
//! la primera vez que el job se renderiza. La PTY se cierra cuando se hace
//! `drop` del backend (al cerrar el job, p.ej.).

use std::path::Path;
use std::sync::mpsc::Sender;

use anyhow::{Context, Result};
use egui_term::{BackendSettings, PtyEvent, TerminalBackend};

/// Wrapper alrededor de `TerminalBackend` que mantiene el id del job para
/// poder enrutar eventos de PTY de vuelta a la app.
pub struct JobTerminal {
    pub backend: TerminalBackend,
}

impl JobTerminal {
    /// Crea un terminal nuevo para el job. `shell_command` es el binario que
    /// se va a ejecutar dentro del PTY (e.g. `cmd.exe`, `bash`, `claude`).
    /// `working_directory` es la ruta donde el PTY debe arrancar.
    pub fn spawn(
        backend_id: u64,
        ctx: egui::Context,
        pty_tx: Sender<(u64, PtyEvent)>,
        shell_command: &str,
        args: Vec<String>,
        working_directory: &Path,
    ) -> Result<Self> {
        let settings = build_backend_settings(shell_command, args, working_directory);
        let backend = TerminalBackend::new(backend_id, ctx, pty_tx, settings)
            .map_err(|e| anyhow::anyhow!("creando TerminalBackend: {e}"))
            .context("spawning PTY backend")?;
        Ok(Self { backend })
    }
}

/// Construye los `BackendSettings` para `TerminalBackend::new`.
/// Extraido como función pura para que sea testeable sin spawnear PTYs.
fn build_backend_settings(
    shell_command: &str,
    args: Vec<String>,
    working_directory: &Path,
) -> BackendSettings {
    BackendSettings {
        shell: shell_command.to_string(),
        args,
        working_directory: Some(working_directory.to_path_buf()),
    }
}

/// Devuelve el shell por defecto del sistema:
/// - Windows: `cmd.exe`
/// - Unix: `$SHELL` o `/bin/bash` como fallback
pub fn default_shell() -> String {
    if cfg!(windows) {
        "cmd.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn build_backend_settings_includes_command_and_args() {
        let settings = build_backend_settings(
            "claude",
            vec!["--name".into(), "feat/x".into()],
            &PathBuf::from("/tmp/repo"),
        );
        assert_eq!(settings.shell, "claude");
        assert_eq!(settings.args, vec!["--name", "feat/x"]);
        assert_eq!(settings.working_directory, Some(PathBuf::from("/tmp/repo")));
    }

    #[test]
    fn build_backend_settings_with_no_args_is_empty_vec() {
        let settings = build_backend_settings("bash", vec![], &PathBuf::from("/home"));
        assert!(settings.args.is_empty());
    }

    #[test]
    fn default_shell_is_non_empty() {
        let s = default_shell();
        assert!(!s.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn default_shell_on_windows_is_cmd() {
        assert_eq!(default_shell(), "cmd.exe");
    }

    #[cfg(unix)]
    #[test]
    fn default_shell_on_unix_falls_back_to_bin_bash() {
        // SAFETY: el test es single-threaded para tocar env vars.
        let original = std::env::var("SHELL").ok();
        unsafe {
            std::env::remove_var("SHELL");
        }
        assert_eq!(default_shell(), "/bin/bash");
        if let Some(v) = original {
            unsafe {
                std::env::set_var("SHELL", v);
            }
        }
    }
}
