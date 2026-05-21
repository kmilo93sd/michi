# michi POC

> **Status:** `IN_PROGRESS`
> **Creado:** 2026-05-17
> **Actualizado:** 2026-05-21

---

## Estado actual (2026-05-21) — pivote a harness + dirección container-first

> Esta sección manda sobre la narrativa original del POC (más abajo), que queda
> como registro histórico de cómo llegamos acá. Para el detalle de implementación
> y el backlog vivo, ver `SESSION.md` y `PHASES.md`.

**Dos cambios de rumbo posteriores al POC original:**

### 1. Pivote 2026-05-20 — de "launcher de worktrees" a "harness multi-agente"

El POC base (Fases 1-6 originales: launcher, sidebar, worktrees, terminal
embebido, persistencia) está esencialmente **DONE** (~173 tests, PRs #27-#33 en
`main`). El foco pasó a **observabilidad + aislamiento de recursos por agente**.
michi ya detecta TODAS las sesiones Claude del host (managed y externas), su
árbol de procesos, RAM y puertos declarados.

### 2. Decisión 2026-05-21 — dirección container-first (sandbox por agente)

Tras reflexión (incluyendo input externo que describía un orquestador cloud
genérico), michi adopta el **modelo de sandbox por agente** como dirección de V1.
Pero aterrizado a la realidad de michi (escritorio, Windows-first, cross-platform,
un dueño), NO como orquestador cloud. Tres planos:

- **Piso de observación (Nivel 0, universal, nativo, sin Docker):** michi siempre
  escanea el host — procesos claude, árbol de recursos (sysinfo), puertos LISTEN
  reales (netstat2). Funciona esté la sesión en contenedor o nativa, lanzada por
  michi o externa. Es lo ya construido y **nunca exige Docker**.
- **Plano de aislamiento (contenedor preferido, fallback nativo):** para las
  sesiones que michi *lanza*, el default es un sandbox en contenedor (el agente +
  sus dev servers + tools viven adentro; el worktree se monta como volumen). Si
  no hay Docker o el usuario opta por no usarlo → fallback nativo (worktree +
  puertos por env), el comportamiento de hoy. Cumple la regla 4 del CLAUDE.md:
  Docker es **preferido, no requerido**.
- **Plano de infra compartida:** UN postgres/redis compartido entre sesiones;
  michi crea una DB/schema efímero por sesión, inyecta `DATABASE_URL` y lo dropa
  al cerrar. Es el diferenciador y es ortogonal a si la sesión corre en contenedor
  o nativa.

**Puertos:** en sesión contenerizada cada contenedor tiene su red propia → ahí SÍ
aplica el proxy (cada agente cree estar en :8080, michi publica/mapea a un host
port único o rutea por reverse proxy → URL estable por sesión). En sesión nativa →
asignación de puertos por env var (lo de hoy).

**Runtime:** Docker + docker-compose como sustrato (Docker Desktop/WSL2 en
Windows, nativo en Linux/Mac). `devcontainer.json` se lee como config opcional si
existe. **Firecracker queda diferido** (KVM = solo Linux; revisar para un modo
"servidor Linux" futuro) para no romper cross-platform.

**Sesiones detectadas (externas):** se siguen observando read-only por el piso.
"Traer a michi" gana una variante: relanzar dentro de un sandbox.

**Lo que NO se adopta del modelo cloud genérico, y por qué:** micro-VMs /
Firecracker (Linux-only), cgroups (Linux-only; análogo = Windows Job Objects /
`sysinfo`, ya presente), CoW Btrfs/ZFS (no portable; los git worktrees ya son el
CoW pragmático — comparten `.git`, no reclonás), NATS (overkill mono-máquina;
`mpsc` + `tracing` alcanzan).

**Consecuencias asumidas (riesgos a manejar):**

- Dependencia nueva pesada (Docker Desktop en Win/Mac). michi la detecta y degrada
  con gracia; NO es requisito duro.
- Scope mayor: lifecycle de contenedores (build/start/stop/destroy), montaje del
  worktree, volúmenes de caché de deps. Fase propia (ver PHASES.md, Fase D).
- Tensión con la levedad original (<100MB, nativo). Mitigación: el contenedor es
  preferido-no-requerido; el camino nativo sigue siendo el default liviano.

### 3. Foco y profundidad del enfoque container (2026-05-21)

**Decisión: el enfoque es el contenedor.** No se hace todo a la vez — el track
container-first (Fase D de PHASES) es **el foco**; A/B/C quedan como soporte
alrededor. Como es territorio nuevo para Camilo, se explora con un **spike manual
ANTES de escribir código de michi**: validar a mano en Windows+Docker, después
codificar solo lo que ya se sabe que funciona.

Profundidad posible del enfoque, para no perder el horizonte:

| Nivel | Qué | Horizonte |
|-------|-----|-----------|
| 1 | Claude corre dentro del contenedor (worktree + creds montadas) | Spike |
| 2 | Entorno de dev completo por agente (runtime, dev server, tests, puertos) | V1 |
| 3 | Infra compartida + DB isolation (1 postgres, DB efímera por sesión) | V1 |
| 4 | Gobierno de recursos (`--memory`/`--cpus`/`--pids-limit` + `docker stats`) | V1 |
| 5 | Snapshots / fork / time-travel (`docker commit` / checkpoint CRIU) | V-futuro (CRIU inestable en Win) |
| 6 | Políticas de red / sandbox real (egress restringido) | V-futuro |
| 7 | Entornos reproducibles y portables cross-platform | payoff transversal |

**Sweet spot de V1 = niveles 1-4.** El techo es un "Codespaces/Replit local":
cada agente en un entorno efímero aislado, capado en recursos, reproducible, con
servicios compartidos detrás, todo observable desde un panel nativo.

**Paredes conocidas (Windows):**

- Docker Desktop = VM Linux (WSL2): cuesta RAM y los **bind mounts Win→contenedor
  son lentos**; el hot-reload del dev server se pone caprichoso. Mitigación:
  worktree dentro del FS de WSL2, o named volumes en vez de bind mount. Pared #1.
- Auth de Claude adentro: `.credentials.json` es un token OAuth; validar refresh /
  binding a machine-id en el spike (es lo primero que se prueba).
- Latencia de arranque: imagen base prebuildeada (`michi-base`) para no romper el
  goal de "create en <5s".
- Nivel 5 (checkpoint/restore) fuera de mesa en Windows hoy.

### 4. Arquitectura del contenedor (2026-05-21): modelo de 3 capas + estándar devcontainer

**Estándar adoptado: dev containers (containers.dev).** Spec abierta (Microsoft)
que usan VS Code, GitHub Codespaces y Gitpod. Define el entorno de un repo en
`devcontainer.json` (imagen / Dockerfile / compose + "features" componibles).
michi se para sobre este estándar en vez de inventar; incluso puede invocar
`@devcontainers/cli` (la implementación de referencia) para el lifecycle. Si un
repo no tiene devcontainer → **imagen universal de fallback** (modelo Codespaces).

**Modelo de 3 capas:**

```
Capa INFRA (michi)     → puertos publicados, DB efímera, volúmenes de caché
Capa AGENTE (michi)    → claude + git, inyectados SIEMPRE (instalador standalone)
Capa RUNTIME (el repo) → devcontainer.json / Dockerfile, o base universal michi
```

El repo es la **fuente de verdad del runtime**; michi le agrega la capa de agente
encima. Así no se enumeran lenguajes: el repo (o la base universal) trae
node/rust/python y michi solo garantiza que `claude` esté presente. El agente se
layerea con el **instalador standalone** de claude (binario, sin dependencia de
node) para poder ir sobre cualquier base.

**Velocidad:** caché de deps en **named volumes** (`~/.cargo`, pnpm store,
`~/.npm`), nunca en bind mount de path Windows (pared #1 medida en el spike).

**Activación (default inteligente + override 1 click, no checkbox tonto):**

- Repo con `devcontainer.json` + Docker → contenedor (alta confianza).
- Repo sin devcontainer + Docker → contenedor con base universal (badge visible).
- Sin Docker, o "usar nativo" → nativo (fallback, el flujo de hoy).

**Bonus (valor michi puro):** michi puede **generar** un `devcontainer.json` para
repos que no lo tienen → los vuelve reproducibles ("michi resuelve lo determinista").

**Camino incremental:**

- **Cut-1:** base universal configurable + capa claude (standalone) + caches en
  volúmenes; activación default-inteligente con toggle override + badge.
- **Cut-2:** leer `devcontainer.json` del repo si existe.
- **Cut-3:** scaffolding (michi genera el `devcontainer.json`).

### 5. Refinamientos post-consulta a Gemini (2026-05-21)

Tras pasarle el [GEMINI_BRIEF.md](GEMINI_BRIEF.md), incorporamos (filtrando, no
tragando):

**Adoptado:**

- **Capa agente por bind-mount del binario, no por imagen derivada:** michi guarda
  el binario standalone de claude para Linux en `~/.michi/bin/` y lo monta
  `-v .../claude:/usr/local/bin/claude:ro`. 0ms de build, actualizable sin rebuild.
- **Imágenes slim por lenguaje, no base universal monstruo** (la universal de
  Codespaces pesa 10GB+): detectar `Cargo.toml`→`rust:slim`, `package.json`→
  `node:slim`, fallback `debian:slim`. Combinado con el bind-mount del binario →
  usar imágenes oficiales **stock**, sin imagen michi propia (salvo `+git`).
- **Split-anatomy para la pared #1:** código fuente en el FS de Windows (para que
  el IDE del host ande) + dirs de build/caché en named volumes
  (`CARGO_TARGET_DIR=/vols/target`, volumen en `node_modules`). Compilación a
  velocidad ext4, edición desde Windows.
- **DB isolation:** migraciones las corre Claude / el dev server, **nunca michi**
  (michi solo provee el `DATABASE_URL`). **Redis: contenedor efímero por sesión**
  (no multi-tenant — los DB 0-15 son globales y `FLUSHALL` es peligroso; pesa
  ~10MiB). **GC de huérfanos al arranque:** tokio task que dropea DBs `session_*` y
  contenedores `michi-*` sin sesión activa (anti-crash).
- **devcontainer del repo con su propio compose de DB:** michi debe **pisar** el
  `DATABASE_URL` para forzar su postgres compartido en vez del que levante el repo.
- **Badge de activación** (adaptado a la regla "sin emojis"): dot de color + texto
  (`● Container · node` / `Nativo` / `⚠ Nativo (Docker offline)`), click → popover
  con recursos asignados y puertos mapeados.

**Corregido / matizado (donde Gemini se equivocó):**

- **Contención de git:** Gemini avisó de `.git/index.lock` compartido, pero **cada
  worktree tiene su PROPIO index** (`.git/worktrees/<id>/index`) → `git
  add/commit/status` no contienden ahí. Riesgo real (menor): `packed-refs.lock` y
  sobre todo el **auto-gc** post-commit. Mitigación liviana: `git config gc.auto 0`
  por worktree + serializar solo las mutaciones git **propias de michi** (worktree
  add/remove) por la cola del worker (mpsc por repo). NO hace falta un mutex global
  sobre cada `git` de cada agente.
- **Arquitectura del contenedor:** no es "siempre x86_64" — en Apple Silicon los
  contenedores son **arm64**. El binario de claude a montar debe ser
  arch-matched (x64 + arm64), por la regla cross-platform.

**Cola de git en Rust (respuesta a Gemini):** `HashMap<PathBuf /*git_dir*/,
Sender<GitJob>>`, un consumer por repo; las mutaciones van por mpsc (serializadas
por construcción, FIFO); read-only fuera de la cola. Encaja con `worker.rs` actual.

**Sequencing:** DB isolation puede entregarse **en modo nativo primero** (inyectar
`DATABASE_URL` a un postgres compartido sin contenedor de la sesión), desacoplando
el diferenciador de mayor valor del wiring más riesgoso. Cut-1 (contenedor) y
DB-isolation-nativa son ortogonales.

---

## Resumen

POC en Rust + egui de un panel de control para gestionar múltiples Claude Codes en paralelo, cada uno en su propio `git worktree` aislado. Resuelve el dolor de trabajar 4+ trabajos simultáneos sin pisarse en git ni perder contexto al switchear.

## Problema

Camilo trabaja 4 Claude Codes en paralelo (2 venpu-workspace + 2 lelemon-workspace) y al atascarse en uno switchea al siguiente. Hoy esto no funciona porque:

1. Abrir 2 instancias en el mismo repo pisa cambios de git
2. No hay flujo automático de crear rama + worktree para cada tarea
3. No hay forma visual de ver en qué trabajo está cada instancia
4. Switch manual entre ventanas/terminales es fricción

Las herramientas existentes (Conductor, Crystal, Claude Squad) o no son Windows o tienen UX rechazada por Camilo. Ver [project_ide_agentico_rust_backlog.md](../../C--Users-kmilo-.claude/projects/C--Users-kmilo-Documents-projects-lelemon-workspace/memory/project_ide_agentico_rust_backlog.md).

## Propuesta

Construir un POC en Rust + egui que:

1. Lista todos los trabajos activos (worktree + claude code corriendo) en sidebar izquierdo
2. Muestra el terminal embebido del trabajo seleccionado en pane derecho
3. Permite crear "Nuevo trabajo" → crea worktree en rama nueva + lanza claude code
4. Muestra estado de cada job (idle, pensando, requiere atención, error)
5. Permite commit & push y kill desde la UI sin salir de la app

POC = 1 semana part-time. Si después de usar 2 semanas resuelve el dolor real, decidir si vale convertir en producto.

## Goals

- [ ] App nativa que arranca en <500ms y consume <100MB RAM (target inicial Windows, código cross-platform desde día 1)
- [ ] Crear nuevo trabajo (worktree + claude code) en <5 segundos
- [ ] Switch entre trabajos en <100ms
- [ ] Terminal embebido funcional para correr claude code (alacritty_terminal)
- [ ] Persistir lista de jobs entre sesiones (`~/.michi/state.json`)
- [ ] Operaciones git: worktree add/remove, status, commit, push desde la UI
- [ ] Status dots con 5 estados visibles claramente
- [ ] **Gestión de entorno por worktree (80/20)**:
  - Copiar `.env.local` del repo base al worktree al crearlo (si existe)
  - Permitir override de `PORT_WEB` y `PORT_API` al crear el job, pasarlas como env vars al claude code
  - Mostrar los puertos asignados en el header del terminal pane

## Non-Goals

- **No** validación end-to-end en macOS durante el POC (código cross-platform desde día 1 por construcción — todas las deps elegidas funcionan en mac/win/linux — pero el dogfood inicial es solo en Windows). macOS se valida cuando Camilo tenga acceso a una mac.
- **No** monitor de procesos/puertos del workspace (V2)
- **No** editor de código embebido (V2, abrir en VSCode si hace falta)
- **No** file tree (V2)
- **No** split view de múltiples terminales (V2)
- **No** themes ni settings UI (V2)
- **No** publicar como producto, no ads, no marketing — uso personal POC
- **No** soporte multi-usuario, multi-cuenta, ni nada cloud
- **No** gestión de MCP servers (V2 — propuesto 2026-05-18)
- **No** gestión de Skills (V2 — propuesto 2026-05-18)
- **No** memoria compartida entre Claude Codes (V3 — propuesto 2026-05-18)

## Vision expandida 2026-05-18 (madrugada) — power user god mode

Camilo aclaro de noche (2026-05-18, sesion paso-de-largo) que michi no es solo "panel de worktrees", es el panel de control del entorno de desarrollo agentico. Modelo de 3 niveles:

```
WORKSPACE (carpeta padre — ej "venpu-workspace", "lelemon-workspace")
├── CLAUDE.md           — reglas globales del workspace
├── specs/              — sistema de planning compartido (skill /spec)
├── .agents/, .claude/  — skills + reglas compartidas
├── docker-compose      — containers asociados al workspace
├── REPO 1              — ej "venpu-backend"
│   ├── CLAUDE.md       — reglas propias del repo
│   ├── .claude/        — skills propias del repo
│   ├── docker          — postgres/redis propios del repo
│   └── WORKTREES/JOBS  — instancias paralelas de Claude Code
├── REPO 2              — ...
```

Features de la vision expandida (entran a V1 segun decision 2026-05-18 03:00 — "olvidate del scope, demosle"):

1. **Workspace como entidad first-class**: discovery automatico de workspaces (escanea `~/Documents/projects/*-workspace/`), lectura de CLAUDE.md por nivel, sidebar muestra arbol 3 niveles.
2. **Specs activos del workspace**: lectura de `specs/` con status (in_progress, done), boton para crear nuevo spec via skill `/spec` desde la UI.
3. **Skills visibles**: lista de skills aplicables al workspace/repo desde `.agents/`, `.claude/`.
4. **Docker integration por workspace**: detectar `docker-compose.yml` o containers etiquetados, levantar/bajar desde UI, ver logs.
5. **Port management proactivo**: detectar puertos ocupados (netstat/lsof por OS), auto-asignar puertos libres a worktrees nuevos para evitar conflictos entre claude codes del mismo repo en branches distintas.
6. **Dev server per worktree**: boton "start dev" por job, lanzar `pnpm dev` u el comando configurado con puerto asignado, ver el dev server corriendo (status, logs).
7. **DX power user**: shortcuts globales (Ctrl+N nuevo, Ctrl+Tab switch, Ctrl+1..9 saltar), command palette (Ctrl+P?), densidad alta, anticipa decisiones.
8. **Asset preview inline (insight 2026-05-18 04:54)**: cuando un Claude Code en un job genera un asset (PNG, SVG, PDF, imagen), michi debe mostrar el preview en el pane derecho o en un sub-pane sin obligar al usuario a abrir explorer.exe. Caso de uso real que disparó esto: durante la sesion del logo, Camilo no podia ver los previews PNG generados sin abrir el visor del OS — friccion fatal en un IDE para devs creativos. Mecanismo propuesto: detectar archivos nuevos en el worktree, ofrecer click-to-preview overlay sobre el terminal; o un toggle "asset gallery" del job.
8. **Configuraciones extensibles (anotado 2026-05-18 03:30)**: MCPs, skills y reglas vienen con un "estandar" por default (lo que ya esta instalado en el workspace/repo) pero el usuario puede personalizar agregando/quitando desde la UI. Modelo: layer base (default detectado) + layer override (config local de michi). Aplica a:
    - MCPs: lista de MCP servers del workspace, toggle on/off por workspace/repo/job
    - Skills: skills detectadas en `.agents/skills/`, `.claude/skills/`, toggle por scope
    - Reglas: CLAUDE.md como base, posibles overrides per-job ("este job usa estas reglas extra")
    - Configuraciones de Docker, puertos, env vars: defaults del workspace, override per-job
9. **File tree del repo al expandir el caret (anotado 2026-05-18 07:xx)**: hoy el caret de un repo en la sidebar muestra solo los JOBS (worktrees activos). Camilo intuyo que mostraba el file tree del repo. Decision: el caret debe mostrar AMBOS — primero los jobs activos, despues una seccion "Files" con un mini file tree del repo (lazy load, con cap de profundidad y filtros de gitignore). UX detalle: solo expandir Files cuando hay 0 jobs O cuando el usuario hace click en una sub-seccion "Files". Pendiente decidir si Files es navegable hasta archivos individuales con preview o solo navegacion. Va a V1.

V2 (cuando V1 demuestre valor):

- **LLM local (gemma 3+)**: analisis offline del PTY output para detectar idle/pensando/needs-attention con LLM en vez de regex; resumen de actividad del job; sugerir nombres de branch desde la tarea inicial; pre-procesar specs.
- **MCP servers gestionados desde UI** (asignar por org/workspace/job).
- **Editor de Skills desde UI**.
- **Memoria compartida entre Claude Codes paralelos** (CRDT / file watcher).
- **Multi-pane split** (2+ terminales lado a lado).
- **Editor de codigo embebido** (dejar de depender de VSCode externo).
- **Modo "companero del dueno"**: asistente cross-job que ejecuta acciones globales ("commitea todo lo pendiente", "rebase todos contra main").

V3 (ambicioso):

- **Validacion + dogfood en Linux** (mac entra desde dia 1 por construccion cross-platform).
- **Publicacion en crates.io + brew + winget**: distribuir como producto.

## Future scope (V2/V3) — anotado para no perderlo

Ideas valiosas que NO entran al POC pero quedan documentadas:

### V2 — features de producto real

- **Gestión de MCP servers**: UI para listar MCP servers configurados por org/workspace, habilitar/deshabilitar por agente o job, ver logs de tool calls. Caso de uso: distintos jobs con distintos MCP enabled (ej: un job con MCP de Linear, otro con Notion).
- **Gestión de Skills**: UI para listar skills disponibles, crear/editar sin abrir archivos manualmente, habilitar por job, compartir skills entre proyectos.
- **Monitor de procesos/puertos del workspace**: ver qué dev servers están corriendo, en qué puertos, integrar con auto-asignación de puertos por job.
- **Auto-asignar puertos por job**: cada nuevo job recibe rango de puertos libre sin conflicto con otros worktrees.
- **Botón "Start dev"**: cada job tiene un comando dev configurado (ej: `pnpm dev`) y un botón para arrancarlo + verlo en pane secundario.
- **File tree del worktree**: navegador básico para abrir archivos en VSCode/editor externo.
- **Themes y settings UI**.

### V3 — features ambiciosas

- **Memoria compartida entre Claude Codes**: hoy cada claude code tiene `~/.claude/projects/<proj>/memory/`. Compartir memoria entre jobs paralelos del mismo proyecto sería potente. Requiere protocolo de sync (CRDT o file watcher) y manejo de conflictos cuando 2 claude codes editan la misma memoria.
- **Multi-pane split**: ver 2+ terminales lado a lado dentro de la misma ventana.
- **Editor de código embebido**: dejar de depender de VSCode externo.
- **Validación + dogfood en Linux**. (macOS no entra acá porque ya queda cross-platform por construcción desde el POC; cuando Camilo tenga mac, debería compilar y correr sin cambios de código.)
- **Modo "compañero del dueño"**: el panel se vuelve cliente del usuario con asistente que conoce todos los jobs y puede ejecutar comandos cross-job ("commitea todo lo pendiente", "muéstrame qué cambió hoy", "rebase todos contra main").

## Contexto Técnico

| Recurso | Ubicación |
|---------|-----------|
| Repo POC | `C:\Users\kmilo\Documents\projects\michi` (a crear) |
| Stack | Rust stable + egui + portable-pty + alacritty_terminal + git2 |
| Workspaces objetivo | `C:\Users\kmilo\Documents\projects\lelemon-workspace`, `...\venpu-workspace` |
| Worktrees creados en | `<workspace>-wt/<branch-slug>` paralelo al workspace base |
| Estado persistente | `%USERPROFILE%\.michi\state.json` |
| Logs | `%USERPROFILE%\.michi\logs\` |

## Alternativas Consideradas

### Opción A: TUI con ratatui + crossterm
- **Pros:** Más simple, menos dependencias, binario aún más pequeño
- **Cons:** Camilo pidió explícitamente GUI ligera. TUI se siente "DOS-like"

### Opción B: GUI con egui (elegida)
- **Pros:** Nativa Windows, single binary ~10MB, sin webview, immediate-mode simple, estética dev-tool (Rerun, Bevy editor). Cross-platform por construcción si algún día se exporta a Linux.
- **Cons:** Comunidad más chica que web stacks. Curva si no se conoce el modelo immediate-mode.

### Opción C: GUI con Tauri
- **Pros:** Frontend web (familiar), backend Rust
- **Cons:** Trae webview (Edge WebView2 en Windows). RAM más alta. Camilo ya rechazó Crystal (Electron) por sentirse pesado. Tauri es más ligero pero el feeling sigue siendo "web".

### Opción D: Setup manual con Windows Terminal + git worktree + script
- **Pros:** 1 hora, cero código Rust. Resuelve el dolor declarado.
- **Cons:** Camilo decidió que quiere construir Rust de todos modos (decisión 2026-05-17 override del postpone). Esta opción queda como fallback si el POC se atasca.

## Riesgos y Mitigaciones

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Embebido de PTY + ANSI rendering en egui es más complejo de lo esperado | Alta | Alto | Si en 3 días no hay terminal funcional, hacer fallback: lanzar Windows Terminal externo y solo gestionar jobs desde michi |
| Camilo se atasca en perfeccionar UI vs cerrar cliente #2 | Media | Alto | POC tiene fecha de corte: 1 semana. Si no funciona end-to-end al día 7, parar y volver a workflow manual. [feedback-no-scope-creep] |
| `alacritty_terminal` API inestable o difícil de integrar a egui | Media | Medio | Fallback: usar `vt100` crate (parser solo) + renderer custom simple monoespaciado |
| Curva de Rust si Camilo no es fluente | Media | Medio | POC pensado para que mucho sea código generado/asistido; aprender en el proceso. Si se vuelve cuello de botella, pausar |
| Worktrees de git en Windows tienen limitaciones (ej: case sensitivity, paths largos) | Baja | Medio | Activar `core.longpaths=true` global, validar paths antes de crear worktree |
| `unsafe` o memory issues bajen velocidad de iteración | Baja | Alto | Adherir a [RUST_GUIDELINES.md](RUST_GUIDELINES.md): cero unsafe en POC, anyhow para errores, no premature optimization |

## Recursos Relacionados

- [PHASES.md](PHASES.md) — plan por fases
- [UI_DESIGN.md](UI_DESIGN.md) — mockups ASCII de la UI
- [RUST_GUIDELINES.md](RUST_GUIDELINES.md) — best practices y antipatterns Rust a seguir
- [SESSION.md](SESSION.md) — contexto para retomar sesiones
- Memoria: [project_ide_agentico_rust_backlog.md](../../../../.claude/projects/C--Users-kmilo-Documents-projects-lelemon-workspace/memory/project_ide_agentico_rust_backlog.md)
