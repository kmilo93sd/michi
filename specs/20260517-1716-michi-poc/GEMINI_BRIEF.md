# Brief para Gemini — michi: harness multi-agente con sandbox container-first

> **Para qué es este doc:** dártelo a Gemini con TODO el contexto para que nos
> ayude con su visión sobre la arquitectura de contenedores de michi. Es
> autocontenido: no necesitás acceso al repo. Al final hay preguntas concretas.

---

## 1. Qué es michi (en una línea)

Un **harness multi-agente** de escritorio (app nativa en Rust + egui) para correr
muchas sesiones de **Claude Code** en paralelo en una misma máquina sin que se
pisen: observa cada sesión, muestra qué hace y qué recursos consume, y **aísla**
esos recursos (git, puertos, base de datos, contenedores) por sesión.

No es un IDE ni un orquestador cloud. michi no edita código — Claude lo hace.
michi es la "torre de control de tráfico". Corre local, un solo dueño, alpha.

## 2. El problema real

El dueño (Camilo) trabaja con varias sesiones largas de Claude Code en paralelo
sobre un workspace. Se chocan: mismo puerto 8080, stacks de Docker duplicados,
estado de git pisado, no sabe qué sesión chupa RAM ni qué puerto quedó tomado.
Las herramientas existentes (Conductor, Crystal, Claude Squad) o no son Windows o
tienen UX rechazada.

## 3. Estado y stack

- **Stack:** Rust estable + egui/eframe (immediate-mode GUI, sin Electron, sin
  webview), tokio, `egui_term` + `portable-pty` (terminal embebido por sesión),
  sysinfo (árbol de procesos/RAM), anyhow, serde.
- **Plataforma:** Windows-first, **cross-platform por construcción** (mac/Linux).
  Regla dura del proyecto: nada OS-specific sin fallback.
- **Disciplina:** TDD estricto (red→green), `cargo fmt`/`clippy -D warnings`/`test`
  como gates de CI en Linux/macOS/Windows. Cero `unwrap`/`unsafe` fuera de tests.
- **Ya construido (harness):** detecta TODAS las sesiones `claude` del host
  (managed + externas), árbol de procesos y RAM por sesión, estado real
  (busy/idle/waiting) leído de `~/.claude/sessions/<pid>.json`, asignación de
  puertos por sesión (lee `PORT_*` de `.env`).

**Dos tipos de sesión:**
- **Managed:** la lanzó michi → tiene PTY embebido, control total (inyectar
  prompts, asignar puertos, aislar DB, sandboxear en contenedor).
- **Detectada:** corre fuera de michi (tu terminal, VS Code) → michi la VE
  escaneando el host, read-only, hasta que la "traés".

## 4. La decisión: dirección container-first

michi adopta el **sandbox por agente** como rumbo de V1, pero aterrizado a su
realidad (escritorio, Windows-first, mono-usuario), NO como orquestador cloud.
**Contenedor PREFERIDO, no requerido** (regla cross-platform): si no hay Docker,
cae a nativo. Tres planos:

1. **Observación (Nivel 0, universal, sin Docker):** escanear host, procesos, RAM,
   puertos LISTEN. Funciona siempre.
2. **Aislamiento (contenedor preferido, fallback nativo):** las sesiones que michi
   lanza van en un contenedor; sin Docker → nativo (worktree + PTY directo).
3. **Infra compartida:** 1 postgres/redis + DB/schema efímero por sesión.

### Lo que DESCARTAMOS del modelo cloud genérico (y por qué) — no lo re-propongas

Una propuesta previa sugería un orquestador cloud (Firecracker, cgroups, Btrfs/ZFS
CoW, Traefik, NATS). Lo evaluamos contra la realidad de michi y descartamos:
- **Firecracker / micro-VMs:** KVM = solo Linux. Rompe cross-platform.
- **cgroups directo:** Linux-only. Análogo cross-platform = límites de Docker
  (`--memory`/`--cpus`) y `sysinfo`/Job Objects, que ya usamos.
- **Btrfs/ZFS CoW:** no portable. Los **git worktrees ya son el CoW pragmático**
  (comparten `.git`, no reclonás).
- **NATS:** overkill mono-máquina; `mpsc` + `tracing` alcanzan.

## 5. Spike manual hecho (Windows + Docker Desktop 27.3.1) — TODO VERDE

Validamos a mano, antes de codear, los riesgos grandes:

- **Auth portable:** `claude` se instala en un contenedor Linux y **autentica
  montando solo `~/.claude/.credentials.json`** (token OAuth) en un home efímero.
  El token de Windows funciona en contenedor Linux. (Era el riesgo #1.)
- **Edit round-trip:** claude editó un archivo montado y el cambio apareció en el
  host.
- **DB isolation:** 1 postgres compartido en una red docker + `CREATE DATABASE
  session_<id>` + `DATABASE_URL` inyectado por sesión → aislamiento real, **1
  postgres = 42 MiB para N sesiones** (vs ~42 MiB *cada uno* si fueran separados).
- **Cap de recursos:** `--memory 256m` + alocar sin freno → OOM-kill (exit 137);
  `docker stats` muestra los límites. "Matar/limitar agente loco" sale gratis.
- **Puertos:** dos contenedores con su `:8080` interno → `-p 4100:8080` y
  `-p 4200:8080` → cada host port pega al contenedor correcto. **URL estable por
  sesión SIN reverse proxy.**
- **Pared #1 (perf):** escribir 2000 archivos chicos → **bind mount Win 5.05s vs
  named volume 0.42s vs FS interno 1.35s.** Bind mount Win→contenedor ~12x más
  lento. ⇒ build artifacts (`target/`, `node_modules`) van en **named volume** o en
  el FS de WSL2, nunca en bind mount de path Windows.

## 6. La arquitectura que proponemos: modelo de 3 capas sobre el estándar devcontainer

**Estándar adoptado: dev containers (https://containers.dev).** Spec abierta
(Microsoft) usada por VS Code, Codespaces, Gitpod. El repo declara su entorno en
`devcontainer.json` (imagen / Dockerfile / compose + "features"). Si el repo no lo
tiene → **imagen universal de fallback** (modelo Codespaces).

```
Capa INFRA (michi)     → puertos publicados, DB efímera por sesión, volúmenes de caché
Capa AGENTE (michi)    → claude + git, inyectados SIEMPRE (instalador standalone, sin node)
Capa RUNTIME (el repo) → devcontainer.json / Dockerfile, o base universal michi
```

Idea clave: **el repo es la fuente de verdad del runtime; michi le agrega la capa
de agente encima.** Así no enumeramos lenguajes — el repo (o la base universal)
trae el runtime y michi solo garantiza `claude`.

**Activación (default inteligente + override 1 click):**
- Repo con `devcontainer.json` + Docker → contenedor (alta confianza).
- Repo sin devcontainer + Docker → contenedor con base universal (badge visible).
- Sin Docker / "usar nativo" → nativo.

**Caches** en named volumes (cargo/pnpm/npm) para que se sienta rápido.
**Bonus:** michi puede generar un `devcontainer.json` para repos que no lo tienen.

**Camino incremental:** Cut-1 (base universal + capa claude + caches + toggle) →
Cut-2 (leer devcontainer.json) → Cut-3 (scaffolding del devcontainer.json).

## 7. Cómo se lanza una sesión (mecánica concreta)

El terminal embebido (`egui_term`) spawnea un comando en un PTY. Hoy es
`claude` directo (nativo). En modo contenedor el PTY spawneará
`docker run -it --rm --name michi-<id> -v <worktree>:/work -v <creds>:/root/.claude/.credentials.json:ro -p <host>:<cont> -e DATABASE_URL=... --memory 4g --cpus 2 -w /work <imagen> claude "<tarea>"`.

Ya está codeado y testeado (TDD, mergeado a main):
- `detect_docker() -> DockerStatus` (Available/Unavailable con fallback).
- `build_run_args(spec) -> Vec<String>` (arma el `docker run`, función pura).
- `plan_launch(docker, spec) -> LaunchPlan` (decide contenedor vs nativo y produce
  command/args/env/cwd).

Falta el **wiring** en la UI (Cut-1) y el lifecycle de contenedores.

## 8. Restricciones / no-goals (para que tu visión sea aplicable)

- Local, escritorio, **un solo usuario**. No cloud, no multi-tenant server, no
  seguridad contra agentes hostiles (los agentes son del dueño, descoordinados, no
  maliciosos). El objetivo es **coordinar recursos**, no una cárcel de seguridad.
- **Cross-platform obligatorio**, Windows-first. Nada Linux-only sin fallback.
- App nativa liviana (no inflar el feeling: el camino nativo sigue siendo el
  default liviano; el contenedor es preferido-no-requerido).
- Terminología real (harness, agent orchestration), sin etiquetas marketineras.

---

## 9. Lo que te pedimos, Gemini (tu visión)

1. **Critica el modelo de 3 capas** (runtime del repo / agente michi / infra
   michi). ¿Es la abstracción correcta? ¿Dónde se rompe?
2. **Capa AGENTE:** ¿layerear `claude`+git con instalador standalone sobre la
   imagen del repo es lo mejor? ¿Conviene construir una **imagen derivada** cacheada
   (`base + agente`) por repo, o instalar al arrancar? ¿Cómo no romper "create en
   <5s"? ¿Te parece bien una "feature" de devcontainer propia para el agente?
3. **Imagen / runtime (DX):** ¿pararse en devcontainer.json + base universal de
   fallback es el mejor balance estándar-vs-esfuerzo? ¿Qué base universal
   recomendarías (algo tipo la `universal` de Codespaces, o más liviano)? ¿Vale la
   detección por lenguaje (Cargo.toml/package.json) como atajo?
4. **Activación / UX:** ¿default inteligente + override es lo correcto, o conviene
   ser más agresivo (siempre contenedor) o más conservador (opt-in)? ¿Cómo lo
   mostrarías en la UI sin fricción?
5. **DB isolation:** validamos schema/DB efímera por sesión sobre 1 postgres. ¿Cómo
   manejarías migraciones (¿las corre michi, el dev server, o Claude?), el
   `redis`/otros servicios, y el cleanup robusto si michi crashea (DBs huérfanas)?
6. **Performance en Windows (pared #1):** dado que el bind mount es ~12x más lento,
   ¿mover el worktree a WSL2, usar named volumes con sync, o qué estrategia? ¿Cómo
   afecta esto al hot-reload de dev servers?
7. **Qué nos estamos perdiendo:** riesgos, una mejor arquitectura que no vimos, o un
   orden de implementación distinto. Sé concreto y aterrizado a las restricciones
   del punto 8 (no propongas Firecracker/cgroups/Btrfs/NATS — ya los descartamos y
   por qué).

Gracias. Buscamos profundidad técnica y opinión fuerte, no validación.
