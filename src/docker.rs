//! Deteccion de Docker disponible para decidir el modo de aislamiento de una
//! sesion: sandbox en contenedor (si hay Docker) o nativo (fallback).
//!
//! El modelo container-first de michi PREFIERE el contenedor pero NO lo exige
//! (regla 4 cross-platform): si Docker no esta corriendo, michi cae al camino
//! nativo (worktree + PTY directo). Este modulo toma esa decision.
//!
//! La parte determinista (clasificar la salida de `docker version`) es una
//! funcion pura testeable; el shell-out es un wrapper delgado encima.

use std::collections::BTreeMap;
use std::path::PathBuf;
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

/// Directorio dentro del contenedor donde se monta el worktree de la sesion.
pub const CONTAINER_WORKDIR: &str = "/work";
/// Ruta dentro del contenedor donde se montan las credenciales de Claude
/// (read-only). El resto de `/root/.claude` queda escribible para que claude
/// guarde sesiones/cache adentro.
pub const CONTAINER_CREDS_PATH: &str = "/root/.claude/.credentials.json";

/// Como lanzar una sesion managed dentro de un contenedor (Fase D).
#[derive(Debug, Clone)]
pub struct ContainerSpec {
    /// Nombre del contenedor (ej "michi-<session-id>").
    pub name: String,
    /// Imagen base (ej "michi-base").
    pub image: String,
    /// Worktree del host a montar en `CONTAINER_WORKDIR`.
    pub worktree_host: PathBuf,
    /// Archivo de credenciales de Claude a montar read-only (token OAuth).
    pub creds_host: Option<PathBuf>,
    /// Puertos a publicar: `(host, contenedor)`.
    pub ports: Vec<(u16, u16)>,
    /// Variables de entorno a inyectar (ej `DATABASE_URL`, `PORT_*`).
    pub env: Vec<(String, String)>,
    /// Limite de memoria (ej "4g"). `None` = sin limite.
    pub memory: Option<String>,
    /// Limite de CPUs (ej "2"). `None` = sin limite.
    pub cpus: Option<String>,
    /// Comando a correr adentro (ej `["claude", "<tarea>"]`).
    pub command: Vec<String>,
}

/// Construye los argumentos para `docker` (sin el "docker" inicial) que lanzan
/// la sesion en un contenedor. El PTY de michi spawnea `docker` con estos args,
/// de modo que el terminal embebido queda conectado a lo que corre adentro.
///
/// Funcion pura (no toca el sistema) para poder testear el armado del comando
/// sin Docker real.
pub fn build_run_args(spec: &ContainerSpec) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "-it".to_string(),
        "--name".to_string(),
        spec.name.clone(),
    ];

    // Worktree montado en el workdir.
    args.push("-v".to_string());
    args.push(format!(
        "{}:{}",
        spec.worktree_host.display(),
        CONTAINER_WORKDIR
    ));

    // Credenciales de Claude, read-only (solo el archivo del token).
    if let Some(creds) = &spec.creds_host {
        args.push("-v".to_string());
        args.push(format!("{}:{}:ro", creds.display(), CONTAINER_CREDS_PATH));
    }

    // Puertos publicados (host -> contenedor).
    for (host, container) in &spec.ports {
        args.push("-p".to_string());
        args.push(format!("{host}:{container}"));
    }

    // Variables de entorno inyectadas.
    for (key, value) in &spec.env {
        args.push("-e".to_string());
        args.push(format!("{key}={value}"));
    }

    // Limites de recursos (si los hay).
    if let Some(memory) = &spec.memory {
        args.push("--memory".to_string());
        args.push(memory.clone());
    }
    if let Some(cpus) = &spec.cpus {
        args.push("--cpus".to_string());
        args.push(cpus.clone());
    }

    // Workdir, imagen y comando al final.
    args.push("-w".to_string());
    args.push(CONTAINER_WORKDIR.to_string());
    args.push(spec.image.clone());
    args.extend(spec.command.iter().cloned());

    args
}

/// Como termino lanzandose una sesion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    /// Dentro de un contenedor Docker (modo preferido).
    Container,
    /// Nativa en el host (fallback cuando no hay Docker).
    Native,
}

/// Plan concreto para lanzar una sesion via PTY. Mapea 1:1 a los parametros de
/// `terminal::JobTerminal::spawn` (command, args, env, working_directory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// En que modo se resolvio (para mostrarlo en la UI).
    pub mode: LaunchMode,
    /// Programa a spawnear: `docker` en contenedor, el comando real en nativo.
    pub command: String,
    /// Argumentos del programa.
    pub args: Vec<String>,
    /// Env vars a inyectar via shell wrapper. Vacio en contenedor (las vars van
    /// por `-e` dentro de los args de `docker run`).
    pub env: BTreeMap<String, String>,
    /// Working dir del PTY.
    pub working_directory: PathBuf,
}

/// Decide como lanzar la sesion: en contenedor si Docker esta disponible, o
/// nativa (worktree + comando directo) como fallback. Implementa la
/// degradacion de la regla 4 (Docker preferido, no requerido). Funcion pura.
pub fn plan_launch(docker: &DockerStatus, spec: &ContainerSpec) -> LaunchPlan {
    if docker.is_available() {
        LaunchPlan {
            mode: LaunchMode::Container,
            command: "docker".to_string(),
            args: build_run_args(spec),
            // En contenedor el env se inyecta por `-e` (ya dentro de los args).
            env: BTreeMap::new(),
            working_directory: spec.worktree_host.clone(),
        }
    } else {
        // Fallback nativo: corremos el comando real directo en el worktree, con
        // el env (puertos, etc) via shell wrapper de `JobTerminal::spawn`.
        let mut parts = spec.command.iter();
        let command = parts
            .next()
            .cloned()
            .unwrap_or_else(|| "claude".to_string());
        LaunchPlan {
            mode: LaunchMode::Native,
            command,
            args: parts.cloned().collect(),
            env: spec.env.iter().cloned().collect(),
            working_directory: spec.worktree_host.clone(),
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

    fn sample_spec() -> ContainerSpec {
        ContainerSpec {
            name: "michi-abc".to_string(),
            image: "michi-base".to_string(),
            worktree_host: PathBuf::from("/host/wt"),
            creds_host: Some(PathBuf::from("/host/.creds.json")),
            ports: vec![(4100, 8080)],
            env: vec![(
                "DATABASE_URL".to_string(),
                "postgres://x/session_abc".to_string(),
            )],
            memory: Some("4g".to_string()),
            cpus: Some("2".to_string()),
            command: vec!["claude".to_string(), "arregla el bug".to_string()],
        }
    }

    /// Valor que sigue a `flag` en el vector de args (para `--name X`, etc).
    fn value_after(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    }

    #[test]
    fn run_args_start_with_run_rm_it_and_name() {
        let a = build_run_args(&sample_spec());
        assert_eq!(a[0], "run");
        assert!(a.contains(&"--rm".to_string()));
        assert!(a.contains(&"-it".to_string()));
        assert_eq!(value_after(&a, "--name"), Some("michi-abc".to_string()));
    }

    #[test]
    fn run_args_mount_worktree_to_workdir() {
        let a = build_run_args(&sample_spec());
        let expected = format!(
            "{}:{}",
            PathBuf::from("/host/wt").display(),
            CONTAINER_WORKDIR
        );
        assert!(a.contains(&expected), "args: {a:?}");
        assert_eq!(value_after(&a, "-w"), Some(CONTAINER_WORKDIR.to_string()));
    }

    #[test]
    fn run_args_mount_creds_read_only() {
        let a = build_run_args(&sample_spec());
        let expected = format!(
            "{}:{}:ro",
            PathBuf::from("/host/.creds.json").display(),
            CONTAINER_CREDS_PATH
        );
        assert!(a.contains(&expected), "args: {a:?}");
    }

    #[test]
    fn run_args_omit_creds_when_none() {
        let mut spec = sample_spec();
        spec.creds_host = None;
        let a = build_run_args(&spec);
        assert!(!a.iter().any(|x| x.contains(CONTAINER_CREDS_PATH)));
    }

    #[test]
    fn run_args_publish_ports() {
        let a = build_run_args(&sample_spec());
        let pos = a.iter().position(|x| x == "-p").unwrap();
        assert_eq!(a[pos + 1], "4100:8080");
    }

    #[test]
    fn run_args_inject_env() {
        let a = build_run_args(&sample_spec());
        let pos = a.iter().position(|x| x == "-e").unwrap();
        assert_eq!(a[pos + 1], "DATABASE_URL=postgres://x/session_abc");
    }

    #[test]
    fn run_args_include_resource_caps() {
        let a = build_run_args(&sample_spec());
        assert_eq!(value_after(&a, "--memory"), Some("4g".to_string()));
        assert_eq!(value_after(&a, "--cpus"), Some("2".to_string()));
    }

    #[test]
    fn run_args_omit_caps_when_none() {
        let mut spec = sample_spec();
        spec.memory = None;
        spec.cpus = None;
        let a = build_run_args(&spec);
        assert!(!a.contains(&"--memory".to_string()));
        assert!(!a.contains(&"--cpus".to_string()));
    }

    #[test]
    fn run_args_end_with_image_then_command() {
        let a = build_run_args(&sample_spec());
        let img = a.iter().position(|x| x == "michi-base").unwrap();
        assert_eq!(a[img + 1], "claude");
        assert_eq!(a[img + 2], "arregla el bug");
        // La imagen viene justo despues de `-w <workdir>`.
        assert_eq!(a[img - 2], "-w");
        assert_eq!(a[img - 1], CONTAINER_WORKDIR);
    }

    #[test]
    fn plan_uses_container_when_docker_available() {
        let docker = DockerStatus::Available {
            server_version: "27.3.1".into(),
        };
        let plan = plan_launch(&docker, &sample_spec());
        assert_eq!(plan.mode, LaunchMode::Container);
        assert_eq!(plan.command, "docker");
        assert_eq!(plan.args, build_run_args(&sample_spec()));
        assert!(
            plan.env.is_empty(),
            "en contenedor el env va por -e dentro de los args"
        );
        assert_eq!(plan.working_directory, PathBuf::from("/host/wt"));
    }

    #[test]
    fn plan_falls_back_to_native_when_docker_unavailable() {
        let docker = DockerStatus::Unavailable {
            reason: UnavailableReason::BinaryNotFound,
        };
        let plan = plan_launch(&docker, &sample_spec());
        assert_eq!(plan.mode, LaunchMode::Native);
        assert_eq!(plan.command, "claude");
        assert_eq!(plan.args, vec!["arregla el bug".to_string()]);
        assert_eq!(plan.working_directory, PathBuf::from("/host/wt"));
    }

    #[test]
    fn native_plan_injects_env_for_shell_wrapper() {
        let docker = DockerStatus::Unavailable {
            reason: UnavailableReason::DaemonNotResponding,
        };
        let plan = plan_launch(&docker, &sample_spec());
        assert_eq!(
            plan.env.get("DATABASE_URL").map(String::as_str),
            Some("postgres://x/session_abc")
        );
    }

    #[test]
    fn native_plan_defaults_command_to_claude_when_empty() {
        let mut spec = sample_spec();
        spec.command = vec![];
        let docker = DockerStatus::Unavailable {
            reason: UnavailableReason::BinaryNotFound,
        };
        let plan = plan_launch(&docker, &spec);
        assert_eq!(plan.command, "claude");
        assert!(plan.args.is_empty());
    }
}
