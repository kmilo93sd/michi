# Rust Guidelines — michi POC

> Best practices y antipatterns que aplican al POC. Consultar antes de tomar decisiones de diseño y al hacer code review.

---

## Filosofía general

1. **Optimizar para iteración rápida, no para performance**. Es POC. Si funciona y compila, vale.
2. **Cero `unsafe`** en este POC. Sin excepciones.
3. **Cero `unwrap()` / `expect()`** en código que corre en producción del POC. Solo se permite en tests y en `main()` del bootstrap inicial.
4. **`anyhow::Result`** para errores en toda la app. No definir enums de error custom hasta que haya razón clara (V2).
5. **`cargo fmt` y `cargo clippy -- -D warnings`** antes de cada commit. Sin excepciones.
6. **Código siempre cross-platform.** El dogfood inicial es Windows pero el código corre también en mac/Linux. Esto NO es opcional ni un "nice to have" — es regla.

---

## Best practices

### Manejo de errores

- Usar `anyhow::Result<T>` como tipo de retorno en funciones fallibles
- Usar operador `?` para propagar errores
- Adjuntar contexto con `.context("descripción")` o `.with_context(|| ...)` antes de propagar
- Para condiciones imposibles que el compilador no puede inferir: `anyhow::bail!("razón")` en vez de `panic!`
- Errores que el usuario debe ver llegan a la UI vía un `tokio::sync::mpsc` y se muestran como toast/dialog, nunca crashean la app

```rust
// BIEN
fn load_state(path: &Path) -> anyhow::Result<State> {
    let bytes = fs::read(path)
        .with_context(|| format!("leyendo state desde {}", path.display()))?;
    let state: State = serde_json::from_slice(&bytes)
        .context("parseando state.json")?;
    Ok(state)
}

// MAL
fn load_state(path: &Path) -> State {
    let bytes = fs::read(path).unwrap();           // crashea si el archivo no existe
    serde_json::from_slice(&bytes).unwrap()        // crashea si el JSON es inválido
}
```

### Ownership y borrows

- En signatures, preferir `&str` sobre `String`, `&[T]` sobre `Vec<T>`, `&Path` sobre `PathBuf`, salvo que la función tome ownership
- Cloning está OK cuando simplifica el diseño en POC, pero documentar con un `// clone para evitar lifetime acrobatics` si es no-obvio
- Para state compartido entre hilos: `Arc<Mutex<T>>` (o `Arc<RwLock<T>>` si hay muchos lectores)
- Para state NO compartido: dejarlo en la struct directo, sin `Arc`

```rust
// BIEN
fn validate_branch_name(name: &str) -> anyhow::Result<()> { ... }

// MAL (toma ownership innecesario)
fn validate_branch_name(name: String) -> anyhow::Result<()> { ... }
```

### Organización de código

- Estilo de módulos Rust 2018+: archivos como módulos. NO usar `mod.rs` salvo cuando un módulo tiene submódulos importantes
- Empezar todo `private`. Marcar `pub` solo lo que cruza el borde del módulo
- `pub(crate)` para cosas que solo se usan en el binario, no exportadas
- Un archivo = un concepto. Si un archivo crece >300 líneas, evaluar partirlo

```
src/
├── main.rs              -- bootstrap, configura logging, lanza app
├── app.rs               -- struct App (state global egui)
├── state/
│   ├── mod.rs           -- re-exporta lo público
│   ├── job.rs           -- struct Job, JobStatus
│   └── persistence.rs   -- load/save state.json
├── git/
│   ├── mod.rs
│   ├── worktree.rs      -- crear/remover worktrees
│   └── status.rs        -- git status, diff, commit
├── terminal/
│   ├── mod.rs
│   ├── pty.rs           -- portable-pty integration
│   └── renderer.rs      -- alacritty_terminal → egui
└── ui/
    ├── mod.rs
    ├── sidebar.rs       -- lista de jobs
    ├── terminal_pane.rs -- terminal embebido
    └── new_job_modal.rs -- modal de crear trabajo
```

### Async y concurrencia

- Usar `tokio` con runtime `current_thread` (no `multi_thread`) en POC — más simple, suficiente
- PTY reads van en threads separados (no tokio tasks, son blocking) y mandan output por `tokio::sync::mpsc` al thread UI
- Git ops son blocking; ejecutar en `tokio::task::spawn_blocking` o thread dedicado
- NO bloquear el thread de UI por ningún motivo (egui requiere 60fps idealmente)
- NO holdear un `MutexGuard` a través de `.await` (deadlock)

```rust
// BIEN
let output = tokio::task::spawn_blocking(move || {
    Command::new("git").args(["worktree", "add", ...]).output()
}).await??;

// MAL (bloquea el runtime)
let output = Command::new("git").args(["worktree", "add", ...]).output()?;
```

### egui specifics

- State vive en una struct `App` que implementa `eframe::App`
- En `update(&mut self, ctx, frame)`: SOLO leer state y dibujar. NO hacer I/O, NO esperar.
- Para acciones async (crear worktree, lanzar claude code): enviar comando a un thread worker vía channel
- Usar `ctx.request_repaint()` cuando llega update desde un thread externo, NO en cada frame
- `egui::TextureHandle` para cachear imágenes/iconos — NUNCA recargar en cada frame
- Reusar `String` buffers para inputs (no `String::new()` en cada draw)

```rust
// BIEN
impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        // drena eventos pendientes sin bloquear
        while let Ok(event) = self.events_rx.try_recv() {
            self.handle_event(event);
        }
        // dibuja según state
        egui::SidePanel::left("sidebar").show(ctx, |ui| { ... });
    }
}

// MAL
fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
    let jobs = load_state_from_disk().unwrap();  // I/O en cada frame!
    ...
}
```

### Logging

- Usar `tracing` crate (no `log`/`env_logger`)
- `tracing_subscriber` con file appender en `~/.michi/logs/michi.log`
- Niveles: `trace` (dev only), `debug` (verbose dev), `info` (eventos significativos), `warn`, `error`
- En POC: log a archivo + stderr en modo debug
- NO loggear secretos, paths completos de usuario en producción, ni contenido de stdout/stderr del claude code

### Dependencies

- Mínimo set inicial:
  - `egui` + `eframe` (UI)
  - `tokio` (async runtime, feature `rt`, `macros`, `sync`, `time`, `process`)
  - `anyhow` (errores)
  - `serde` + `serde_json` (state persistence)
  - `tracing` + `tracing-subscriber` + `tracing-appender` (logs)
  - `portable-pty` (PTY cross-platform)
  - `alacritty_terminal` (ANSI parser + grid)
  - `git2` (git ops programáticas) O shell-out a `git` con `tokio::process::Command` (más simple para empezar)
  - `dirs` (home dir cross-platform)
  - `clap` (CLI flags, V2 — POC no necesita)
- ANTES de agregar una dependency nueva: justificar en commit message. ¿Vale el peso?

### Tests

- POC mínimo: tests unitarios para lógica pura (parseo de branch names, paths, etc.)
- NO escribir tests de integración con UI hasta V2
- Cuando se escriba un test: `#[cfg(test)] mod tests { ... }` al final del archivo

---

## Antipatterns a evitar

### `unwrap()` y `expect()` en hot paths

- Cualquier llamada a una API externa (fs, git, network, parsing) puede fallar
- `unwrap()` convierte un fallo recuperable en crash
- Solo OK en: tests, en `main()` para el bootstrap inicial (config dir creation), y constants conocidos en compile time

### Cloning defensivo en exceso

```rust
// MAL
fn show_job(job: Job) -> String {        // toma ownership innecesario
    let name = job.name.clone();         // clone que no se necesita
    let branch = job.branch.clone();
    format!("{} · {}", name, branch)
}

// BIEN
fn show_job(job: &Job) -> String {
    format!("{} · {}", job.name, job.branch)
}
```

### `Arc<Mutex<T>>` everywhere

- Solo usar `Arc<Mutex<T>>` cuando hay state que SE comparte entre threads/tasks
- State local a la struct App de egui: NO necesita Arc/Mutex (egui da `&mut self`)
- "Por si acaso" no es justificación válida

### Custom Error enums prematuros

- `thiserror` es bueno para crates publicados
- Para POC de app: `anyhow::Error` cubre todo, con `.context()` para info
- Si algún día sale a producción y un módulo necesita errores tipados, ahí se hace la refactor

### Bloquear el UI thread

- I/O en `update()` → frame drops
- `tokio::block_on()` en main → deadlock
- Loop infinito en update → UI congela
- Solución universal: thread worker + channel

### Held `MutexGuard` across `.await`

```rust
// MAL - deadlock garantizado
let guard = state.lock().await;
do_async_thing().await;     // guard sigue held durante await

// BIEN
let snapshot = {
    let guard = state.lock().await;
    guard.clone()           // o extraer solo lo necesario
};                          // guard dropped acá
do_async_thing(snapshot).await;
```

### Macros custom prematuros

- Las macros son potentes pero ilegibles
- En POC, función > macro siempre que se pueda
- Solo usar `macro_rules!` si hay 5+ usos del patrón y no se resuelve con genéricos

### `dyn Trait` por default

```rust
// MAL en hot paths
fn process(items: &[Box<dyn Component>]) { ... }

// BIEN cuando el conjunto es conocido en compile time
enum Component { Sidebar(Sidebar), Terminal(Terminal), Modal(Modal) }
fn process(items: &[Component]) { ... }
```

- `dyn Trait` está OK para plugins reales o heterogéneos. NO como default.

### Premature optimization

- `#[inline]`, `unsafe`, `unsafe_impl`, custom allocators → ninguno en POC
- Si hay un perf problem, profileearlo primero (`cargo flamegraph`)

### Shell-out cuando hay crate decente, y viceversa

- `git`: válido shell-out con `Command::new("git")` para POC (simple, mismo binario que el usuario)
- `git2`: usar solo si necesitamos algo programático que `git` CLI no facilita
- DECIDIR entre los dos al inicio y ser consistente, no mezclar

### Paths con strings hardcoded

```rust
// MAL
let state_path = format!("{}/.michi/state.json", env::var("HOME").unwrap());

// BIEN
let state_path = dirs::home_dir()
    .context("HOME no disponible")?
    .join(".michi")
    .join("state.json");
```

### Asumir Windows (rompe cross-platform)

```rust
// MAL
let claude_bin = "C:\\Users\\kmilo\\AppData\\Local\\nvm\\...\\claude.cmd";
Command::new("wt").args(["-w", "0", ...]);   // wt solo existe en Windows
Command::new("cmd.exe").arg("/c").arg(...);  // cmd.exe solo Windows

// BIEN
Command::new("claude")                       // confiar en PATH, mismo binario en todas las plataformas
    .current_dir(&worktree)
    .spawn()?;

// Si necesitas un fallback per-OS, usa cfg!:
#[cfg(target_os = "windows")]
let term = "wt";
#[cfg(target_os = "macos")]
let term = "open";
#[cfg(target_os = "linux")]
let term = std::env::var("TERMINAL").unwrap_or_else(|_| "xterm".into());
```

- Para diferencias por OS usar `#[cfg(target_os = "...")]` o el crate `cfg-if`
- NUNCA hardcodear `\` en separadores de path — `PathBuf::join` los maneja
- Para shell commands, preferir el binario nativo (`claude`, `git`) que es el mismo en todas las plataformas

### Configurar `panic = "abort"` en POC

- Default `panic = "unwind"` permite tests y mejores stack traces
- `panic = "abort"` solo cuando hay razón explícita (smaller binary final, embedded)

---

## Checklist pre-commit

- [ ] `cargo fmt` aplicado
- [ ] `cargo clippy -- -D warnings` pasa sin warnings
- [ ] `cargo test` pasa
- [ ] `cargo build --release` compila
- [ ] No hay `unwrap()` / `expect()` nuevos en código no-test
- [ ] No hay `dbg!` / `println!` de debug olvidados
- [ ] Funciones con I/O o lentas están fuera del thread UI
- [ ] Cualquier dependency nueva está justificada en el commit message

---

## Recursos

- [The Rust Programming Language](https://doc.rust-lang.org/book/) — referencia base
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) — naming, conventions
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial) — async patterns
- [egui examples](https://github.com/emilk/egui/tree/master/examples) — patrones immediate-mode
- [alacritty_terminal docs](https://docs.rs/alacritty_terminal/) — terminal emulation
- [clippy lints](https://rust-lang.github.io/rust-clippy/master/) — qué arregla cada warning
