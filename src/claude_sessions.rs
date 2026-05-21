//! Deteccion de sesiones de Claude Code corriendo en el sistema, incluso
//! las que NO fueron lanzadas por michi (terminales sueltas, VS Code, etc).
//!
//! Escanea la lista de procesos (de `resource_monitor`) y filtra los que son
//! el CLI de Claude Code de verdad — excluyendo la app de escritorio
//! (Electron, `WindowsApps\Claude_...`) y sus subprocesos (`--type=...`), y
//! la herramienta `claude-meter`. Por cada sesion detectada agrega sus
//! recursos (arbol de procesos) y extrae el `cwd` (para agrupar por
//! workspace) y el `--resume <id>` si la sesion fue retomada.

use std::path::PathBuf;

use serde::Deserialize;

use crate::resource_monitor::{self, ProcInfo, ProcessBreakdown, SessionResources};

/// Estado que Claude Code reporta para una sesion en
/// `~/.claude/sessions/<pid>.json`. Es el estado REAL (lo emite el propio
/// Claude), mas confiable que parsear el output del PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeStatus {
    /// Pensando / generando.
    Busy,
    /// Libre, esperando que le des una tarea.
    Idle,
    /// Esperando tu permiso o input (la que necesita tu atencion).
    Waiting,
    /// Corriendo un comando de shell.
    Shell,
    /// Estado desconocido o sin sessions file.
    Unknown,
}

impl ClaudeStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "busy" => ClaudeStatus::Busy,
            "idle" => ClaudeStatus::Idle,
            "waiting" => ClaudeStatus::Waiting,
            "shell" => ClaudeStatus::Shell,
            _ => ClaudeStatus::Unknown,
        }
    }

    /// Texto corto para mostrar en la card.
    pub fn label(self) -> &'static str {
        match self {
            ClaudeStatus::Busy => "pensando",
            ClaudeStatus::Idle => "libre",
            ClaudeStatus::Waiting => "esperando permiso",
            ClaudeStatus::Shell => "ejecutando",
            ClaudeStatus::Unknown => "?",
        }
    }
}

/// Metadata de una sesion leida de `~/.claude/sessions/<pid>.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMeta {
    pub session_id: String,
    pub cwd: Option<PathBuf>,
    pub status: ClaudeStatus,
    pub started_at_ms: u64,
}

/// Parsea el JSON de un sessions file. Funcion pura, testeable.
pub fn parse_session_meta(json: &str) -> Option<SessionMeta> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(rename = "sessionId")]
        session_id: String,
        cwd: Option<String>,
        #[serde(default)]
        status: String,
        #[serde(rename = "startedAt", default)]
        started_at: u64,
    }
    let raw: Raw = serde_json::from_str(json).ok()?;
    Some(SessionMeta {
        session_id: raw.session_id,
        cwd: raw.cwd.map(PathBuf::from),
        status: ClaudeStatus::parse(&raw.status),
        started_at_ms: raw.started_at,
    })
}

/// Lee `~/.claude/sessions/<pid>.json` para un PID. Glue sobre el FS.
/// Devuelve None si no existe o no parsea.
pub fn read_session_meta(pid: u32) -> Option<SessionMeta> {
    let home = dirs::home_dir()?;
    let path = home
        .join(".claude")
        .join("sessions")
        .join(format!("{pid}.json"));
    let raw = std::fs::read_to_string(path).ok()?;
    parse_session_meta(&raw)
}

/// Una sesion de Claude Code detectada en el sistema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedSession {
    /// PID del proceso CLI de Claude.
    pub pid: u32,
    /// Working directory de la sesion (en que proyecto trabaja). `None` si
    /// el OS no lo expuso.
    pub cwd: Option<PathBuf>,
    /// Session id de Claude si la sesion fue retomada con `--resume <id>`.
    pub resume_id: Option<String>,
    /// Recursos agregados del arbol de procesos de la sesion.
    pub resources: SessionResources,
    /// Desglose de procesos notables del arbol (shells, runtimes, docker).
    pub breakdown: ProcessBreakdown,
    /// Estado real reportado por Claude (`~/.claude/sessions/<pid>.json`).
    /// `Unknown` si no se pudo leer el sessions file.
    pub status: ClaudeStatus,
}

/// `true` si el proceso es el CLI de Claude Code (no la app de escritorio
/// ni un subproceso de Electron ni `claude-meter`).
///
/// Reglas:
/// - El nombre debe ser exactamente `claude` o `claude.exe` (descarta
///   `claude-meter.exe`).
/// - El ejecutable (cmd[0]) NO debe estar en `WindowsApps` (app desktop).
/// - No debe tener un arg `--type=...` (subproceso de Electron).
pub fn is_claude_cli(name: &str, cmd: &[String]) -> bool {
    let lname = name.to_lowercase();
    if lname != "claude" && lname != "claude.exe" {
        return false;
    }
    let exec = cmd.first().map(|s| s.to_lowercase()).unwrap_or_default();
    if exec.contains("windowsapps") {
        return false;
    }
    if cmd.iter().any(|a| a.starts_with("--type=")) {
        return false;
    }
    true
}

/// Extrae el id de `--resume <id>` del command line, si existe.
pub fn extract_resume_id(cmd: &[String]) -> Option<String> {
    let pos = cmd.iter().position(|a| a == "--resume")?;
    cmd.get(pos + 1).cloned()
}

/// Detecta todas las sesiones de Claude CLI en el snapshot de procesos.
/// Para cada una agrega los recursos de su arbol de descendientes y lee el
/// estado real desde `~/.claude/sessions/<pid>.json`.
pub fn detect_sessions(all: &[ProcInfo]) -> Vec<DetectedSession> {
    let mut sessions: Vec<DetectedSession> = all
        .iter()
        .filter(|p| is_claude_cli(&p.name, &p.cmd))
        .map(|p| {
            let subtree = resource_monitor::collect_subtree(all, p.pid);
            let status = read_session_meta(p.pid)
                .map(|m| m.status)
                .unwrap_or(ClaudeStatus::Unknown);
            DetectedSession {
                pid: p.pid,
                cwd: p.cwd.clone(),
                resume_id: extract_resume_id(&p.cmd),
                resources: resource_monitor::aggregate(&subtree),
                breakdown: resource_monitor::classify_processes(&subtree),
                status,
            }
        })
        .collect();
    // Orden estable por PID: sysinfo devuelve los procesos en orden
    // arbitrario entre snapshots, lo que hacia que las cards saltaran de
    // posicion en cada poll. El PID es estable durante la vida de la sesion.
    sessions.sort_by_key(|s| s.pid);
    sessions
}

/// `true` si el `cwd` de una sesion detectada cae dentro (o es igual a)
/// `workspace_path`. Se usa para agrupar las sesiones por workspace.
pub fn cwd_belongs_to_workspace(cwd: &std::path::Path, workspace_path: &std::path::Path) -> bool {
    cwd.starts_with(workspace_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn claude_status_parse_known_values() {
        assert_eq!(ClaudeStatus::parse("busy"), ClaudeStatus::Busy);
        assert_eq!(ClaudeStatus::parse("idle"), ClaudeStatus::Idle);
        assert_eq!(ClaudeStatus::parse("waiting"), ClaudeStatus::Waiting);
        assert_eq!(ClaudeStatus::parse("shell"), ClaudeStatus::Shell);
    }

    #[test]
    fn claude_status_parse_unknown_is_unknown() {
        assert_eq!(ClaudeStatus::parse("frobnicating"), ClaudeStatus::Unknown);
        assert_eq!(ClaudeStatus::parse(""), ClaudeStatus::Unknown);
    }

    #[test]
    fn parse_session_meta_real_shape() {
        // Forma real de ~/.claude/sessions/<pid>.json.
        let json = r#"{"pid":3696,"sessionId":"4ec52303-5a7e-4396","cwd":"C:\\Users\\kmilo\\Documents\\projects\\lelemon-workspace","startedAt":1779080494956,"version":"2.1.143","status":"busy","updatedAt":1779372738817}"#;
        let meta = parse_session_meta(json).unwrap();
        assert_eq!(meta.session_id, "4ec52303-5a7e-4396");
        assert_eq!(meta.status, ClaudeStatus::Busy);
        assert_eq!(meta.started_at_ms, 1779080494956);
        assert_eq!(
            meta.cwd,
            Some(PathBuf::from(
                "C:\\Users\\kmilo\\Documents\\projects\\lelemon-workspace"
            ))
        );
    }

    #[test]
    fn parse_session_meta_waiting_status() {
        let json = r#"{"sessionId":"abc","cwd":"/x","status":"waiting","startedAt":1}"#;
        let meta = parse_session_meta(json).unwrap();
        assert_eq!(meta.status, ClaudeStatus::Waiting);
    }

    #[test]
    fn parse_session_meta_invalid_json_is_none() {
        assert!(parse_session_meta("{not json").is_none());
    }

    #[test]
    fn parse_session_meta_missing_session_id_is_none() {
        // sessionId es obligatorio para identificar la sesion.
        assert!(parse_session_meta(r#"{"status":"idle"}"#).is_none());
    }

    #[test]
    fn parse_session_meta_missing_status_defaults_unknown() {
        let meta = parse_session_meta(r#"{"sessionId":"x","cwd":"/y"}"#).unwrap();
        assert_eq!(meta.status, ClaudeStatus::Unknown);
    }

    #[test]
    fn cli_from_local_bin_is_claude_cli() {
        assert!(is_claude_cli(
            "claude.exe",
            &s(&["C:\\Users\\kmilo\\.local\\bin\\claude.exe"])
        ));
    }

    #[test]
    fn cli_with_resume_flag_is_claude_cli() {
        assert!(is_claude_cli(
            "claude.exe",
            &s(&[
                "C:\\Users\\kmilo\\.local\\bin\\claude.exe",
                "--resume",
                "f570160f-3e76-4153-8481-edae0063f68e"
            ])
        ));
    }

    #[test]
    fn desktop_app_is_not_cli() {
        // App de escritorio Electron: path en WindowsApps.
        assert!(!is_claude_cli(
            "claude.exe",
            &s(&["C:\\Program Files\\WindowsApps\\Claude_1.7\\app\\Claude.exe"])
        ));
    }

    #[test]
    fn electron_subprocess_is_not_cli() {
        // Subproceso con --type=renderer.
        assert!(!is_claude_cli(
            "claude.exe",
            &s(&[
                "C:\\Program Files\\WindowsApps\\Claude_1.7\\app\\Claude.exe",
                "--type=renderer"
            ])
        ));
    }

    #[test]
    fn claude_meter_is_not_cli() {
        assert!(!is_claude_cli(
            "claude-meter.exe",
            &s(&["C:\\Users\\kmilo\\...\\claude-meter.exe", "serve"])
        ));
    }

    #[test]
    fn unrelated_process_is_not_cli() {
        assert!(!is_claude_cli("node.exe", &s(&["node.exe", "server.js"])));
        assert!(!is_claude_cli("code.exe", &s(&["code.exe"])));
    }

    #[test]
    fn cli_unix_name_without_exe() {
        assert!(is_claude_cli(
            "claude",
            &s(&["/home/kmilo/.local/bin/claude"])
        ));
    }

    #[test]
    fn extract_resume_id_finds_uuid() {
        let cmd = s(&["claude.exe", "--resume", "abc-123"]);
        assert_eq!(extract_resume_id(&cmd), Some("abc-123".to_string()));
    }

    #[test]
    fn extract_resume_id_none_when_absent() {
        let cmd = s(&["claude.exe"]);
        assert_eq!(extract_resume_id(&cmd), None);
    }

    #[test]
    fn extract_resume_id_none_when_flag_has_no_value() {
        let cmd = s(&["claude.exe", "--resume"]);
        assert_eq!(extract_resume_id(&cmd), None);
    }

    fn proc(
        pid: u32,
        parent: Option<u32>,
        name: &str,
        cmd: &[&str],
        cwd: Option<&str>,
    ) -> ProcInfo {
        ProcInfo {
            pid,
            parent_pid: parent,
            name: name.to_string(),
            memory_bytes: 100,
            cwd: cwd.map(PathBuf::from),
            cmd: s(cmd),
        }
    }

    #[test]
    fn detect_finds_only_cli_sessions() {
        let all = vec![
            proc(
                100,
                None,
                "claude.exe",
                &["C:\\Users\\k\\.local\\bin\\claude.exe"],
                Some("C:\\proj\\ws"),
            ),
            proc(
                200,
                None,
                "claude.exe",
                &[
                    "C:\\Program Files\\WindowsApps\\Claude_1\\app\\Claude.exe",
                    "--type=gpu",
                ],
                Some("C:\\WINDOWS"),
            ),
            proc(300, None, "node.exe", &["node.exe"], Some("C:\\x")),
        ];
        let found = detect_sessions(&all);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].pid, 100);
    }

    #[test]
    fn detect_aggregates_subtree_resources() {
        // claude (100) → hijo node (101). Recursos suman ambos.
        let all = vec![
            proc(
                100,
                None,
                "claude.exe",
                &["C:\\Users\\k\\.local\\bin\\claude.exe"],
                Some("C:\\ws"),
            ),
            proc(101, Some(100), "node.exe", &["node.exe", "dev"], None),
        ];
        let found = detect_sessions(&all);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].resources.process_count, 2, "claude + node hijo");
    }

    #[test]
    fn detect_sessions_returns_stable_order_by_pid() {
        // Aunque los procesos vengan desordenados, la salida es por PID
        // ascendente — asi las cards no saltan entre polls.
        let all = vec![
            proc(
                300,
                None,
                "claude.exe",
                &["C:\\x\\.local\\bin\\claude.exe"],
                Some("/a"),
            ),
            proc(
                100,
                None,
                "claude.exe",
                &["C:\\x\\.local\\bin\\claude.exe"],
                Some("/b"),
            ),
            proc(
                200,
                None,
                "claude.exe",
                &["C:\\x\\.local\\bin\\claude.exe"],
                Some("/c"),
            ),
        ];
        let found = detect_sessions(&all);
        let pids: Vec<u32> = found.iter().map(|s| s.pid).collect();
        assert_eq!(pids, vec![100, 200, 300]);
    }

    #[test]
    fn detect_extracts_cwd_and_resume() {
        let all = vec![proc(
            100,
            None,
            "claude.exe",
            &["C:\\Users\\k\\.local\\bin\\claude.exe", "--resume", "xyz"],
            Some("C:\\proj\\venpu-workspace"),
        )];
        let found = detect_sessions(&all);
        assert_eq!(
            found[0].cwd,
            Some(PathBuf::from("C:\\proj\\venpu-workspace"))
        );
        assert_eq!(found[0].resume_id, Some("xyz".to_string()));
    }

    #[test]
    fn cwd_belongs_to_workspace_matches_exact_and_nested() {
        let ws = std::path::Path::new("/proj/ws");
        assert!(cwd_belongs_to_workspace(
            std::path::Path::new("/proj/ws"),
            ws
        ));
        assert!(cwd_belongs_to_workspace(
            std::path::Path::new("/proj/ws/repo-a"),
            ws
        ));
        assert!(!cwd_belongs_to_workspace(
            std::path::Path::new("/proj/other"),
            ws
        ));
    }
}
