# Fases - michi

> Estado: **POC base + tanda harness COMPLETADOS.** Roadmap V1 en curso.
> Última actualización: 2026-05-21

> Cada fase termina con algo demoable y testeable (TDD: red → green → refactor).
> Si una fase se demora >2x lo estimado, parar y reevaluar — no acumular fases
> incompletas.

---

## Estado real (2026-05-21)

El plan original de 6 fases (abajo, **completado**) sirvió para parir el POC. Tras
el pivote 2026-05-20 (a harness de recursos) y la decisión 2026-05-21
(dirección container-first), el trabajo activo es el **Roadmap V1**. Ver
`SPEC.md` sección "Estado actual (2026-05-21)".

### POC base (Fases 1-6 originales) — ✅ COMPLETADO en lo esencial

Bootstrap, layout estático con tree de 3 niveles, git worktree real +
persistencia (`~/.michi/state.json`), terminal embebido (`egui_term` +
`portable-pty`), acciones git desde la UI y polish. El detalle por tarea vive en
el historial git. ~173 tests TDD, `clippy -D warnings` + `fmt` limpios.

### Tanda harness (PRs #27-#33) — ✅ COMPLETADO

- `resource_monitor` — árbol de procesos por sesión (sysinfo), RAM agregada,
  `classify_processes` (shells/runtimes/docker), `subtree_details`.
- `claude_sessions` — detecta TODOS los `claude.exe` del host (filtra desktop
  app/Electron), agrupa por workspace (cwd), lee estado real de
  `~/.claude/sessions/<pid>.json` (busy/idle/waiting/shell) y título legible
  (primer mensaje del `.jsonl`).
- `port_detector` + `port_alloc` — detecta `PORT_*` en `.env`, asigna rango por
  sesión, inyecta env vars al PTY vía shell wrapper.
- UI — cards de sesión con dot de estado real + título + chips + puertos; click
  en detectada → panel de detalle de procesos. Orden estable por pid.

---

## Roadmap V1 — harness, dirección container-first

> Modelo de 3 planos (ver SPEC.md): piso de observación nativo universal +
> aislamiento contenedor-preferido (fallback nativo) + infra compartida.
> Las fases A-C entregan valor sin Docker; la D introduce el sandbox; la E cierra.

> **Foco actual (2026-05-21): el track container (Fase D).** Decisión de Camilo:
> no hacer todo a la vez, explorar el contenedor. Se arranca con un **spike
> manual (D.0)** antes de escribir código. A/B/C quedan como soporte alrededor.

### Fase A — Cerrar el piso de observación (Nivel 0 universal)

> **Estado:** `EN CURSO`
> **Objetivo:** michi ve la realidad completa del host, sin importar el runtime.

- [ ] A.1 - Context menu en cards de sesión **detectada**:
  - **Renombrar** — alias local override del título, persistido por `sessionId`.
  - **Cerrar sesión** — mata el proceso claude + árbol (destructivo, confirmación).
  - **Traer a michi** — relanza con `claude --resume <sessionId>` en PTY embebido
    (managed). Confirmación avisa qué se pierde (procesos hijos vivos, cache).
- [ ] A.2 - Puertos LISTEN reales vía `netstat2`; cruzar puerto ↔ PID ↔ sesión.
- [ ] A.3 - Resource tree + estado real también en sesiones **managed** (hoy el
  estado real solo está en detectadas).

**Criterio:** para cualquier sesión (managed o detectada) michi muestra procesos,
RAM y puertos realmente escuchando.

### Fase B — Inyección y arbitraje

> **Estado:** `PENDIENTE`
> **Objetivo:** michi pasa de observar a intervenir (lo determinista) y delegar a
> Claude (lo no determinista).

- [ ] B.1 - Primitiva: enviar prompt a sesión managed (`process_command(Write)` al
  PTY + Enter).
- [ ] B.2 - Inyectar la "tarea inicial" del modal como primer prompt + display_name
  automático.
- [ ] B.3 - Detección de conflictos (2 sesiones en el mismo puerto) → aviso +
  botón "arreglar con Claude" que inyecta un prompt de corrección.

**Criterio:** al detectar choque de puertos, michi avisa y puede pedirle a Claude
que lo resuelva con un click.

### Fase C — DB isolation (el diferenciador, Nivel 2-3)

> **Estado:** `PENDIENTE`
> **Objetivo:** varias sesiones comparten un postgres sin pisarse los datos.

- [ ] C.1 - Detectar uso de `DATABASE_URL` en el proyecto (`.env` / compose).
- [ ] C.2 - Postgres compartido: detectar contenedor existente o levantar uno.
- [ ] C.3 - Crear DB/schema efímero por sesión; inyectar `DATABASE_URL` por sesión.
  Migraciones las corre Claude/el dev server, NUNCA michi (solo provee el tubo).
- [ ] C.4 - Drop de la DB/schema al cerrar la sesión.
- [ ] C.5 - Redis: contenedor efímero **por sesión** (no multi-tenant — DB 0-15
  globales, `FLUSHALL` peligroso; pesa ~10MiB).
- [ ] C.6 - GC de huérfanos al arranque: tokio task que dropea DBs `session_*` y
  contenedores `michi-*` sin sesión activa (anti-crash). Funciona en nativo y
  contenedor (DB isolation es ortogonal al sandbox → puede ir primero en nativo).

**Criterio:** dos sesiones del mismo repo corren tests contra la misma instancia
postgres sin colisión (cada una en su DB); al cerrar, se limpia.

### Fase D — Sandbox container-first (la dirección nueva) · **FOCO**

> **Estado:** `Cut-1 COMPLETO (#42-#50)` — el contenedor anda de punta a punta
> (lanzar / traer / detener / reabrir / cerrar). Quedan refinamientos
> (split-anatomy, PTY resize, persistir session_id) y los cuts 2-3.
> **Objetivo:** las sesiones que michi lanza viven en un contenedor aislado;
> fallback nativo si no hay Docker.

**Hecho en #42-#50 (todos en main):** wiring `plan_launch`→`JobTerminal::spawn`
(ON por defecto); imagen slim por lenguaje; **bind-mount del binario claude**
(arch-matched, extraído con docker-cp a `~/.michi/bin/`); montaje de `~/.claude` +
`~/.claude.json` RW (fix del login interactivo); `docker rm -f` antes de run (fix
conflicto de nombre) + teardown al cerrar + **GC de huérfanos al arrancar**;
**Traer a michi** (externa → managed via resume); **Detener/Reabrir** (resume,
`ManagedSession`); **pantalla de fin de sesión** (resumen + Retomar/Reiniciar/
Cerrar) como card; **badge** contenedor/nativo en el header; menú contextual de
sesiones + relabel "trabajos"→"sesiones".

- [x] D.0 - **Spike manual** — ✅ COMPLETADO (2026-05-21). Hallazgos completos en
  SESSION.md "Hallazgos del spike D.0". Validado:
  - ✅ **Nivel 1:** `claude` autentica dentro de un contenedor Linux montando solo
    `.credentials.json`; edita archivos montados con round-trip al host.
  - ✅ Perf: bind mount Win **~12x más lento** que named volume → build artifacts
    van en named volume / WSL2, no en bind mount de path Windows.
  - ✅ **Nivel 3:** postgres compartido + DB efímera por sesión vía `DATABASE_URL`,
    aislamiento real, 1 postgres = 42 MiB para N sesiones.
  - ✅ **Nivel 4:** `--memory`/`--cpus` se respetan (OOM exit 137) y `docker stats`
    los muestra.
  - ✅ Imagen `michi-spike` (node + claude + git + psql) = embrión de `michi-base`.
- [x] D.1 - Detección de Docker: módulo `docker.rs` (`detect_docker() -> DockerStatus`,
  con `Unavailable { BinaryNotFound | DaemonNotResponding }` como fallback). 5 tests
  TDD. El cableado de la degradación a nativo al *lanzar* va con D.2.
- [x] D.2 - `build_run_args()` (#35): arma el comando `docker run` (mounts, creds
  read-only, puertos publicados, env, caps de memoria/cpu). Función pura testeable.
- [x] D.2a - `plan_launch()` (#36): planner contenedor-vs-nativo (degradación de la
  regla 4); produce command/args/env/cwd que mapean 1:1 a `JobTerminal::spawn`.

Arquitectura confirmada (ver SPEC.md §4): **3 capas** (runtime del repo / agente
michi / infra michi) sobre el **estándar devcontainer**. El wiring va por cuts:

- [x] D.3 (Cut-1) - **Wiring real** ✅ (#42-#50). Hecho: `plan_launch`→spawn,
  imagen slim por lenguaje, bind-mount del binario, activación ON-por-defecto +
  **badge** contenedor/nativo en el header, montaje `~/.claude` + `~/.claude.json`.
  **Pendiente dentro de D.3:** split-anatomy (volúmenes de caché), PTY resize
  (SIGWINCH), inyectar la tarea inicial como primer prompt.
- [ ] D.4 (Cut-2) - Leer `devcontainer.json` del repo si existe (respeta el estándar).
- [ ] D.5 (Cut-3) - Scaffolding: michi genera un `devcontainer.json` para repos sin él.
- [ ] D.6 - Cache de imagen derivada (base + capa agente) para acercar "create en
  <5s". (stop/destroy con `docker rm -f` + GC al arrancar YA hechos en #46.)
- [ ] D.7 - Puertos: published ports (`-p host:8080`) = URL estable por sesión
  (spike ✅; falta cablear el mapeo en el `docker run` real).
- [ ] D.8 - Serializar mutaciones git de michi: cola mpsc por repo
  (`HashMap<git_dir, Sender<GitJob>>`) + `git config gc.auto 0` por worktree.
  Read-only fuera de la cola. (Corrige el blind spot de Gemini sin mutex global.)
- [ ] D.9 - **Persistir** `session_id`/modo de las sesiones managed (hoy en
  memoria en `managed`/`launch_modes`) → Detener/Reabrir sobrevive a reiniciar michi.

**Criterio:** crear un trabajo levanta su contenedor, `claude` corre adentro, el
dev server queda accesible por una URL/puerto estable, y cerrar el trabajo
destruye el contenedor. Sin Docker, michi cae al camino nativo de hoy.

### Fase E — Observabilidad unificada + dogfood

> **Estado:** `PENDIENTE`
> **Objetivo:** un solo lugar para ver qué hace cada agente; usar michi en serio.

- [ ] E.1 - Streams de logs por sesión centralizados (contenedor + dev server +
  compilación) a un panel.
- [ ] E.2 - Atajos globales (Ctrl+N/Tab/W/1..9) + command palette.
- [ ] E.3 - Dogfood 1 semana real; bug list en SESSION.md.
- [ ] E.4 - Postmortem + decisión V1 (`POSTMORTEM.md`).

**Criterio:** Camilo trabaja sus sesiones paralelas reales desde michi una semana.

---

## Resumen de Progreso

| Bloque | Estado |
|--------|--------|
| POC base (Fases 1-6) | ✅ Completado |
| Tanda harness (PRs #27-#33) | ✅ Completado |
| Fase D · Sandbox container-first | 🔄 **Cut-1 ✅ (#42-#50); refinamientos D.4-D.9 pendientes** |
| Fase A · Piso de observación (context menu / Traer a michi) | ✅ Hecho (#43-#44) |
| Fase B · Inyección y arbitraje | ⏳ Pendiente (inyectar prompts) |
| Fase C · DB isolation | ⏳ Pendiente |
| Fase E · Observabilidad + dogfood | ⏳ Pendiente |

**Leyenda:** ⏳ Pendiente · 🔄 En curso · ⏸ Pausada · ✅ Completado

## Stop conditions

Cualquiera de estas → parar y revaluar:

1. La sandbox en contenedor en Windows (Docker Desktop/WSL2) resulta inestable o
   lenta al punto de molestar más de lo que ayuda → el camino nativo sigue siendo
   el default y el contenedor se posterga.
2. DB isolation exige asumir demasiado del proyecto target (no usa
   `DATABASE_URL`) → degradar a "observar + avisar".
3. Aparece cliente #2 / prioridad de negocio → pausar michi, atender eso primero.
4. >50 horas acumuladas desde el pivote sin valor demoable nuevo → reevaluar.
