//! Helpers para integrarse con el OS (abrir explorer / Finder / xdg-open, etc).

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Abre el path en el file manager nativo del OS:
/// - Windows: `explorer.exe`
/// - macOS: `open`
/// - Linux/otros: `xdg-open`
///
/// Es fire-and-forget: arranca el proceso y no espera. Falla si el binario
/// no existe en PATH.
pub fn open_folder(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("path no existe: {}", path.display());
    }

    let cmd = file_manager_command();
    Command::new(cmd)
        .arg(path)
        .spawn()
        .with_context(|| format!("invocando `{cmd}` con {}", path.display()))?;
    Ok(())
}

/// Construye el comando para matar un proceso por PID. En Windows usa
/// `taskkill /F /T` (mata el arbol de hijos); en unix `kill -9` (un PID).
/// Pure: testeable sin matar nada.
fn kill_command(pid: u32) -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        (
            "taskkill",
            vec!["/F".into(), "/T".into(), "/PID".into(), pid.to_string()],
        )
    } else {
        ("kill", vec!["-9".into(), pid.to_string()])
    }
}

/// Mata una sesion y su arbol de procesos. En Windows `taskkill /F /T` sobre el
/// PID raiz baja todo el arbol de una; en unix mata cada PID del arbol (sin
/// pgid no hay kill recursivo simple). Best-effort: ignora procesos ya muertos.
pub fn kill_session(root_pid: u32, tree_pids: &[u32]) -> Result<()> {
    if cfg!(windows) {
        let (cmd, args) = kill_command(root_pid);
        Command::new(cmd)
            .args(&args)
            .status()
            .with_context(|| format!("matando sesion pid {root_pid}"))?;
    } else {
        for &pid in tree_pids {
            let (cmd, args) = kill_command(pid);
            let _ = Command::new(cmd).args(&args).status();
        }
    }
    Ok(())
}

/// Devuelve el binario del file manager segun el OS.
fn file_manager_command() -> &'static str {
    if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_manager_command_is_non_empty() {
        assert!(!file_manager_command().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn file_manager_on_windows_is_explorer() {
        assert_eq!(file_manager_command(), "explorer.exe");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn file_manager_on_macos_is_open() {
        assert_eq!(file_manager_command(), "open");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn file_manager_on_linux_is_xdg_open() {
        assert_eq!(file_manager_command(), "xdg-open");
    }

    #[test]
    fn open_folder_returns_err_for_nonexistent_path() {
        let err = open_folder(Path::new("/this/path/never/exists/abc")).unwrap_err();
        assert!(err.to_string().contains("no existe"));
    }

    #[test]
    fn kill_command_includes_the_pid() {
        let (_, args) = kill_command(4321);
        assert!(args.iter().any(|a| a == "4321"));
    }

    #[cfg(windows)]
    #[test]
    fn kill_command_on_windows_uses_taskkill_tree() {
        let (cmd, args) = kill_command(1234);
        assert_eq!(cmd, "taskkill");
        assert_eq!(args, vec!["/F", "/T", "/PID", "1234"]);
    }

    #[cfg(not(windows))]
    #[test]
    fn kill_command_on_unix_uses_kill_minus9() {
        let (cmd, args) = kill_command(1234);
        assert_eq!(cmd, "kill");
        assert_eq!(args, vec!["-9", "1234"]);
    }
}
