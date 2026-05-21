//! Terminal embebido por job, basado en `egui_term` (alacritty_terminal + egui).
//!
//! Cada job tiene su propio `TerminalBackend` con su PTY. El backend vive
//! dentro de `App` en un `HashMap<JobId, JobTerminal>` y se crea perezosamente
//! la primera vez que el job se renderiza. La PTY se cierra cuando se hace
//! `drop` del backend (al cerrar el job, p.ej.).
//!
//! ## Env vars per-PTY
//!
//! `BackendSettings` de egui_term solo expone `shell`, `args`,
//! `working_directory` — sin un campo de env vars. Para que cada PTY arranque
//! con variables distintas (puertos asignados por sesion), envolvemos el
//! comando objetivo en un shell wrapper que setea las vars antes de
//! invocarlo: `cmd.exe /c "set X=1 && claude"` en Windows, `bash -c
//! "X=1 exec claude"` en Unix. Esto agrega un proceso intermedio chico pero
//! es portable y libre de race conditions (vs `std::env::set_var`).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc::Sender;

use anyhow::{Context, Result};
use egui_term::{BackendSettings, PtyEvent, TerminalBackend};

/// Wrapper alrededor de `TerminalBackend` que mantiene el id del job para
/// poder enrutar eventos de PTY de vuelta a la app.
pub struct JobTerminal {
    pub backend: TerminalBackend,
    /// PID del proceso raiz del PTY (el shell wrapper o claude directo). Se
    /// usa para construir el arbol de procesos de la sesion y agregar sus
    /// recursos. `pty_id()` de egui_term, pese al nombre, es el OS PID.
    pub root_pid: u32,
}

impl JobTerminal {
    /// Crea un terminal nuevo para el job. `command` es el binario que se va
    /// a ejecutar dentro del PTY (e.g. `claude`). `env` se inyecta como env
    /// vars del proceso via shell wrapper (ver doc del modulo).
    pub fn spawn(
        backend_id: u64,
        ctx: egui::Context,
        pty_tx: Sender<(u64, PtyEvent)>,
        command: &str,
        args: Vec<String>,
        env: &BTreeMap<String, String>,
        working_directory: &Path,
    ) -> Result<Self> {
        let (shell, shell_args) = compose_command_with_env(command, &args, env);
        let settings = build_backend_settings(&shell, shell_args, working_directory);
        let backend = TerminalBackend::new(backend_id, ctx, pty_tx, settings)
            .map_err(|e| anyhow::anyhow!("creando TerminalBackend: {e}"))
            .context("spawning PTY backend")?;
        let root_pid = backend.pty_id();
        Ok(Self { backend, root_pid })
    }
}

/// Compone el `(shell, args)` final para arrancar `command` con las env vars
/// dadas. Si `env` esta vacio, devuelve `(command, args)` directo sin shell
/// wrapper. Si hay vars, envuelve en `cmd.exe /c` (Windows) o `bash -c`
/// (Unix) con las vars pre-seteadas.
pub fn compose_command_with_env(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> (String, Vec<String>) {
    if env.is_empty() {
        return (command.to_string(), args.to_vec());
    }
    if cfg!(windows) {
        let mut script = String::new();
        for (k, v) in env {
            script.push_str(&format!("set {k}={v}&& "));
        }
        script.push_str(command);
        for a in args {
            script.push(' ');
            script.push_str(a);
        }
        ("cmd.exe".to_string(), vec!["/c".into(), script])
    } else {
        let mut script = String::new();
        for (k, v) in env {
            script.push_str(&format!("{k}={v} "));
        }
        script.push_str("exec ");
        script.push_str(command);
        for a in args {
            script.push(' ');
            script.push_str(a);
        }
        ("bash".to_string(), vec!["-c".into(), script])
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

    #[test]
    fn compose_without_env_returns_command_directly() {
        let env = BTreeMap::new();
        let (shell, args) = compose_command_with_env("claude", &[], &env);
        assert_eq!(shell, "claude");
        assert!(args.is_empty());
    }

    #[test]
    fn compose_without_env_preserves_args() {
        let env = BTreeMap::new();
        let (shell, args) =
            compose_command_with_env("claude", &["--name".into(), "x".into()], &env);
        assert_eq!(shell, "claude");
        assert_eq!(args, vec!["--name", "x"]);
    }

    #[cfg(windows)]
    #[test]
    fn compose_with_env_on_windows_uses_cmd_set() {
        let mut env = BTreeMap::new();
        env.insert("PORT_API".into(), "4100".into());
        env.insert("PORT_WEB".into(), "4101".into());
        let (shell, args) = compose_command_with_env("claude", &[], &env);
        assert_eq!(shell, "cmd.exe");
        assert_eq!(args[0], "/c");
        // BTreeMap es ordenado por key alfabeticamente → PORT_API antes que PORT_WEB
        assert_eq!(args[1], "set PORT_API=4100&& set PORT_WEB=4101&& claude");
    }

    #[cfg(unix)]
    #[test]
    fn compose_with_env_on_unix_uses_bash_inline() {
        let mut env = BTreeMap::new();
        env.insert("PORT_API".into(), "4100".into());
        env.insert("PORT_WEB".into(), "4101".into());
        let (shell, args) = compose_command_with_env("claude", &[], &env);
        assert_eq!(shell, "bash");
        assert_eq!(args[0], "-c");
        assert_eq!(args[1], "PORT_API=4100 PORT_WEB=4101 exec claude");
    }

    #[test]
    fn compose_with_env_keeps_keys_sorted_for_determinism() {
        let mut env = BTreeMap::new();
        env.insert("ZED".into(), "9".into());
        env.insert("ALPHA".into(), "1".into());
        env.insert("BETA".into(), "2".into());
        let (_, args) = compose_command_with_env("claude", &[], &env);
        let script = &args[1];
        let alpha_pos = script.find("ALPHA").unwrap();
        let beta_pos = script.find("BETA").unwrap();
        let zed_pos = script.find("ZED").unwrap();
        assert!(alpha_pos < beta_pos && beta_pos < zed_pos);
    }
}
