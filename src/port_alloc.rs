//! Asignacion automatica de rangos de puertos por sesion.
//!
//! Cada job (sesion Claude) recibe un `port_range_start` (ej. 4100) y usa
//! `port_range_start + N` para cada slot detectado (`PORT_API=4100`,
//! `PORT_WEB=4101`, etc).
//!
//! Asi dos sesiones paralelas en el mismo workspace no chocan: sesion A
//! usa 4100-4199, sesion B usa 4200-4299. El step entre sesiones es 100
//! por default (ver `RANGE_STEP`), suficiente para ~100 slots distintos
//! por sesion.

use crate::state::Job;

/// Primer puerto que michi asigna a la primera sesion. Elegido por encima
/// del rango "well-known" (0-1023) y del rango efimero comun de Windows
/// (~49152+). 4100 deja espacio para defaults clasicos (3000, 3500, 5432).
pub const RANGE_START: u16 = 4100;

/// Cuanto separar el inicio del rango entre sesiones. 100 deja margen para
/// que cada sesion use hasta 99 puertos sin overflow al rango de la siguiente.
pub const RANGE_STEP: u16 = 100;

/// Asigna el siguiente `port_range_start` libre para una sesion nueva,
/// dado el conjunto de jobs activos. La estrategia es buscar el primer
/// "slot" libre en la secuencia `RANGE_START + k*RANGE_STEP` que no este
/// ocupado por ningun job (excluyendo jobs con `port_range_start == 0`,
/// que es el marker "no asignado" para jobs legacy).
pub fn assign_next_range(jobs: &[Job]) -> u16 {
    let occupied: std::collections::HashSet<u16> = jobs
        .iter()
        .map(|j| j.port_range_start)
        .filter(|p| *p != 0)
        .collect();
    let mut candidate = RANGE_START;
    while occupied.contains(&candidate) {
        // Si `candidate + STEP` overflowa u16, devolvemos el ultimo libre
        // que encontramos. En la practica nunca se llega: 65535/100 = 655
        // sesiones simultaneas.
        let Some(next) = candidate.checked_add(RANGE_STEP) else {
            return candidate;
        };
        candidate = next;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Job, JobStatus};
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn job_with_range(start: u16) -> Job {
        Job {
            id: format!("job-{start}"),
            workspace: "ws".into(),
            repo: "(workspace)".into(),
            branch: "(workspace)".into(),
            worktree_path: PathBuf::from("/tmp/ws"),
            status: JobStatus::Idle,
            files_changed: 0,
            last_activity: SystemTime::now(),
            port_range_start: start,
        }
    }

    #[test]
    fn empty_returns_first_range() {
        assert_eq!(assign_next_range(&[]), 4100);
    }

    #[test]
    fn skips_occupied_range_at_start() {
        let jobs = vec![job_with_range(4100)];
        assert_eq!(assign_next_range(&jobs), 4200);
    }

    #[test]
    fn fills_gap_when_middle_range_is_free() {
        // 4100 y 4300 ocupados, 4200 libre → devuelve 4200.
        let jobs = vec![job_with_range(4100), job_with_range(4300)];
        assert_eq!(assign_next_range(&jobs), 4200);
    }

    #[test]
    fn skips_multiple_consecutive_occupied_ranges() {
        let jobs = vec![
            job_with_range(4100),
            job_with_range(4200),
            job_with_range(4300),
        ];
        assert_eq!(assign_next_range(&jobs), 4400);
    }

    #[test]
    fn ignores_jobs_with_zero_range_unassigned() {
        // Jobs legacy con port_range_start=0 NO ocupan el slot 0 ni nada
        // (porque 0 es el marker "sin asignar"). El primer libre sigue
        // siendo 4100.
        let jobs = vec![job_with_range(0), job_with_range(0)];
        assert_eq!(assign_next_range(&jobs), 4100);
    }

    #[test]
    fn does_not_assume_jobs_are_sorted() {
        // El orden de jobs no importa: 4300 antes que 4100 → devuelve 4200.
        let jobs = vec![job_with_range(4300), job_with_range(4100)];
        assert_eq!(assign_next_range(&jobs), 4200);
    }

    #[test]
    fn assigning_after_each_alloc_yields_distinct_ranges() {
        // Simulacion: 3 sesiones consecutivas. Cada nueva no debe colisionar.
        let mut jobs: Vec<Job> = Vec::new();
        let mut allocated = Vec::new();
        for _ in 0..3 {
            let range = assign_next_range(&jobs);
            allocated.push(range);
            jobs.push(job_with_range(range));
        }
        assert_eq!(allocated, vec![4100, 4200, 4300]);
    }
}
