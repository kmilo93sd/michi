# CLAUDE.md — michi

> Este archivo lo lee Claude Code automáticamente al abrir el repo.
> Si vas a tocar código aquí, léelo primero. Es corto, no hay excusa.

## Qué es michi

**Harness multi-agente** para correr varias sesiones de Claude Code en paralelo
en una misma máquina sin que se pisen: michi **observa** cada sesión, muestra qué
hace y qué recursos consume, y **aísla** esos recursos (git worktree, puertos,
base de datos, contenedores) por sesión.

No es un IDE ni un orquestador cloud. michi no edita código, Claude lo hace.
michi es la **torre de control de tráfico**: corre nativo en tu máquina
(Rust + egui, sin Electron, sin webview), no en un servidor. La terminología
real es **harness / agent orchestration** — sin etiquetas marketineras.

**Dos tipos de sesión:**
- **Managed** — la lanzó michi, tiene PTY embebido y control total (inyectar
  prompts, asignar puertos, aislar su DB, sandboxearla en contenedor).
- **Detectada** — corre fuera de michi (tu terminal, VS Code). michi la VE
  escaneando el host, read-only, hasta que la "traés".

**Dirección V1 (decisión 2026-05-21): container-first.** Las sesiones que michi
lanza van en un sandbox de contenedor cuando hay Docker, con **fallback nativo**
si no (Docker preferido, no requerido — ver regla 4). Detalle completo en
[specs/.../SPEC.md](./specs/20260517-1716-michi-poc/SPEC.md) sección "Estado
actual (2026-05-21)".

Estado: alpha · desarrollo activo. Cross-platform desde día 1
(Windows / macOS / Linux).

## Reglas obligatorias

Estas reglas son **no negociables**. Si las violas en un PR, el reviewer
te pedirá rehacerlo.

### 1. TDD estricto (red-green-refactor)

Toda feature, refactor o bugfix sigue:

1. **RED.** Escribir el test que falla primero, antes de tocar código de
   producción. Si es un bug, el test reproduce el bug y falla.
2. **GREEN.** Escribir el mínimo código necesario para que el test pase.
   Sin features de más, sin abstracciones especulativas.
3. **REFACTOR.** Limpiar el código manteniendo todos los tests verdes.

Reglas derivadas:

- **Cada PR introduce al menos un test nuevo.** Sin excepciones (incluso
  bugfixes traen su regression test).
- **No se commitea con tests rotos.** `cargo test --all-targets` debe pasar.
- **No se "pone el test después" para ahorrar tiempo.** El orden importa:
  primero falla, después pasa.
- Excepción razonable: cambios de UI puramente visuales (theme.toml,
  spacings) donde no hay lógica testable. Estos pasan por revisión visual
  manual.

### 2. Clean code, no parches

- **Cero `#[allow(dead_code)]`** y similares. Si un warning aparece,
  resuélvelo (usá el código, bórralo, o migra la API). Ver
  [feedback_no_parches_clippy_clean](../lelemon-workspace/...) (regla del proyecto).
- **Cero `unwrap()` / `expect()`** fuera de tests y del bootstrap de `main`.
- **`anyhow::Result<T>`** para errores. Agrega contexto con `.with_context()`.
- **Cero `unsafe`** en este POC.

### 3. Pre-commit gates

Antes de cada commit (`cargo` los corre en CI también):

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Si cualquiera falla, no commitees.

### 4. Cross-platform

El código debe compilar y correr en Windows, macOS y Linux.

- Usar `dirs::home_dir()` y `PathBuf::join()`. Nunca hardcodear `\\` o `/`.
- Para diferencias por OS usar `#[cfg(target_os = "...")]` o el crate `cfg-if`.
- No invocar binarios específicos de un OS sin un fallback (`wt` solo
  existe en Windows, `open` solo en macOS, etc.).

### 5. Design system

Todos los colores, fuentes y spacings viven en `src/theme.rs`. El usuario
puede editarlos en `~/.michi/theme.toml`. **Nunca hardcodees** un
`Color32::from_rgb(...)` ni un height fijo en otro archivo.

Lee [DESIGN_SYSTEM.md](./DESIGN_SYSTEM.md) antes de tocar UI. Cubre
tokens, patterns y gotchas conocidos de egui 0.34.

## Estructura del repo

```
src/
├── lib.rs              -- public API del crate (re-exporta los modulos)
├── main.rs             -- binary entry (thin wrapper sobre lib)
├── app.rs              -- struct App + eframe::App impl
├── theme.rs            -- tokens visuales + serde a TOML
├── state/
│   ├── job.rs          -- struct Job + JobStatus
│   ├── workspace.rs    -- struct Workspace + Repo + discovery
│   └── persistence.rs  -- AppState + load/save state.json
├── git/
│   ├── worktree.rs     -- create / remove / list (shell out a git)
│   └── status.rs       -- count_changed_files
├── terminal/           -- egui_term + portable-pty (PTY de sesiones managed)
├── ui/
│   └── new_job_modal.rs -- modal "Nuevo trabajo"
├── claude_sessions.rs  -- detecta TODAS las sesiones claude del host
│                          (managed + externas), estado real, titulo, agrupa por cwd
├── claude_config.rs    -- inventario de skills/agents/MCPs por scope
│                          (global/workspace/repo) para los totales del sidebar
├── resource_monitor.rs -- arbol de procesos por sesion (sysinfo), RAM, classify
├── port_detector.rs    -- detecta PORT_* en .env del repo/worktree
├── port_alloc.rs       -- asigna rangos de puerto por sesion, inyecta env vars
├── system.rs           -- helpers de OS (abrir carpeta en el file manager nativo)
├── workspace_prep.rs   -- detecta/prepara workspaces "pelados" (scaffolding
│                          CLAUDE.md/.claude/specs + git init opcional)
├── worker.rs           -- threads + mpsc (create/remove worktree, status poller)
└── tests embebidos en cada archivo con #[cfg(test)] mod tests { ... }
```

> Roadmap V1 agrega modulos para: DB isolation (postgres compartido + DB por
> sesion), sandbox en contenedor (lifecycle Docker), e inyeccion de prompts a
> sesiones managed. Ver `specs/20260517-1716-michi-poc/PHASES.md`.

Decisión arquitectónica: `lib + bin split`. La lib expone API pública;
`main.rs` es delgado. Esto asegura que cada `pub fn` esté cubierto por
tests aunque `main` no la llame directamente.

## Stack y deps

- **egui 0.34** + **eframe 0.34** — UI nativa, immediate mode
- **egui_term** (git dep) — terminal embebido con alacritty_terminal backend
- **tokio** — runtime async (rt, macros, sync, time, process, io-util)
- **anyhow** — error handling
- **serde + serde_json + toml** — persistencia
- **tracing + tracing-subscriber + tracing-appender** — logs estructurados
  en `~/.michi/logs/michi.log`
- **rfd** — file dialogs nativos del OS
- **uuid v4** — ids de jobs

Dev: **tempfile** para tests integración con git real.

## Workflow

1. **Branch desde main.** Naming: `feature/<slug>`, `chore/<slug>`,
   `fix/<slug>`. Para fases del spec: `feature/<n>-<slug>` (ej
   `feature/3g-wire-modal-to-git`).
2. **Commits descriptivos.** Conventional commits: `feat(scope): ...`,
   `chore(scope): ...`, `fix(scope): ...`. Cuerpo del commit explica QUÉ
   cambió y POR QUÉ.
3. **PR con la template** (`.github/PULL_REQUEST_TEMPLATE.md`). Adjunta
   screenshot si toca UI.
4. **CI corre clippy + fmt + audit + test en Linux/macOS/Windows.**
   Sin verde no se mergea.
5. **Stacked PRs son OK** si un bloque depende del anterior aún sin
   mergear. Después del merge del padre, rebasear el hijo a main fresh.

## Pre-commit checklist

- [ ] Test nuevo agregado (red → green)
- [ ] `cargo fmt --check` pasa
- [ ] `cargo clippy --all-targets -- -D warnings` pasa
- [ ] `cargo test --all-targets` pasa
- [ ] Si toca UI: tomé screenshot
- [ ] Si toca config del usuario (theme.toml, state.json): documenté el
      schema change en el commit

## Gotchas conocidos

Ver [DESIGN_SYSTEM.md sección "Gotchas"](./DESIGN_SYSTEM.md#gotchas-conocidos-de-egui-034)
para los específicos de egui 0.34.

Adicionales:

- **`cargo run` en Windows puede fallar con "Acceso denegado"** si
  `michi.exe` está corriendo de una sesión anterior. Solución: `taskkill
  /F /IM michi.exe` antes de rebuildar.
- **`git worktree add` falla en Windows con paths >260 chars.** Por eso
  el setup requiere `git config --global core.longpaths true` (ver
  `CONTRIBUTING.md`).
- **CARGO_TARGET_DIR del proyecto** apunta a `D:\DevCaches\cargo-target`
  para no llenar C:. Si trabajas en otro disco, sobreescribe la env var.

## Referencias

- [README.md](./README.md) — overview público
- [CONTRIBUTING.md](./CONTRIBUTING.md) — setup dev, code style, PR flow
- [DESIGN_SYSTEM.md](./DESIGN_SYSTEM.md) — tokens, patterns y gotchas UI
- [Spec del POC](./specs/20260517-1716-michi-poc/SPEC.md) — problema, propuesta,
  goals, decisiones de diseño. Ver también `SESSION.md` (contexto para retomar),
  `PHASES.md`, `UI_DESIGN.md` y `RUST_GUIDELINES.md` en esa misma carpeta.
