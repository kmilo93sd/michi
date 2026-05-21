//! Monitor de recursos por sesion (agente Claude Code).
//!
//! Cada job tiene un PID raiz (el proceso que michi spawneo via egui_term,
//! accesible con `TerminalBackend::pty_id()` — que pese al nombre es el OS
//! PID del child). A partir de ese PID construimos el arbol de procesos
//! descendientes y agregamos sus recursos: cuantos procesos hay y cuanta RAM
//! consumen en total.
//!
//! Esto resuelve el dolor "no se cual Claude Code esta chupando RAM": cada
//! card muestra el consumo agregado de SU arbol de procesos.
//!
//! La logica de construccion del arbol (`collect_subtree`) y de agregacion
//! (`aggregate`) son funciones puras testeables. La lectura del estado del
//! OS (`snapshot_all_processes`) es glue fina sobre `sysinfo`.

use std::collections::HashMap;
use std::path::PathBuf;

use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};

/// Info minima de un proceso para construir el arbol y agregar recursos.
/// Incluye `cwd` y `cmd` porque el detector de sesiones Claude externas
/// (`claude_sessions`) los necesita para identificar el CLI real y agrupar
/// por workspace. Para el arbol de recursos solo se usan pid/parent/memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub memory_bytes: u64,
    pub cwd: Option<PathBuf>,
    pub cmd: Vec<String>,
}

/// Recursos agregados del arbol de procesos de una sesion.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionResources {
    /// Cantidad de procesos en el arbol (incluye el raiz).
    pub process_count: usize,
    /// RAM total sumada de todos los procesos del arbol, en bytes.
    pub total_memory_bytes: u64,
}

impl SessionResources {
    /// RAM en formato humano: "240 MB", "1.4 GB". Para mostrar en la card.
    pub fn memory_human(&self) -> String {
        humanize_bytes(self.total_memory_bytes)
    }
}

/// Dado un snapshot de TODOS los procesos del sistema y un `root_pid`,
/// devuelve el subarbol: el root + todos sus descendientes (hijos, nietos,
/// etc). Funcion pura — no toca el OS.
///
/// Si `root_pid` no esta en `all`, devuelve vector vacio.
pub fn collect_subtree(all: &[ProcInfo], root_pid: u32) -> Vec<ProcInfo> {
    // Index hijos por parent para BFS eficiente.
    let mut children_of: HashMap<u32, Vec<&ProcInfo>> = HashMap::new();
    for p in all {
        if let Some(parent) = p.parent_pid {
            children_of.entry(parent).or_default().push(p);
        }
    }
    let Some(root) = all.iter().find(|p| p.pid == root_pid) else {
        return Vec::new();
    };

    let mut result = vec![root.clone()];
    let mut queue = vec![root_pid];
    let mut visited = std::collections::HashSet::new();
    visited.insert(root_pid);
    while let Some(pid) = queue.pop() {
        if let Some(kids) = children_of.get(&pid) {
            for kid in kids {
                if visited.insert(kid.pid) {
                    result.push((*kid).clone());
                    queue.push(kid.pid);
                }
            }
        }
    }
    result
}

/// Agrega recursos de un subarbol (count + RAM total). Funcion pura.
pub fn aggregate(subtree: &[ProcInfo]) -> SessionResources {
    SessionResources {
        process_count: subtree.len(),
        total_memory_bytes: subtree.iter().map(|p| p.memory_bytes).sum(),
    }
}

/// Desglose de los procesos "notables" del arbol de una sesion: cuantos
/// shells abrio, que runtimes/servers levanto, y si esta usando docker.
/// Es lo que michi muestra como chips en la card ("1 shell · node · docker").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessBreakdown {
    /// Cantidad de procesos shell (bash, cmd, powershell, etc).
    pub shells: usize,
    /// Runtimes/servers unicos detectados, ordenados ("node", "python").
    pub runtimes: Vec<String>,
    /// Hay algun proceso docker en el arbol.
    pub has_docker: bool,
}

/// Clasifica el arbol de procesos en shells / runtimes / docker. Funcion
/// pura. Ignora ruido (claude, conhost, el shell wrapper de michi, etc) —
/// solo cuenta lo que le importa al usuario para entender que levanto la
/// sesion.
pub fn classify_processes(subtree: &[ProcInfo]) -> ProcessBreakdown {
    use std::collections::BTreeSet;
    let mut shells = 0usize;
    let mut runtimes: BTreeSet<String> = BTreeSet::new();
    let mut has_docker = false;
    for p in subtree {
        let base = normalize_proc_name(&p.name);
        if is_shell(&base) {
            shells += 1;
        } else if base.contains("docker") {
            has_docker = true;
        } else if let Some(rt) = runtime_label(&base) {
            runtimes.insert(rt.to_string());
        }
    }
    ProcessBreakdown {
        shells,
        runtimes: runtimes.into_iter().collect(),
        has_docker,
    }
}

/// Normaliza el nombre del proceso: minusculas y sin extension `.exe`.
fn normalize_proc_name(name: &str) -> String {
    let lower = name.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

fn is_shell(base: &str) -> bool {
    matches!(
        base,
        "bash" | "sh" | "zsh" | "fish" | "cmd" | "powershell" | "pwsh"
    )
}

/// Devuelve la etiqueta de runtime/server si el proceso es uno conocido.
/// `None` para procesos que no son runtimes notables.
fn runtime_label(base: &str) -> Option<&'static str> {
    match base {
        "node" => Some("node"),
        "deno" => Some("deno"),
        "bun" => Some("bun"),
        "python" | "python3" | "py" => Some("python"),
        "ruby" => Some("ruby"),
        "php" => Some("php"),
        "java" => Some("java"),
        "go" => Some("go"),
        "dotnet" => Some("dotnet"),
        "postgres" | "postgresql" => Some("postgres"),
        "redis-server" | "redis" => Some("redis"),
        _ => None,
    }
}

impl ProcessBreakdown {
    /// `true` si no hay nada notable que mostrar.
    pub fn is_empty(&self) -> bool {
        self.shells == 0 && self.runtimes.is_empty() && !self.has_docker
    }
}

/// Lee TODOS los procesos del sistema via sysinfo. Glue fina sobre el OS.
/// Refresca solo lo necesario (procesos + memoria) para ser barato.
pub fn snapshot_all_processes() -> Vec<ProcInfo> {
    let sys = System::new_with_specifics(
        RefreshKind::nothing().with_processes(
            ProcessRefreshKind::nothing()
                .with_memory()
                .with_cwd(UpdateKind::Always)
                .with_cmd(UpdateKind::Always),
        ),
    );
    sys.processes()
        .iter()
        .map(|(pid, proc)| ProcInfo {
            pid: pid.as_u32(),
            parent_pid: proc.parent().map(|p| p.as_u32()),
            name: proc.name().to_string_lossy().to_string(),
            memory_bytes: proc.memory(),
            cwd: proc.cwd().map(|p| p.to_path_buf()),
            cmd: proc
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_string())
                .collect(),
        })
        .collect()
}

/// Conveniencia: snapshot del OS + subarbol + agregacion para un `root_pid`.
pub fn resources_for(root_pid: u32) -> SessionResources {
    let all = snapshot_all_processes();
    let subtree = collect_subtree(&all, root_pid);
    aggregate(&subtree)
}

fn humanize_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, parent: Option<u32>, mem: u64) -> ProcInfo {
        ProcInfo {
            pid,
            parent_pid: parent,
            name: format!("p{pid}"),
            memory_bytes: mem,
            cwd: None,
            cmd: Vec::new(),
        }
    }

    #[test]
    fn subtree_of_single_root_with_no_children() {
        let all = vec![proc(100, None, 50)];
        let sub = collect_subtree(&all, 100);
        assert_eq!(sub.len(), 1);
        assert_eq!(sub[0].pid, 100);
    }

    #[test]
    fn subtree_includes_direct_children() {
        let all = vec![
            proc(100, None, 10),
            proc(101, Some(100), 20),
            proc(102, Some(100), 30),
        ];
        let sub = collect_subtree(&all, 100);
        let pids: std::collections::HashSet<u32> = sub.iter().map(|p| p.pid).collect();
        assert_eq!(pids, [100, 101, 102].into_iter().collect());
    }

    #[test]
    fn subtree_includes_grandchildren() {
        // 100 -> 101 -> 102 -> 103 (cadena)
        let all = vec![
            proc(100, None, 1),
            proc(101, Some(100), 1),
            proc(102, Some(101), 1),
            proc(103, Some(102), 1),
        ];
        let sub = collect_subtree(&all, 100);
        assert_eq!(sub.len(), 4);
    }

    #[test]
    fn subtree_excludes_unrelated_processes() {
        let all = vec![
            proc(100, None, 1),
            proc(101, Some(100), 1),
            proc(200, None, 1),      // arbol distinto
            proc(201, Some(200), 1), // hijo del otro arbol
        ];
        let sub = collect_subtree(&all, 100);
        let pids: std::collections::HashSet<u32> = sub.iter().map(|p| p.pid).collect();
        assert_eq!(pids, [100, 101].into_iter().collect());
        assert!(!pids.contains(&200));
    }

    #[test]
    fn subtree_of_missing_root_is_empty() {
        let all = vec![proc(100, None, 1)];
        assert!(collect_subtree(&all, 999).is_empty());
    }

    #[test]
    fn subtree_handles_cycle_without_infinite_loop() {
        // Defensa: si el OS reporta un ciclo (no deberia), no colgar.
        let all = vec![proc(100, Some(101), 1), proc(101, Some(100), 1)];
        let sub = collect_subtree(&all, 100);
        // Debe terminar y no duplicar.
        assert!(sub.len() <= 2);
    }

    #[test]
    fn aggregate_sums_memory_and_counts() {
        let sub = vec![
            proc(100, None, 100),
            proc(101, Some(100), 200),
            proc(102, Some(100), 300),
        ];
        let res = aggregate(&sub);
        assert_eq!(res.process_count, 3);
        assert_eq!(res.total_memory_bytes, 600);
    }

    #[test]
    fn aggregate_empty_is_zero() {
        let res = aggregate(&[]);
        assert_eq!(res.process_count, 0);
        assert_eq!(res.total_memory_bytes, 0);
    }

    #[test]
    fn humanize_bytes_scales() {
        assert_eq!(humanize_bytes(512), "512 B");
        assert_eq!(humanize_bytes(2048), "2 KB");
        assert_eq!(humanize_bytes(5 * 1024 * 1024), "5 MB");
        assert_eq!(humanize_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn memory_human_via_session_resources() {
        let r = SessionResources {
            process_count: 2,
            total_memory_bytes: 240 * 1024 * 1024,
        };
        assert_eq!(r.memory_human(), "240 MB");
    }

    fn named(pid: u32, name: &str) -> ProcInfo {
        ProcInfo {
            pid,
            parent_pid: None,
            name: name.to_string(),
            memory_bytes: 10,
            cwd: None,
            cmd: Vec::new(),
        }
    }

    #[test]
    fn classify_counts_shells() {
        let tree = vec![
            named(1, "claude.exe"),
            named(2, "bash"),
            named(3, "cmd.exe"),
            named(4, "powershell.exe"),
        ];
        let b = classify_processes(&tree);
        assert_eq!(b.shells, 3, "bash + cmd + powershell");
    }

    #[test]
    fn classify_detects_runtimes_unique_sorted() {
        let tree = vec![
            named(1, "node.exe"),
            named(2, "node.exe"),
            named(3, "python.exe"),
        ];
        let b = classify_processes(&tree);
        assert_eq!(b.runtimes, vec!["node".to_string(), "python".to_string()]);
    }

    #[test]
    fn classify_detects_docker() {
        let tree = vec![named(1, "claude.exe"), named(2, "com.docker.backend.exe")];
        let b = classify_processes(&tree);
        assert!(b.has_docker);
    }

    #[test]
    fn classify_ignores_noise() {
        // claude, conhost, procesos random no cuentan.
        let tree = vec![
            named(1, "claude.exe"),
            named(2, "conhost.exe"),
            named(3, "explorer.exe"),
        ];
        let b = classify_processes(&tree);
        assert!(b.is_empty(), "nada notable, fue: {b:?}");
    }

    #[test]
    fn classify_postgres_and_redis_as_runtimes() {
        let tree = vec![named(1, "postgres"), named(2, "redis-server")];
        let b = classify_processes(&tree);
        assert!(b.runtimes.contains(&"postgres".to_string()));
        assert!(b.runtimes.contains(&"redis".to_string()));
    }

    #[test]
    fn classify_full_mix() {
        let tree = vec![
            named(1, "claude.exe"),
            named(2, "bash"),
            named(3, "node.exe"),
            named(4, "docker.exe"),
        ];
        let b = classify_processes(&tree);
        assert_eq!(b.shells, 1);
        assert_eq!(b.runtimes, vec!["node".to_string()]);
        assert!(b.has_docker);
        assert!(!b.is_empty());
    }

    #[test]
    fn snapshot_returns_some_processes() {
        // Glue test: el sistema siempre tiene al menos el proceso de test.
        let all = snapshot_all_processes();
        assert!(!all.is_empty(), "el OS siempre tiene procesos corriendo");
    }

    #[test]
    fn resources_for_current_process_is_nonzero() {
        // El PID del propio test runner debe tener RAM > 0.
        let my_pid = std::process::id();
        let res = resources_for(my_pid);
        assert!(res.process_count >= 1);
        assert!(res.total_memory_bytes > 0);
    }
}
