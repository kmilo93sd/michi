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
}
