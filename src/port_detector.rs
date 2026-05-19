//! Detector de "slots de puerto" en un workspace.
//!
//! Escanea archivos `.env*` del workspace + sus repos hijos para identificar
//! que variables de entorno representan puertos (`PORT_API`, `API_PORT`,
//! `POSTGRES_PORT`, etc). El resultado es una lista de slots semanticos que
//! michi usa despues para:
//! - Asignar valores unicos por sesion (rango por sesion).
//! - Inyectarlos como env vars al spawn del PTY.
//! - Mostrarlos en la card del job.
//!
//! El primer pase solo lee `.env`, `.env.local`, `.env.example` a nivel
//! workspace y a nivel cada repo hijo. Soporte para docker-compose y
//! package.json scripts se agregara en PRs siguientes si vale la pena.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Un slot de puerto detectado en la config del workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortSlot {
    /// Nombre semantico ("API", "WEB", "POSTGRES"). Se deriva del env var.
    pub name: String,
    /// Variable de entorno tal como aparece en .env (`PORT_API`).
    pub env_var: String,
    /// Valor por defecto detectado en algun archivo (puede haber varios
    /// archivos con el mismo var; nos quedamos con el primero encontrado).
    pub default_value: u16,
    /// Archivos donde aparece este slot (para tooltip / debug).
    pub source_files: Vec<PathBuf>,
}

/// Escanea el workspace + repos hijos. Devuelve los slots ordenados por
/// nombre (estable entre llamadas).
pub fn detect_ports(workspace_path: &Path, repo_paths: &[PathBuf]) -> Vec<PortSlot> {
    // Por env_var → (default, source_files). Usamos BTreeMap para orden estable.
    let mut by_var: BTreeMap<String, (u16, Vec<PathBuf>)> = BTreeMap::new();

    // 1. Archivos del workspace.
    for f in env_files_in(workspace_path) {
        ingest_env_file(&f, &mut by_var);
    }
    // 2. Archivos de cada repo hijo.
    for repo in repo_paths {
        for f in env_files_in(repo) {
            ingest_env_file(&f, &mut by_var);
        }
    }

    by_var
        .into_iter()
        .map(|(env_var, (default_value, source_files))| PortSlot {
            name: slot_name(&env_var),
            env_var,
            default_value,
            source_files,
        })
        .collect()
}

/// Quita prefijo `PORT_` o sufijo `_PORT` para extraer la parte semantica.
/// Si la var es solo `PORT` devuelve `"default"`. Si no matchea ningun
/// patron port-ish, devuelve la var en uppercase (fallback debugable).
pub fn slot_name(env_var: &str) -> String {
    let upper = env_var.to_uppercase();
    if let Some(rest) = upper.strip_prefix("PORT_") {
        rest.to_string()
    } else if let Some(rest) = upper.strip_suffix("_PORT") {
        rest.to_string()
    } else if upper == "PORT" {
        "default".to_string()
    } else {
        upper
    }
}

/// Heuristica: el nombre de la env var "parece" un puerto?
/// - Contiene `PORT` como token (separado por `_` o al inicio/final).
/// - El valor parsea a u16 > 0.
pub fn looks_like_port_var(name: &str) -> bool {
    let upper = name.to_uppercase();
    upper == "PORT"
        || upper.starts_with("PORT_")
        || upper.ends_with("_PORT")
        || upper.contains("_PORT_")
}

fn env_files_in(dir: &Path) -> Vec<PathBuf> {
    let candidates = [".env", ".env.local", ".env.example", ".env.development"];
    candidates
        .iter()
        .map(|n| dir.join(n))
        .filter(|p| p.is_file())
        .collect()
}

fn ingest_env_file(path: &Path, by_var: &mut BTreeMap<String, (u16, Vec<PathBuf>)>) {
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = parse_env_line(trimmed) else {
            continue;
        };
        if !looks_like_port_var(&key) {
            continue;
        }
        let Ok(port) = value.parse::<u16>() else {
            continue;
        };
        if port == 0 {
            continue;
        }
        let entry = by_var.entry(key).or_insert_with(|| (port, Vec::new()));
        // El primer file gana en cuanto al valor default. Acumulamos todos
        // los paths donde aparece.
        if !entry.1.contains(&path.to_path_buf()) {
            entry.1.push(path.to_path_buf());
        }
    }
}

/// Parsea `KEY=VALUE` con tolerancia: ignora `export `, quita comillas,
/// quita comentarios inline `KEY=VALUE  # comentario`.
fn parse_env_line(line: &str) -> Option<(String, String)> {
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, rest) = line.split_once('=')?;
    let key = key.trim().to_string();
    if key.is_empty() {
        return None;
    }
    let value_with_comment = rest.trim();
    // Si arranca con comilla, recortamos hasta la siguiente comilla.
    let value = if let Some(stripped) = value_with_comment.strip_prefix('"') {
        stripped.split('"').next().unwrap_or("").to_string()
    } else if let Some(stripped) = value_with_comment.strip_prefix('\'') {
        stripped.split('\'').next().unwrap_or("").to_string()
    } else {
        // Sin comillas: corta en espacio o `#` (comment inline).
        value_with_comment
            .split_whitespace()
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("")
            .to_string()
    };
    Some((key, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_env(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn slot_name_extracts_semantic_part() {
        assert_eq!(slot_name("PORT_API"), "API");
        assert_eq!(slot_name("PORT_WEB"), "WEB");
        assert_eq!(slot_name("API_PORT"), "API");
        assert_eq!(slot_name("DATABASE_PORT"), "DATABASE");
        assert_eq!(slot_name("PORT"), "default");
    }

    #[test]
    fn slot_name_case_insensitive() {
        assert_eq!(slot_name("port_api"), "API");
        assert_eq!(slot_name("Port_Web"), "WEB");
    }

    #[test]
    fn looks_like_port_var_true_cases() {
        assert!(looks_like_port_var("PORT"));
        assert!(looks_like_port_var("PORT_API"));
        assert!(looks_like_port_var("API_PORT"));
        assert!(looks_like_port_var("port_web"));
    }

    #[test]
    fn looks_like_port_var_false_cases() {
        assert!(!looks_like_port_var("DATABASE_URL"));
        assert!(!looks_like_port_var("PORTABLE"));
        assert!(!looks_like_port_var("PASSPORT"));
        assert!(!looks_like_port_var("NODE_ENV"));
    }

    #[test]
    fn parse_env_line_simple_kv() {
        let (k, v) = parse_env_line("PORT_API=4100").unwrap();
        assert_eq!(k, "PORT_API");
        assert_eq!(v, "4100");
    }

    #[test]
    fn parse_env_line_strips_export() {
        let (k, v) = parse_env_line("export PORT_WEB=3500").unwrap();
        assert_eq!(k, "PORT_WEB");
        assert_eq!(v, "3500");
    }

    #[test]
    fn parse_env_line_strips_quotes() {
        let (_, v1) = parse_env_line(r#"PORT_API="4100""#).unwrap();
        assert_eq!(v1, "4100");
        let (_, v2) = parse_env_line("PORT_API='4100'").unwrap();
        assert_eq!(v2, "4100");
    }

    #[test]
    fn parse_env_line_strips_inline_comment() {
        let (_, v) = parse_env_line("PORT_API=4100  # default").unwrap();
        assert_eq!(v, "4100");
    }

    #[test]
    fn empty_workspace_returns_no_ports() {
        let tmp = TempDir::new().unwrap();
        let ports = detect_ports(tmp.path(), &[]);
        assert!(ports.is_empty());
    }

    #[test]
    fn detects_simple_env_file_at_workspace() {
        let tmp = TempDir::new().unwrap();
        write_env(
            tmp.path(),
            ".env",
            "PORT_API=4100\nPORT_WEB=3500\nNODE_ENV=development\n",
        );
        let ports = detect_ports(tmp.path(), &[]);
        assert_eq!(ports.len(), 2);
        let names: Vec<&str> = ports.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"API"));
        assert!(names.contains(&"WEB"));
    }

    #[test]
    fn ignores_non_port_vars_even_if_numeric() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), ".env", "TIMEOUT=3000\nMAX_RETRIES=5\n");
        let ports = detect_ports(tmp.path(), &[]);
        assert!(ports.is_empty());
    }

    #[test]
    fn ignores_port_var_with_non_numeric_value() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), ".env", "PORT_API=auto\nPORT_WEB=3500\n");
        let ports = detect_ports(tmp.path(), &[]);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].name, "WEB");
    }

    #[test]
    fn ignores_port_zero_and_out_of_range() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), ".env", "PORT_A=0\nPORT_B=99999\nPORT_C=4100\n");
        let ports = detect_ports(tmp.path(), &[]);
        let names: Vec<&str> = ports.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["C"]);
    }

    #[test]
    fn reads_env_example_if_no_env_present() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), ".env.example", "PORT_API=4100\n");
        let ports = detect_ports(tmp.path(), &[]);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].name, "API");
    }

    #[test]
    fn merges_ports_from_workspace_and_repos() {
        let tmp = TempDir::new().unwrap();
        let repo_a = tmp.path().join("repo-a");
        let repo_b = tmp.path().join("repo-b");
        fs::create_dir_all(&repo_a).unwrap();
        fs::create_dir_all(&repo_b).unwrap();

        write_env(tmp.path(), ".env", "POSTGRES_PORT=5432\n");
        write_env(&repo_a, ".env", "PORT_API=4100\n");
        write_env(&repo_b, ".env", "PORT_WEB=3500\n");

        let ports = detect_ports(tmp.path(), &[repo_a, repo_b]);
        let names: Vec<&str> = ports.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"POSTGRES"));
        assert!(names.contains(&"API"));
        assert!(names.contains(&"WEB"));
        assert_eq!(ports.len(), 3);
    }

    #[test]
    fn dedupes_same_var_appearing_in_multiple_files() {
        let tmp = TempDir::new().unwrap();
        write_env(tmp.path(), ".env", "PORT_API=4100\n");
        write_env(tmp.path(), ".env.example", "PORT_API=4100\n");
        let ports = detect_ports(tmp.path(), &[]);
        assert_eq!(ports.len(), 1);
        let api = &ports[0];
        assert_eq!(api.name, "API");
        assert_eq!(
            api.source_files.len(),
            2,
            "ambos archivos quedan en source_files"
        );
    }

    #[test]
    fn ignores_comment_lines() {
        let tmp = TempDir::new().unwrap();
        write_env(
            tmp.path(),
            ".env",
            "# PORT_FAKE=1\n\nPORT_REAL=4100\n# more comments\n",
        );
        let ports = detect_ports(tmp.path(), &[]);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].name, "REAL");
    }

    #[test]
    fn first_file_wins_for_default_value() {
        let tmp = TempDir::new().unwrap();
        // El loader procesa .env primero (lista candidates ordenada). Si
        // tambien hay .env.example con un valor distinto, el .env gana.
        write_env(tmp.path(), ".env", "PORT_API=4100\n");
        write_env(tmp.path(), ".env.example", "PORT_API=9999\n");
        let ports = detect_ports(tmp.path(), &[]);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].default_value, 4100);
    }
}
