# Contexto de Sesión - michi POC

> **Propósito:** Información para retomar el trabajo rápidamente en cada sesión.
> Lee este archivo al inicio de cada sesión de trabajo.

---

## Estado Actual (2026-05-21)

**Repo:** `github.com/kmilo93sd/michi` — ~173 tests TDD, `cargo clippy -D warnings` + `cargo fmt` limpios. Workflow: PRs atomicos directo a `main` (NO stacks largos — se rompen al squash-merge con `--delete-branch`).

**PIVOTE 2026-05-20 (lo mas importante):** michi dejo de ser "orquestador de Claude Codes en worktrees" y paso a ser un **harness multi-agente: observabilidad + aislamiento de recursos por agente**. El POC original (Fases 1-6: launcher + sidebar + worktrees) esta DONE en lo esencial; el foco ahora es la VISION V1 (que el SPEC ya tenia) + la nueva direccion. Ver memoria `project_michi_direction.md` y la "Sesion 2026-05-20/21" abajo.

**DIRECCION 2026-05-21 (container-first):** se decidio adoptar el modelo de **sandbox por agente** como rumbo de V1, pero aterrizado: contenedor **PREFERIDO, no requerido** para las sesiones que michi lanza, con **fallback nativo** si no hay Docker (cumple regla 4 cross-platform). Tres planos: (1) piso de observacion nativo universal sin Docker — lo ya construido; (2) aislamiento contenedor-preferido para managed; (3) infra compartida (1 postgres, DB/schema por sesion). El proxy de puertos SI aplica, pero solo en sesiones contenerizadas (ahi cada una tiene su red). Se descartan micro-VMs/Firecracker (Linux-only), cgroups (analogo = Job Objects/sysinfo ya presente), CoW Btrfs/ZFS (worktrees ya son el CoW pragmatico) y NATS (mpsc/tracing alcanza). Detalle completo en `SPEC.md` seccion "Estado actual (2026-05-21)" y `PHASES.md` "Roadmap V1". La spec ademas se MOVIO a este repo (`michi/specs/...`); antes vivia en lelemon-workspace.

**Modelo conceptual cerrado:**
- **Sesion = entorno aislado** (no = spec; una sesion ataca varias specs). Cada sesion Claude vive en el cwd del workspace, crea worktrees y levanta servicios.
- **Dos tipos de sesion:** *managed* (michi la lanzo, tiene PTY embebido, control total) y *detectada* (corre fuera de michi — terminal/VS Code; michi la VE escaneando procesos, read-only hasta "traerla").
- **Diferenciadores validados por investigacion** (harness engineering, nadie los hace bien): resource tree por agente + database isolation automatica.

**Lo construido en esta tanda (PRs #27-#33, todos en main):**
- `resource_monitor`: arbol de procesos por sesion (sysinfo), RAM agregada, `classify_processes` (shells/runtimes/docker), `subtree_details`.
- `claude_sessions`: detecta TODOS los claude.exe del sistema (filtra desktop app/Electron/claude-meter), agrupa por workspace (cwd), lee estado real de `~/.claude/sessions/<pid>.json` (busy/idle/waiting/shell), titulo legible (primer mensaje del `.jsonl`), breakdown + procesos.
- `port_detector` + `port_alloc`: detecta `PORT_*` en `.env`, asigna rango por sesion (4100/4200/...), inyecta env vars al PTY via shell wrapper.
- UI: cards de sesion con dot de estado real + titulo + chips (shells/runtimes/docker) + puertos. Click en detectada → panel central con detalle de procesos (nombre/pid/RAM). Orden estable por pid.

**Habilitador tecnico CLAVE:** `TerminalBackend::pty_id()` de egui_term YA da el OS PID real (no hace falta forkear). Y `~/.claude/sessions/<pid>.json` da estado+sessionId+cwd. Y `~/.claude/projects/<encoded-cwd>/<sessionId>.jsonl` da el historial (primer mensaje = titulo).

**PROXIMA TAREA (lo que se estaba por arrancar al reiniciar):** Context menu en cards de sesion detectada con 3 acciones:
1. **Renombrar** — alias local override del titulo, persistido por sessionId.
2. **Cerrar sesion** — mata el proceso claude + arbol (destructivo, pide confirmacion).
3. **Traer a michi** — cierra la externa + reabre con `claude --resume <sessionId>` en PTY embebido (managed). Historial se MANTIENE (resume lee el .jsonl); se pierden procesos hijos vivos (servicios levantados) + cache de prompt. Confirmacion que avisa el estado ("esta sesion tiene node+docker, traerla los cierra").

"Traer a michi" es el HABILITADOR de todo lo demas: una vez managed, michi puede **inyectar prompts** al PTY (`backend.process_command(Write(texto+Enter))`) → caso de uso god: michi detecta conflicto/problema → boton "arreglar con Claude" → inyecta prompt → Claude corrige.

## Archivos Clave

| Archivo | Por qué es relevante |
|---------|---------------------|
| `SPEC.md` | Resumen, problema, propuesta, goals, non-goals — leer primero |
| `RUST_GUIDELINES.md` | Best practices y antipatterns. Consultar ANTES de cada decisión de diseño |
| `UI_DESIGN.md` | Mockups ASCII y comportamientos de la UI |
| `PHASES.md` | Plan por fases con criterios de éxito y stop conditions |
| `C:\Users\kmilo\Documents\projects\michi\` | Repo del POC (a crear en Fase 1) |

## Comandos Frecuentes

```powershell
# Levantar el POC en dev
cd C:\Users\kmilo\Documents\projects\michi
cargo run

# Build release
cargo build --release
# Output: target\release\michi.exe

# Format + lint pre-commit
cargo fmt
cargo clippy -- -D warnings
cargo test

# Logs en runtime
Get-Content "$env:USERPROFILE\.michi\logs\michi.log" -Wait -Tail 50
```

## Decisiones Tomadas

| Fecha | Decisión | Razón |
|-------|----------|-------|
| 2026-05-17 | Override del postpone del IDE Rust | Camilo decidió POC explícito, runway y prioridades evaluadas |
| 2026-05-17 | Stack: Rust + egui + portable-pty + alacritty_terminal | GUI nativa ligera, sin Electron, comunidad activa |
| 2026-05-17 | Repo del POC en `C:\Users\kmilo\Documents\projects\michi` | Independiente, paralelo a otros workspaces |
| 2026-05-17 | Nombre: `michi` | Corto, brandeado, `lc` como alias futuro |
| 2026-05-17 | POC scope cerrado a 6 fases (~24-36h) | Evitar scope creep, ver PHASES.md |
| 2026-05-17 | Sin emojis en UI, dark mode, monoespaciada en sidebar | Consistente con preferencias documentadas en memoria |
| 2026-05-17 | Worktrees creados en `<workspace>-wt/<branch-slug>` | Paralelo al workspace base, fácil de limpiar |
| 2026-05-18 | Código cross-platform desde día 1 (no solo Windows) | Camilo confirmó que mac viene después. Costo extra del approach cross-platform = 0 (todas las deps elegidas ya lo son). Regla agregada a RUST_GUIDELINES sección 6. |
| 2026-05-18 | `CARGO_TARGET_DIR=D:\DevCaches\cargo-target` (env var User scope) | Para no llenar C: con build artifacts dentro del propio repo michi. Consistente con regla "caches dev nunca en C:" de la reorg de discos. |
| 2026-05-18 | Toolchain MSVC + VS Build Tools 2022 (no GNU) | MSVC es la apuesta segura para crates con bindings nativos (eframe/wgpu/portable-pty). Trade-off: ~7GB vs ~500MB de GNU. |
| 2026-05-18 | Cargo.toml deja `cargo add` resolver versiones (no hardcodear) | Cutoff de modelo es enero 2026, egui libera rápido. Confiar en cargo + Cargo.lock para reproducibilidad. |
| 2026-05-20 | Pivote a harness multi-agente (observabilidad + aislamiento de recursos) | El dolor real es el choque de recursos entre sesiones paralelas, no solo el worktree. Resource tree + DB isolation son el gap que nadie resuelve. |
| 2026-05-21 | Dirección V1: **container-first** (sandbox por agente) con fallback nativo | Adopta el modelo sandbox de la reflexión, aterrizado a escritorio/cross-platform: contenedor preferido-no-requerido, piso de observación nativo universal, infra compartida + DB por sesión. Descartado lo Linux-only (Firecracker/cgroups/Btrfs). |
| 2026-05-21 | Spec movida a `michi/specs/` (antes en lelemon-workspace) | La spec es de michi, corresponde versionarla con su código. CLAUDE.md y README apuntan ahora a la copia local. |

## Bloqueadores / Pendientes

- [x] ~~**Disco C: estuvo lleno**~~ → **Resuelto 2026-05-18**: 190GB libres tras limpieza. Rust 1.95.0 + cargo instalados desde antes (en `~/.cargo/bin`).
- [x] ~~Confirmar si `claude` CLI tiene flag para inyectar primer prompt~~ → **Resuelto**: `claude [prompt]` toma el prompt como argumento posicional. También útiles: `--name`, `--session-id`, `-w/--worktree` (Claude crea worktree solo — descartado, mejor control con `git worktree add` explícito)
- [x] ~~`git config --global core.longpaths true`~~ → **Resuelto 2026-05-18**: seteado (solo Windows aplica)
- [x] ~~**Versiones de crates al bootstrap**~~ → **Resuelto 2026-05-18**: bootstrap.sh ejecutado, cargo eligió últimas estables. Ver decisión 2026-05-18.
- [x] ~~Investigar crate `egui-term` o similar~~ → **Resuelto 2026-05-18**: usar **`egui_term`** (github.com/Harzu/egui_term, también en crates.io + docs.rs, CI activo). Features cubren todo lo de PHASES.md Fase 4: PTY rendering, multiple instances (clave porque vamos 4+ jobs paralelos), keyboard input, resize, scroll, focus, selection, font/color scheme, hyperlinks. **"Tested on MacOS, Linux, and Windows"** → refuerza el approach cross-platform. Falta validar compatibilidad con egui 0.34.2 al `cargo add` en Fase 4 (puede requerir bump o pin de versión). Esto baja la complejidad estimada de Fase 4 de 8-12h a probablemente 4-6h, y libera evaluar la tarea 4.5 (renderer custom desde alacritty raw) como innecesaria. Alternativas evaluadas: `Quinntyx/egui-terminal`, `PaulWagener/egui-terminal` (viejo, 2025).
- [ ] **VS Build Tools 2022 instalándose en background** (winget ID `bkhev694g`, ~7GB, ~10-20min). Sin esto el linker MSVC no funciona y `cargo build` falla con "link.exe returned unexpected error".
- [ ] **API egui puede haber cambiado**: verificar al compilar contra egui 0.34.2; ajustar `ViewportBuilder`/`run_native`/etc. si cambió desde el template (mediados 2025).
- [ ] **Cross-platform**: cuando Camilo tenga mac, validar que compile sin cambios. Setup mac: `xcode-select --install` y listo (ver SPEC.md non-goal actualizado).

## Notas de Sesiones Anteriores

### Sesión 2026-05-21 (cont.) — spike, código D.1/D.2 y visión de Gemini

**Construido (4 PRs en main):** #34 (pivote + D.1 detección de Docker), #35 (D.2 `build_run_args`), #36 (D.2 `plan_launch` planner contenedor-vs-nativo), #37 (arquitectura 3 capas + brief Gemini). Toda la lógica pura de Fase D testeada (TDD). Falta el wiring (Cut-1).

**Spike D.0 hecho** (todo verde): auth portable, edit round-trip, DB isolation (1 pg=42MiB), caps OOM, puertos published, pared #1 (bind mount ~12x).

**Visión de Gemini incorporada (ver SPEC.md §5).** Adoptado: bind-mount del binario claude (0ms build), slim por lenguaje (no base universal 10GB), split-anatomy (fuente en Win, build/caché en volumes), redis efímero por sesión, GC de huérfanos al arranque, override del `DATABASE_URL` si el repo trae su compose. Corregido: (1) git no comparte `index.lock` entre worktrees (cada uno tiene el suyo) → solo `gc.auto 0` + serializar mutaciones michi por cola mpsc-por-repo, sin mutex global; (2) contenedores son arm64 en Apple Silicon, no siempre x86_64 → binario claude arch-matched.

**Próximo:** Cut-1 (wiring real en la UI) — o DB-isolation-nativa primero (son ortogonales, decisión de Camilo).

### Sesión 2026-05-21 (decisión container-first + alineación de docs)

**Que paso:** Camilo trajo una reflexión con Gemini sobre cómo gobernar muchos agentes de código en paralelo (choques de puertos, docker duplicado, recursos). Gemini propuso un orquestador cloud genérico (Firecracker, cgroups, Btrfs CoW, Traefik, NATS). Se evaluó contra la realidad de michi (escritorio, Windows-first, cross-platform, mono-usuario): la mitad es Linux-only y rompe la regla 4. Se rescató lo que sí encaja (DB por schema, puertos/proxy, caché compartida — varias ya en el backlog del SPEC).

**Decisión:** Camilo eligió **dirección container-first** (sobre la recomendación de quedarse 100% nativo). Se reconcilió como **container-preferido + piso nativo + fallback nativo** (regla 4). Reframe clave: los agentes no son "procesos hostiles" (modelo cloud) sino "tuyos pero descoordinados" → michi es un **controlador de tráfico de recursos**, no una cárcel de seguridad.

**Hecho esta sesión:** spec movida a `michi/specs/`; `CLAUDE.md` y `README.md` actualizados al modelo harness + container-first; `SPEC.md` con sección de decisión + mapa de profundidad (niveles 1-7) + foco; `PHASES.md` reescrito (POC base + tanda harness completos, Roadmap V1 con Fases A-E, **Fase D = FOCO**, A pausada); este registro.

**FOCO elegido (2026-05-21):** el **track contenedor (Fase D)**. Camilo: "no podemos hacer todo, hay que decidir; el de contenedores es bueno, nunca lo probé, quiero explorarlo". Por eso: **spike manual antes de código**. Prereqs verificados: Docker 27.3.1 + Compose v2.30 corriendo, `claude` en PATH (`~/.local/bin/claude`), `~/.claude/.credentials.json` existe (token OAuth, montable).

**Hallazgos del spike D.0 (2026-05-21) — TODO VERDE:**
- **Auth portable (Nivel 1):** claude se instala vía npm en `node:22-slim` y **autentica** montando solo `.credentials.json` (token OAuth) en un `/root/.claude` fresco. El token de Windows funciona en contenedor Linux. Riesgo #1 de la Fase D = descartado.
- **Edit round-trip:** claude editó un archivo montado del host y el cambio apareció en Windows. Volumen ida y vuelta OK.
- **Pared #1 (perf bind mount):** 2000 archivos chicos → bind mount Win **5.05s** vs named volume **0.42s** vs FS interno **1.35s**. Bind mount **~12x más lento**. Decisión: artefactos de build (`target/`, `node_modules`) NO en bind mount de path Windows; usar named volume o worktree en WSL2.
- **DB isolation (Nivel 3):** 1 postgres compartido + DB efímera por sesión (`session_a`/`session_b`) vía `DATABASE_URL`. Sesión B no ve la tabla de A (aislado), el dato de A persiste. 1 postgres = **42 MiB** para ambas. El diferenciador, probado.
- **Cap de recursos (Nivel 4):** `--memory 256m` + alocar sin freno → **OOM-kill (exit 137)**. `docker stats` muestra el límite (512MiB) y CPU. "Matar/limitar agente loco" sale gratis de Docker.
- **Imagen `michi-spike`** (node:22-slim + claude + git + psql) construida = embrión de `michi-base`.

**Implicaciones de diseño para Fase D (confirmadas por el spike):**
- **Auth:** copiar `.credentials.json` a un home efímero del contenedor (no montar todo `~/.claude` rw).
- **Storage:** named volume / WSL2 para build artifacts; bind mount solo para fuente liviana.
- **DB:** postgres compartido en una red docker; `CREATE DATABASE session_<id>` + `DATABASE_URL` inyectado; drop al cerrar.
- **Recursos:** `--memory`/`--cpus` por sesión + `docker stats` para el `resource_monitor` en modo contenedor.

**Próximo:** decidir si arrancamos a codear Fase D en michi (D.1 detección de Docker → D.2 lanzar sesión managed en contenedor) o seguimos con más spike (puertos/proxy, imagen base por repo).

### Sesión 2026-05-20/21 (pivote a harness de recursos + construccion intensa)

**Que paso:** tras tener el POC funcional (sidebar, worktrees, terminal embebido, workspace prep, totales de skills/MCPs), Camilo aterrizo el dolor REAL: trabaja con varias sesiones largas de Claude en paralelo sobre un workspace, y se le chocan en codigo/procesos/docker, no sabe que puerto quedo levantado ni cual sesion chupa RAM. Investigamos harness engineering (estado del arte) → confirmado que el resource tree por agente + database isolation son el gap que nadie resuelve. michi pivota a eso.

**Decisiones de arquitectura conversadas (CLAVE para retomar):**

1. **michi resuelve lo determinista, Claude lo no determinista.** No delegar TODO a Claude (gasta tokens, se enreda). Worktrees/git/status/puertos = michi. Refactors/conflictos/config compleja = Claude.

2. **Niveles de integracion (graceful degradation) — como michi calza distintos entornos (moderno desde 0, legacy, intermedio):**
   - **Nivel 0 · Observar** (procesos, puertos activos netstat, RAM, conflictos) → requiere NADA, funciona en cualquier proyecto. Es el piso universal.
   - **Nivel 1 · Inyectar puertos** (PORT_* por sesion) → requiere que el codigo lea env vars.
   - **Nivel 2 · DB isolation** (DATABASE_URL → DB por sesion) → requiere que use DATABASE_URL.
   - **Nivel 3 · Orquestar infra** (postgres/redis compartido, crea DBs) → requiere docker-compose o servicios declarados.
   michi detecta señales (docker-compose? .env con DATABASE_URL/PORT_*?) y aplica el nivel mas alto posible. Lo que no puede automatizar → observa + avisa + sugiere (o se lo pide a Claude). Posible `.michi/env.toml` opcional por workspace para override.

3. **Servicios: estrategia hibrida** (NO levantar todo separado):
   - Infraestructura/estado (postgres, redis): **compartido** — 1 contenedor, 1 DB/schema por sesion. Ahorra RAM masiva.
   - Codigo en ejecucion (dev servers node/vite): **separado** por sesion (puertos distintos) — ahi el aislamiento SI importa.

4. **Naming:** NO inventar etiquetas marketineras ("control plane de agentes" fue rechazado por inventado). Terminologia real: "harness", "agent orchestration". Pitch final = copywriter humano.

5. **Interactuar con sesiones:** michi puede inyectar prompts solo a MANAGED (escribe al PTY). Externas → traer primero. Caso god: michi detecta problema → boton "arreglar con Claude" → inyecta prompt de correccion.

**Backlog priorizado (post-context-menu):**
1. Context menu detectadas: renombrar / cerrar / traer a michi (PROXIMA — en curso al reiniciar).
2. Enviar prompt a managed (primitiva: escribir al PTY).
3. Puertos LISTEN reales (netstat2 / crate `netstat2`) — nivel 0 universal, ve la realidad sin importar como esten declarados. Cruzar puerto↔PID↔sesion.
4. Deteccion de conflictos (2 sesiones mismo puerto) + boton "arreglar con Claude".
5. Database isolation automatica (nivel 2-3) — postgres compartido + DB por sesion. El otro diferenciador.
6. Contenedores docker con nombres (docker ps) por sesion.
7. Inyectar tarea inicial del modal como primer prompt (PR 15 pendiente) + display_name auto.
8. Aplicar resource tree/estado tambien a sesiones managed (hoy el estado real solo en detectadas).

**Modulos del repo (estado actual):** app, claude_config, claude_sessions, git, port_alloc, port_detector, resource_monitor, state (job/workspace/persistence), system, terminal, theme, ui, worker, workspace_prep.

### Sesión 2026-05-17 (planning)

- Spec completo creado: SPEC.md, RUST_GUIDELINES.md, UI_DESIGN.md, PHASES.md, este SESSION.md
- Stack decidido y justificado en alternativas consideradas
- UI bosquejada en ASCII con detalle de estados, modales, acciones
- Próximo paso: arrancar Fase 1 (bootstrap del repo)

## Cómo retomar

1. Leer `SPEC.md` (skim, ~3 min)
2. Leer `RUST_GUIDELINES.md` (la primera vez en detalle, después solo cuando dudas)
3. Leer este SESSION.md
4. Revisar `PHASES.md` para ver qué tarea sigue
5. Empezar a tirar código

## Cómo cerrar sesión

1. Commitear lo que esté listo
2. Actualizar este SESSION.md:
   - "Última tarea completada"
   - "Próxima tarea"
   - Decisiones nuevas
   - Bloqueadores nuevos
3. Agregar entrada en "Notas de Sesiones Anteriores"
