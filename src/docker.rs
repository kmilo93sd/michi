//! Deteccion de Docker disponible para decidir el modo de aislamiento de una
//! sesion: sandbox en contenedor (si hay Docker) o nativo (fallback).
//!
//! El modelo container-first de michi PREFIERE el contenedor pero NO lo exige
//! (regla 4 cross-platform): si Docker no esta corriendo, michi cae al camino
//! nativo (worktree + PTY directo). Este modulo toma esa decision.
//!
//! La parte determinista (clasificar la salida de `docker version`) es una
//! funcion pura testeable; el shell-out es un wrapper delgado encima.

use std::process::Command;

/// Estado de Docker en la maquina, desde el punto de vista de michi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerStatus {
    /// El daemon responde. Guardamos la version del server para mostrarla.
    Available { server_version: String },
    /// El binario falta, el daemon no corre, o la salida fue ininteligible.
    Unavailable { reason: UnavailableReason },
}

/// Por que michi no puede usar el modo contenedor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnavailableReason {
    /// No se encontro el binario `docker` en PATH.
    BinaryNotFound,
    /// El binario existe pero el daemon no respondio (Docker Desktop apagado,
    /// permisos, etc).
    DaemonNotResponding,
}

impl DockerStatus {
    /// `true` si michi puede usar el modo contenedor para una sesion nueva.
    pub fn is_available(&self) -> bool {
        matches!(self, DockerStatus::Available { .. })
    }
}

/// Corre `docker version` y clasifica el resultado. Nunca paniquea: cualquier
/// fallo se mapea a `Unavailable`.
pub fn detect_docker() -> DockerStatus {
    match Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
    {
        Ok(output) => classify_version_output(
            output.status.success(),
            &String::from_utf8_lossy(&output.stdout),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DockerStatus::Unavailable {
            reason: UnavailableReason::BinaryNotFound,
        },
        Err(_) => DockerStatus::Unavailable {
            reason: UnavailableReason::DaemonNotResponding,
        },
    }
}

/// Logica pura: dado si el comando tuvo exito y su stdout, decide el estado.
/// Cuando el daemon esta apagado, `docker version` sale con codigo != 0 y la
/// linea de Server.Version queda vacia → `Unavailable`.
pub fn classify_version_output(success: bool, stdout: &str) -> DockerStatus {
    let version = stdout.trim();
    if success && !version.is_empty() {
        DockerStatus::Available {
            server_version: version.to_string(),
        }
    } else {
        DockerStatus::Unavailable {
            reason: UnavailableReason::DaemonNotResponding,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_output_ok_is_available() {
        let s = classify_version_output(true, "27.3.1");
        assert_eq!(
            s,
            DockerStatus::Available {
                server_version: "27.3.1".to_string()
            }
        );
        assert!(s.is_available());
    }

    #[test]
    fn version_output_trims_whitespace() {
        let s = classify_version_output(true, "  27.3.1\n");
        assert_eq!(
            s,
            DockerStatus::Available {
                server_version: "27.3.1".to_string()
            }
        );
    }

    #[test]
    fn empty_version_is_unavailable() {
        let s = classify_version_output(true, "   \n");
        assert_eq!(
            s,
            DockerStatus::Unavailable {
                reason: UnavailableReason::DaemonNotResponding
            }
        );
        assert!(!s.is_available());
    }

    #[test]
    fn failed_command_is_unavailable_even_with_stdout() {
        // Daemon apagado: `docker version` sale != 0; aunque imprima algo del
        // cliente, no sirve para lanzar contenedores.
        let s = classify_version_output(false, "Client: 27.3.1\nServer: error");
        assert!(!s.is_available());
    }

    #[test]
    fn detect_docker_never_panics() {
        // Smoke test: corra o no Docker, devuelve un estado valido sin paniquear.
        let s = detect_docker();
        let _ = s.is_available();
    }
}
