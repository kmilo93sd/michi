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
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

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
/// Ruta donde se monta el binario del agente (claude) dentro del contenedor.
pub const CONTAINER_CLAUDE_PATH: &str = "/usr/local/bin/claude";

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
    /// Binario standalone de claude (Linux, arch-matched) a montar read-only en
    /// `CONTAINER_CLAUDE_PATH`. `None` = la imagen ya trae claude (o modo nativo).
    pub claude_binary_host: Option<PathBuf>,
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

    // Binario del agente (claude) inyectado read-only, sin rebuild de imagen.
    if let Some(claude) = &spec.claude_binary_host {
        args.push("-v".to_string());
        args.push(format!("{}:{}:ro", claude.display(), CONTAINER_CLAUDE_PATH));
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

/// Imagen base del contenedor segun el lenguaje detectado en el repo, por
/// archivos marcadores. Imagenes oficiales **slim** (todas glibc, no alpine,
/// para que el binario de claude montado corra). Fallback `debian:stable-slim`.
/// Liviano a proposito: nada de base universal monstruo (10GB+).
pub fn detect_base_image(repo_path: &Path) -> String {
    // Orden = prioridad. Marcadores mas especificos del runtime principal primero.
    const MARKERS: [(&str, &str); 6] = [
        ("Cargo.toml", "rust:slim"),
        ("package.json", "node:slim"),
        ("pyproject.toml", "python:slim"),
        ("requirements.txt", "python:slim"),
        ("go.mod", "golang:bookworm"),
        ("Gemfile", "ruby:slim"),
    ];
    for (marker, image) in MARKERS {
        if repo_path.join(marker).is_file() {
            return image.to_string();
        }
    }
    "debian:stable-slim".to_string()
}

/// Mapea el arch del host (`std::env::consts::ARCH`) al "platform" de Docker.
/// Los contenedores corren el arch del host: amd64 en Win/Linux x86, arm64 en
/// Apple Silicon (corrección al supuesto "siempre x86_64").
pub fn arch_to_docker_platform(host_arch: &str) -> &'static str {
    match host_arch {
        "aarch64" => "arm64",
        // x86_64 y cualquier otro caen a amd64 (default seguro en Win/Linux x86).
        _ => "amd64",
    }
}

/// Ruta del binario de claude para Linux cacheado por michi, según el arch del
/// host (ej `<bin_dir>/claude-linux-amd64`).
pub fn claude_binary_path(bin_dir: &Path, host_arch: &str) -> PathBuf {
    bin_dir.join(format!(
        "claude-linux-{}",
        arch_to_docker_platform(host_arch)
    ))
}

/// Argumentos de `docker` para extraer el binario standalone de claude de un
/// contenedor throwaway: instala claude en una base debian y deja el binario
/// (deref del symlink) en `<out_dir>/<target_filename>`. Pura (testeable).
pub fn build_extract_args(out_dir: &Path, target_filename: &str) -> Vec<String> {
    let script = format!(
        "apt-get update -qq && apt-get install -y -qq curl ca-certificates >/dev/null 2>&1 \
         && curl -fsSL https://claude.ai/install.sh -o /tmp/i.sh && bash /tmp/i.sh >/dev/null 2>&1 \
         && cp -L /root/.local/bin/claude /out/{target_filename}"
    );
    vec![
        "run".to_string(),
        "--rm".to_string(),
        "-v".to_string(),
        format!("{}:/out", out_dir.display()),
        "debian:stable-slim".to_string(),
        "bash".to_string(),
        "-c".to_string(),
        script,
    ]
}

/// Comando a correr dentro de la sesion: `claude`, con la tarea inicial como
/// primer prompt si la hay (no vacía).
pub fn build_session_command(initial_task: Option<&str>) -> Vec<String> {
    match initial_task.map(str::trim) {
        Some(task) if !task.is_empty() => vec!["claude".to_string(), task.to_string()],
        _ => vec!["claude".to_string()],
    }
}

/// Garantiza que el binario de claude para Linux esté cacheado en `bin_dir`.
/// Si ya existe, lo devuelve; si no, lo extrae con `docker` (lento la 1a vez).
pub fn ensure_claude_binary(bin_dir: &Path, host_arch: &str) -> Result<PathBuf> {
    let target = claude_binary_path(bin_dir, host_arch);
    if target.is_file() {
        return Ok(target);
    }
    std::fs::create_dir_all(bin_dir).with_context(|| format!("creando {}", bin_dir.display()))?;
    let filename = target
        .file_name()
        .and_then(|n| n.to_str())
        .context("nombre de binario inválido")?;
    let status = Command::new("docker")
        .args(build_extract_args(bin_dir, filename))
        .status()
        .context("ejecutando docker para extraer el binario de claude")?;
    if !status.success() {
        bail!("la extracción del binario de claude falló (docker {status})");
    }
    if !target.is_file() {
        bail!("docker no dejó el binario en {}", target.display());
    }
    Ok(target)
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
            claude_binary_host: Some(PathBuf::from("/host/.michi/bin/claude-linux-amd64")),
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

    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), "x").unwrap();
    }

    #[test]
    fn image_rust_for_cargo_toml() {
        let tmp = tempfile::TempDir::new().unwrap();
        touch(tmp.path(), "Cargo.toml");
        assert_eq!(detect_base_image(tmp.path()), "rust:slim");
    }

    #[test]
    fn image_node_for_package_json() {
        let tmp = tempfile::TempDir::new().unwrap();
        touch(tmp.path(), "package.json");
        assert_eq!(detect_base_image(tmp.path()), "node:slim");
    }

    #[test]
    fn image_python_for_pyproject_or_requirements() {
        let tmp = tempfile::TempDir::new().unwrap();
        touch(tmp.path(), "requirements.txt");
        assert_eq!(detect_base_image(tmp.path()), "python:slim");

        let tmp2 = tempfile::TempDir::new().unwrap();
        touch(tmp2.path(), "pyproject.toml");
        assert_eq!(detect_base_image(tmp2.path()), "python:slim");
    }

    #[test]
    fn image_fallback_debian_for_unknown() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(detect_base_image(tmp.path()), "debian:stable-slim");
    }

    #[test]
    fn image_rust_wins_over_node_when_both_present() {
        // Prioridad por orden de marcadores: un repo Rust con tooling node
        // (scripts) sigue siendo Rust.
        let tmp = tempfile::TempDir::new().unwrap();
        touch(tmp.path(), "Cargo.toml");
        touch(tmp.path(), "package.json");
        assert_eq!(detect_base_image(tmp.path()), "rust:slim");
    }

    #[test]
    fn arch_maps_to_docker_platform() {
        assert_eq!(arch_to_docker_platform("x86_64"), "amd64");
        assert_eq!(arch_to_docker_platform("aarch64"), "arm64");
        assert_eq!(arch_to_docker_platform("mips"), "amd64");
    }

    #[test]
    fn claude_binary_path_uses_arch_suffix() {
        assert_eq!(
            claude_binary_path(Path::new("/m/bin"), "aarch64"),
            PathBuf::from("/m/bin/claude-linux-arm64")
        );
        assert_eq!(
            claude_binary_path(Path::new("/m/bin"), "x86_64"),
            PathBuf::from("/m/bin/claude-linux-amd64")
        );
    }

    #[test]
    fn run_args_mount_claude_binary_read_only() {
        let a = build_run_args(&sample_spec());
        let expected = format!(
            "{}:{}:ro",
            PathBuf::from("/host/.michi/bin/claude-linux-amd64").display(),
            CONTAINER_CLAUDE_PATH
        );
        assert!(a.contains(&expected), "args: {a:?}");
    }

    #[test]
    fn run_args_omit_claude_binary_when_none() {
        let mut spec = sample_spec();
        spec.claude_binary_host = None;
        let a = build_run_args(&spec);
        assert!(!a.iter().any(|x| x.contains(CONTAINER_CLAUDE_PATH)));
    }

    #[test]
    fn extract_args_mount_out_and_install_to_target() {
        let a = build_extract_args(Path::new("/m/bin"), "claude-linux-amd64");
        assert_eq!(a[0], "run");
        assert!(a.contains(&"--rm".to_string()));
        assert!(a.contains(&"debian:stable-slim".to_string()));
        let mount = format!("{}:/out", Path::new("/m/bin").display());
        assert!(a.contains(&mount), "args: {a:?}");
        // El script instala via install.sh y copia el binario al target en /out.
        let script = a.last().unwrap();
        assert!(script.contains("install.sh"));
        assert!(script.contains("/out/claude-linux-amd64"));
    }

    #[test]
    fn session_command_without_task_is_just_claude() {
        assert_eq!(build_session_command(None), vec!["claude".to_string()]);
        assert_eq!(
            build_session_command(Some("   ")),
            vec!["claude".to_string()]
        );
    }

    #[test]
    fn session_command_with_task_passes_it_as_prompt() {
        assert_eq!(
            build_session_command(Some("arregla el CORS")),
            vec!["claude".to_string(), "arregla el CORS".to_string()]
        );
    }

    #[test]
    fn ensure_claude_binary_short_circuits_when_present() {
        // Si el binario ya esta cacheado, devuelve su ruta sin tocar docker.
        let tmp = tempfile::TempDir::new().unwrap();
        let expected = claude_binary_path(tmp.path(), "x86_64");
        std::fs::write(&expected, "fake-binary").unwrap();
        let got = ensure_claude_binary(tmp.path(), "x86_64").unwrap();
        assert_eq!(got, expected);
    }
}
