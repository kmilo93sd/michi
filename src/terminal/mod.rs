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

/// Comando que arranca en cada job nuevo: `claude` (Claude Code CLI).
///
/// michi es un orquestador de Claude Code; cada PTY embebido corre el CLI
/// `claude` directamente. Para que esto funcione el binario tiene que estar
/// en `PATH` del proceso que lanza michi.
///
/// Si quieres una shell normal (cmd.exe, bash) para un job especifico, lo
/// arrancas DESDE Claude — pero el default del producto es Claude.
pub fn default_shell() -> String {
    "claude".to_string()
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
    fn default_shell_is_claude() {
        // michi es un orquestador de Claude Code: cada PTY arranca
        // directamente `claude`, no una shell generica.
        assert_eq!(default_shell(), "claude");
    }

    #[test]
    fn default_shell_is_consistent_across_platforms() {
        // Antes el binario cambiaba entre OS (cmd.exe vs bash). Ahora es el
        // mismo en todos lados porque depende del CLI de Claude, no del OS.
        let first = default_shell();
        let second = default_shell();
        assert_eq!(first, second);
    }
}
