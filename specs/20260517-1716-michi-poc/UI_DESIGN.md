# UI Design — michi POC

> Mockups ASCII de la UI. La app es GUI nativa con egui, no TUI. Los mockups acá son para discutir layout y comportamiento, no para implementar literal.

---

## Principios

1. **Dark mode por default**, acento amarillo Lelemon (`#f7c948` aprox) en hover/seleccionado
2. **Sin emojis** en la UI — usar caracteres ASCII para indicadores de estado
3. **Densidad alta** — Camilo es power user, prefiere ver más info por pixel
4. **Una sola ventana**, sin tabs ni windows hijo (modales centrados sobre la app)
5. **Tipografía monoespaciada** (JetBrains Mono o Cascadia Code) en sidebar y terminal; sans-serif solo para chrome/headers

---

## Vista principal

```
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│  michi                          4 trabajos · 2 pensando            [ + Nuevo trabajo ]    │
├──────────────────────────────┬───────────────────────────────────────────────────────────────┤
│                              │  lelemon-app · feat/cors-fix                                  │
│  LELEMON-WORKSPACE     [ v ] │  ~/projects/lelemon-workspace-wt/cors-fix                     │
│                              │  3 archivos modificados            [ Diff ] [ Commit ] [ X ]  │
│  ▶ ● lelemon-app             ├───────────────────────────────────────────────────────────────┤
│    feat/cors-fix             │                                                               │
│    3 cambios · hace 2 min    │  > Voy a revisar el archivo main.ts para entender el CORS    │
│                              │    actual...                                                  │
│    ◐ lelemon-studio-web      │                                                               │
│    landing-v2                │  ● Read main.ts                                               │
│    sin cambios · pensando    │                                                               │
│                              │  La config actual permite '*' como origin. Voy a cambiarla   │
│                              │  para que use una whitelist desde env vars.                   │
│  VENPU-WORKSPACE       [ v ] │                                                               │
│                              │  ● Edit main.ts                                               │
│    ◐ venpu-backend           │                                                               │
│    fix/whatsapp-webhook      │  Listo. ¿Quieres que también actualice el .env.example?      │
│    1 cambio · pensando       │                                                               │
│                              │  > _                                                          │
│    ○ venpu-admin             │                                                               │
│    feat/dashboard-v2         │                                                               │
│    pausado · 1 día           │                                                               │
│                              │                                                               │
│                              │                                                               │
│                              │                                                               │
│  [ + ]                       │                                                               │
├──────────────────────────────┴───────────────────────────────────────────────────────────────┤
│  Ctrl+N nuevo · Ctrl+Tab siguiente · Ctrl+Shift+Tab anterior · Ctrl+W cerrar trabajo         │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Zonas:**
- Top bar (32px): título + contador + CTA principal
- Sidebar izquierdo (260-300px): lista de jobs agrupados por workspace
- Main pane: header del job seleccionado + terminal embebido
- Bottom bar (24px): hints de shortcuts

---

## Status dots

```
●  verde      idle, esperando tu input        — el caso default cuando vuelves a un job
◐  amarillo   claude pensando / ejecutando    — anima como spinner sutil
○  gris       pausado (sin proceso claude)    — proceso muerto, worktree existe
◆  rojo       error / claude crasheó          — click para ver log
▲  azul       requiere atención               — claude pidió permiso de tool o decisión
```

**Reglas de transición:**
- Job nuevo arranca como `◐` (pensando) mientras claude code bootea
- Cuando claude termina de procesar y espera input → `●`
- Si claude code muere → `○`, con badge "reiniciar" en hover
- Si hay panic/crash → `◆`
- Si claude muestra `[1] Yes  [2] No` o similar → `▲` (parser sobre el output)

---

## Card de job

### Estado normal

```
┌──────────────────────────────┐
│  ● lelemon-app               │
│    feat/cors-fix             │
│    3 cambios · hace 2 min    │
└──────────────────────────────┘
```

### Estado seleccionado

```
┌──────────────────────────────┐
│▌ ● lelemon-app               │   barra lateral amarilla 3px + bg ligeramente más claro
│▌   feat/cors-fix             │
│▌   3 cambios · hace 2 min    │
└──────────────────────────────┘
```

### Estado "necesita atención"

```
┌──────────────────────────────┐
│  ▲ venpu-backend             │   dot azul pulsante (animación ~1s)
│    fix/whatsapp-webhook      │
│    permiso pendiente         │   texto azul, no gris habitual
└──────────────────────────────┘
```

### Hover

```
┌──────────────────────────────┐
│  ● lelemon-app   [⋮]         │   menú kebab aparece a la derecha
│    feat/cors-fix             │
│    3 cambios · hace 2 min    │
└──────────────────────────────┘
```

### Datos en la card

| Campo | Origen | Refresh |
|-------|--------|---------|
| Status dot | Parser del PTY output del job | Cada output del PTY |
| Repo name | Config del job | Estático |
| Branch | Config del job | Estático (cambia solo si user renombra rama) |
| N cambios | `git status --porcelain` sobre el worktree | Cada 5 segundos en background |
| Last activity | Último timestamp de output del PTY | Cada output |

---

## Menú contextual (click derecho o kebab)

Sigue el design system rule de Camilo: click derecho abre menú con acciones disponibles.

```
┌─────────────────────────────┐
│  Ir al trabajo              │
│  Commit & push              │
│  Ver diff                   │
│  Abrir carpeta              │
│  ───────────────────────    │
│  Reiniciar claude           │
│  Pausar                     │
│  ───────────────────────    │
│  Cerrar trabajo             │
└─────────────────────────────┘
```

**Comportamientos:**
- "Ir al trabajo": selecciona el job (equivalente a click)
- "Commit & push": abre modal de commit (ver abajo)
- "Ver diff": overlay con `git diff` coloreado del worktree
- "Abrir carpeta": `explorer.exe <worktree-path>`
- "Reiniciar claude": kill + relaunch del proceso claude
- "Pausar": kill claude, worktree queda
- "Cerrar trabajo": confirma + `git worktree remove` + olvida el job

---

## Modal "Nuevo trabajo"

```
┌─────────────────────────────────────────────────────────┐
│  Nuevo trabajo                                      [X] │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  Workspace                                              │
│  ┌───────────────────────────────────────────────────┐ │
│  │ lelemon-workspace                          [ v ]  │ │
│  └───────────────────────────────────────────────────┘ │
│                                                         │
│  Repo                                                   │
│  ┌───────────────────────────────────────────────────┐ │
│  │ lelemon-app                                [ v ]  │ │
│  └───────────────────────────────────────────────────┘ │
│                                                         │
│  Rama nueva                                             │
│  ┌───────────────────────────────────────────────────┐ │
│  │ feat/                                             │ │
│  └───────────────────────────────────────────────────┘ │
│  Se crea desde: main                          [ cambiar]│
│                                                         │
│  Tarea inicial (opcional)                               │
│  ┌───────────────────────────────────────────────────┐ │
│  │ Arregla el CORS para que use whitelist en vez    │ │
│  │ de "*"                                            │ │
│  │                                                   │ │
│  └───────────────────────────────────────────────────┘ │
│  Esto se envía como primer prompt a claude              │
│                                                         │
│                              [ Cancelar ]  [ Crear ]    │
└─────────────────────────────────────────────────────────┘
```

**Validaciones antes de habilitar "Crear":**
- Workspace seleccionado y existe en disco
- Repo seleccionado y existe en `<workspace>/<repo>`
- Rama nueva: matches `^[a-z0-9/_-]+$`, no existe en el repo
- Base branch existe en el repo

**Al hacer "Crear":**
1. `git fetch` en el repo base
2. `git worktree add <workspace>-wt/<branch-slug> -b <branch> <base-branch>` desde el repo
3. Lanzar `claude` (subproceso vía portable-pty) en ese worktree
4. Si hay tarea inicial: inyectar como primer prompt vía stdin del PTY
5. Persistir el job en `state.json`
6. Seleccionar el job nuevo en la sidebar
7. Cerrar modal

**Errores:**
- Worktree ya existe → mostrar error inline "Esa rama ya tiene worktree, elige otro nombre"
- Branch ya existe → "Esa rama ya existe en el repo, elige otro nombre"
- Falla `git fetch` → permitir continuar offline con warning

---

## Header del terminal pane

```
┌──────────────────────────────────────────────────────────────────────────────┐
│  lelemon-app · feat/cors-fix                                                 │
│  ~/projects/lelemon-workspace-wt/cors-fix                                    │
│  3 archivos modificados                  [ Diff ] [ Commit & Push ] [ X ]    │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Botones:**

### [ Diff ]

Overlay sobre el terminal mostrando `git diff` coloreado. Cierra con Esc o click fuera.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│  Diff · lelemon-app · feat/cors-fix                                     [X] │
├──────────────────────────────────────────────────────────────────────────────┤
│  apps/api/src/main.ts                                                        │
│                                                                              │
│  - app.enableCors({ origin: '*' });                                          │
│  + app.enableCors({                                                          │
│  +   origin: process.env.CORS_ORIGINS?.split(',') ?? [],                     │
│  + });                                                                       │
│                                                                              │
│  apps/api/.env.example                                                       │
│  + CORS_ORIGINS=http://localhost:3500,https://lelemon.cl                     │
│                                                                              │
│  apps/api/test/cors.spec.ts                                                  │
│  + ... (12 líneas)                                                           │
└──────────────────────────────────────────────────────────────────────────────┘
```

### [ Commit & Push ]

Modal pequeño:

```
┌─────────────────────────────────────────────────────────┐
│  Commit & Push                                      [X] │
├─────────────────────────────────────────────────────────┤
│  3 archivos · feat/cors-fix → origin                    │
│                                                         │
│  Mensaje                                                │
│  ┌───────────────────────────────────────────────────┐ │
│  │ fix(cors): use env whitelist instead of '*'      │ │
│  └───────────────────────────────────────────────────┘ │
│                                                         │
│  [x] Push después del commit                            │
│  [ ] Stage all (incluye archivos nuevos)                │
│                                                         │
│                              [ Cancelar ]  [ Commit ]   │
└─────────────────────────────────────────────────────────┘
```

### [ X ]

Confirmación si hay cambios sin commit:

```
┌─────────────────────────────────────────────────────────┐
│  Cerrar trabajo                                     [X] │
├─────────────────────────────────────────────────────────┤
│  Tienes 3 cambios sin commitear en feat/cors-fix.       │
│  Si cierras el trabajo, el worktree se elimina pero     │
│  los cambios quedan en la rama feat/cors-fix.           │
│                                                         │
│  [ Commit primero ]  [ Descartar todo ]  [ Cancelar ]   │
└─────────────────────────────────────────────────────────┘
```

---

## Terminal embebido

- Renderer: `alacritty_terminal` (parser + grid) + custom egui widget que pinta el grid
- Fuente: JetBrains Mono 13px (fallback: Cascadia Code, Consolas)
- Scrollback: 10000 líneas por job, en memoria
- Selección: click + drag, Ctrl+Shift+C copia
- Paste: Ctrl+Shift+V
- Resize: cuando cambia el tamaño del pane, llamar `pty.resize(rows, cols)`
- Colores: paleta tipo solarized-dark o tokyo-night, no decidido todavía

**Comportamientos especiales:**
- Cuando el output del PTY menciona "permission" / "[1] Yes [2] No" / patrones similares → marcar job como `▲` (necesita atención)
- Cuando llega un newline después de >2s de silencio → job pasa a `●` (idle)
- Mientras hay output activo → job está `◐` (pensando)

---

## Shortcuts globales

```
Ctrl+N             Nuevo trabajo (abre modal)
Ctrl+Tab           Siguiente trabajo en la lista
Ctrl+Shift+Tab     Trabajo anterior
Ctrl+1..9          Saltar al trabajo N (orden de la sidebar)
Ctrl+W             Cerrar trabajo actual (con confirmación si hay cambios)
Ctrl+,             Settings (V2, no implementado en POC)
F2                 Renombrar rama del trabajo actual
F5                 Refresh status de todos los jobs
```

**Dentro del terminal:**
```
Ctrl+Shift+C       Copiar selección
Ctrl+Shift+V       Pegar
Ctrl+L             Limpiar pantalla (clear)
```

---

## Estados que NO van en el POC

- Settings UI / themes
- Multi-pane split (ver 2 terminales a la vez)
- File tree del worktree
- Editor de código embebido
- Buscador global de jobs
- Notificaciones de sistema cuando un job pasa a `▲`
- Drag & drop para reordenar jobs
- Tags/labels personalizados por job
- Métricas (tiempo total, tokens consumidos, etc.)

Todo esto es V2 si el POC demuestra valor.
