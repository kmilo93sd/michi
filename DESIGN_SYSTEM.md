# Design System — michi

> Reglas, patterns y gotchas para mantener una UI coherente.
> Si tocás UI, leé esto antes.

## Filosofía

- **Dark mode por defecto.** El target es power user dev. Light mode no se construye hasta V2.
- **Densidad alta, pero respirando.** Ni VS Code apretado ni Apple Mail aireado.
- **Tokens en un solo archivo.** Todos los colores, fuentes y spacings viven en `src/theme.rs`.
- **El usuario edita `~/.michi/theme.toml`**, no toca código.
- **Power user > visual fancy.** Cero animaciones gratuitas, cero gradientes, cero glassmorphism.

## Tokens

Tokens canónicos en `src/theme.rs` (struct `Theme`). Cada uno se persiste a
`~/.michi/theme.toml` como string hex (`"#0f0f11"`) o número (`13.0`).

### Colores

| Token | Default | Uso |
|---|---|---|
| `accent` | `#f7c948` | Lelemon yellow. Barras de selección, focus borders, hyperlinks. |
| `bg_base` | `#0f0f11` | Fondo del área central (terminal pane, empty state). Más oscuro que `bg_surface`. |
| `bg_surface` | `#16161a` | Chrome de la app (sidebar, bottom bar, modales, header del job). |
| `bg_card_selected` | `#26262c` | Fondo de la card del job seleccionado en sidebar. **Más claro** que `bg_base` (se levanta, no se hunde). |
| `bg_card_hover` | `#1c1c21` | Fondo de cualquier widget clickeable en hover. |
| `border` | `#28282f` | Bordes sutiles (separadores, stroke de inputs en reposo). |
| `text_primary` | `#e4e4e8` | Texto general. **Alto contraste sobre `bg_base`.** |
| `text_muted` | `#8c8c9c` | Subtítulos, hints, metadata secundaria. |
| `text_workspace_label` | `#aaaabc` | UPPERCASE de los workspace headers en sidebar. |
| `text_repo_label` | `#d2d2de` | Nombres de repos en sidebar. |
| `status_idle` | `#30d158` | Dot verde (Apple green). |
| `status_thinking` | `#f7c948` | Dot amarillo (= accent). |
| `status_paused` | `#787884` | Dot gris medio. |
| `status_error` | `#ff453a` | Dot rojo. También para mensajes de error inline. |
| `status_needs_attention` | `#0a84ff` | Dot azul (iOS blue). |

### Tipografía y sizes

| Token | Default | Uso |
|---|---|---|
| `font_mono_size` | `13.0` | Fuente monoespaciada del sidebar y terminal. |
| `sidebar_min_width` | `260.0` | |
| `sidebar_max_width` | `380.0` | |
| `sidebar_default_width` | `300.0` | |
| `bottom_bar_height` | `26.0` | |
| `job_header_height` | `76.0` | |
| `card_row_height` | `56.0` | Altura de cada card de job en sidebar. |
| `workspace_header_height` | `42.0` | Header colapsable de workspace (2 líneas: uppercase + sublabel). |
| `repo_header_height` | `26.0` | Header colapsable de repo (1 línea: chevron + nombre + count). |
| `tree_line_ws_x` | `8.0` | Offset X de la tree line de workspace. |
| `tree_line_repo_x` | `24.0` | Offset X de la tree line de repo. |

## Patterns

### Aplicar el theme a egui

`Theme::build_visuals()` construye un `egui::Visuals` con los tokens. Se
aplica en dos lugares:

1. **`App::new` (boot):** `cc.egui_ctx.set_visuals(theme.build_visuals())`.
   Sienta el baseline para todo el árbol.
2. **`fn ui` cada frame:** `ctx.set_visuals(...)` otra vez. **Sí, cada frame.**
   Razón: egui crea popups (ComboBox dropdown, tooltips, menus) como `Area`s
   independientes que NO heredan los `Visuals` locales del `Ui` padre. Para
   que esos popups respeten dark mode hay que sincronizar el Context cada
   frame. Es una struct copy, costo despreciable.
3. **Dentro de cada `egui::Modal::show()`:** `ui.style_mut().visuals = theme.build_visuals()`.
   Defensa adicional: el modal frame puede tener un style heredado de otra
   Area, así forzamos.

**No es opcional.** Si saltás uno, vas a ver inputs blancos o dropdowns en
light theme contra todo lo demás dark.

### Modal

```rust
let frame = egui::Frame::new()
    .fill(theme.bg_surface)
    .inner_margin(egui::Margin::same(20))
    .corner_radius(egui::CornerRadius::same(8))
    .stroke(egui::Stroke::new(1.0, theme.border));

let resp = egui::Modal::new(egui::Id::new("my_modal"))
    .frame(frame)
    .show(ctx, |ui| {
        ui.style_mut().visuals = theme.build_visuals();
        ui.spacing_mut().interact_size = egui::vec2(0.0, 36.0);
        ui.spacing_mut().button_padding = egui::vec2(14.0, 10.0);
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
        ui.set_min_width(480.0);
        ui.set_max_width(620.0);
        // ... contenido ...
    });
```

Comportamiento estándar: cierra con `Esc`, click en backdrop, o botón X.
Si el modal está en estado "ocupado" (creating worktree, etc), ignorar
todas esas vías de cierre.

### Inputs

```rust
ui.label(egui::RichText::new("Workspace").strong().color(theme.text_primary));
ui.add_space(4.0);
ui.add(
    egui::TextEdit::singleline(&mut state.branch)
        .desired_width(f32::INFINITY)
        .margin(egui::Margin::symmetric(10, 8))
        .hint_text("feat/cors-fix"),
);
```

- Label encima del input (no inline a la izquierda).
- Label en `strong()` con `text_primary` (no `small().weak()` — son inputs
  importantes, no metadata).
- `margin symmetric(10, 8)` para padding interno cómodo.
- `desired_width(f32::INFINITY)` para llenar el ancho del contenedor.

### Botones primarios

Botón grande full-width cuando es la acción central de un container
(sidebar `+ Nuevo trabajo`, empty state `+ Crear primer trabajo`):

```rust
ui.add_sized([ui.available_width(), 32.0], egui::Button::new("+ Nuevo trabajo"))
    .on_hover_cursor(egui::CursorIcon::PointingHand)
```

Botón inline secundario (dentro de un row con texto):

```rust
ui.button("Cancelar")
    .on_hover_cursor(egui::CursorIcon::PointingHand)
```

### Cursor pointer en clickeables

**Cualquier widget interactivo** debe llamar `.on_hover_cursor(egui::CursorIcon::PointingHand)`
para que el cursor se vuelva manito. Sin esto la UI se siente muerta. Aplica a:

- Todos los `Button`
- `Label` con `Sense::click()`
- Cards de job
- Headers de workspace y repo
- Items de menú contextual

### Cards clickeables full-row

Para que **toda la fila** sea clickeable (no solo el texto), usar
`allocate_exact_size(...).sense(Sense::click())`:

```rust
let (rect, response) = ui.allocate_exact_size(
    egui::vec2(full_width, theme.card_row_height),
    egui::Sense::click(),
);

// Pintar el bg primero (hover / selected)
let bg = if selected { theme.bg_card_selected }
         else if response.hovered() { theme.bg_card_hover }
         else { Color32::TRANSPARENT };
ui.painter().rect_filled(rect, 4.0, bg);

// Acento de selección (barra amarilla 3px a la izquierda)
if selected {
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
    ui.painter().rect_filled(bar, 0.0, theme.accent);
}

// Tree lines verticales (jerarquía)
paint_tree_line(ui, rect, theme, theme.tree_line_ws_x);

// Contenido dentro del rect, con padding
let inner = rect.shrink2(egui::vec2(40.0, 6.0));
let mut child = ui.new_child(UiBuilder::new().max_rect(inner)...);
child.label(...);

response.on_hover_cursor(egui::CursorIcon::PointingHand)
```

### Tree lines (jerarquía visual)

Para listas anidadas (workspace → repo → job), pintar líneas verticales
sutiles a la izquierda. El offset por nivel se configura en theme:

```rust
fn paint_tree_line(ui: &egui::Ui, rect: egui::Rect, theme: &Theme, offset_x: f32) {
    let x = rect.min.x + offset_x;
    ui.painter().line_segment(
        [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
        egui::Stroke::new(1.0, theme.border),
    );
}
```

### Paneles (Sidebar, BottomBar, CentralPanel)

Pasar **siempre `Frame` explícito** a los paneles, nunca confiar en el
default. Razón: el default usa `panel_fill` del visuals, pero la composición
anidada (CentralPanel dentro de otro Ui) a veces no lo aplica correctamente.

```rust
egui::Panel::left("sidebar")
    .size_range(self.theme.sidebar_min_width..=self.theme.sidebar_max_width)
    .frame(
        self.theme
            .surface_panel_frame()  // helper que devuelve Frame con bg_surface
            .inner_margin(egui::Margin::symmetric(12, 0)),
    )
    .show_inside(ui, |ui| { ... });
```

`Theme` expone dos helpers:

- `surface_panel_frame()` → `Frame` con `bg_surface` (sidebar, bottom bar,
  job header).
- `base_panel_frame()` → `Frame` con `bg_base` (central pane).

### ComboBox dropdown

El popup del ComboBox es una Area independiente. Para que sus items se vean
con buen tamaño:

```rust
egui::ComboBox::from_id_salt("my_combo")
    .selected_text(label)
    .width(width)
    .height(360.0)
    .show_ui(ui, |ui| {
        ui.spacing_mut().interact_size.y = 32.0;
        ui.spacing_mut().item_spacing.y = 2.0;
        for option in options {
            ui.selectable_label(selected, option);
        }
    });
```

### Status dots

Caracteres Unicode (no emoji) coloreados con el token de status:

| Status | Char | Token |
|---|---|---|
| Idle | `●` `\u{25CF}` | `status_idle` |
| Thinking | `◐` `\u{25D0}` | `status_thinking` |
| Paused | `○` `\u{25CB}` | `status_paused` |
| Error | `◆` `\u{25C6}` | `status_error` |
| NeedsAttention | `▲` `\u{25B2}` | `status_needs_attention` |

Test obligatorio: `status_dot_has_unique_char_per_variant` defiende este
contrato — agregar status nuevo requiere un char distinto.

### Chevrons (colapsable)

| Estado | Char |
|---|---|
| Expandido | `▾` `\u{25BE}` |
| Colapsado | `▸` `\u{25B8}` |

## Gotchas conocidos de egui 0.34

- **Popups no heredan Visuals.** Ver sección "Aplicar el theme".
- **`CentralPanel::default()` anidado puede no respetar `panel_fill`.** Pasar
  Frame explícito o pintar el rect con `painter().rect_filled` directo.
- **`set_visuals` propaga lazy.** Setearlo cada frame es la solución
  estándar para garantizar coherencia en todas las Areas.
- **`Frame::new()` sin `.fill(...)` queda transparente**, no usa
  `panel_fill`. Siempre pasar fill explícito.
- **APIs deprecated entre 0.31 y 0.34:**
  - `min_width / max_width / default_width` → `size_range` / `min_size` / `max_size` / `default_size`
  - `exact_height` → `exact_size`
  - `TopBottomPanel::top` / `SidePanel::left` → `Panel::top` / `Panel::left`
  - `eframe::App::update` → `eframe::App::ui` (recibe `&mut Ui` en vez de `&Context`)

## Cómo personalizar (usuario o Claude)

`~/.michi/theme.toml` se crea con los defaults la primera vez que michi
arranca. Editarlo y reabrir la app para aplicar los cambios. Ejemplo:

```toml
# Mas yellow agresivo, mas espacio entre cards
accent = "#ffd900"
card_row_height = 64.0
font_mono_size = 14.0
sidebar_default_width = 320.0
```

V2 va a sumar hot-reload del TOML con `notify` para no reabrir la app.

## Cómo extender

1. **Token nuevo:** agregar campo a `Theme`, su default en `dark_default()`,
   y serializarlo en `ThemeConfig` (forma con `String` para colores hex).
2. **Pattern nuevo:** documentarlo acá con un snippet copy-pasteable.
3. **Gotcha nuevo:** sumarlo a la sección de gotchas con el comportamiento
   observado y la fix.

Tests obligatorios para cualquier token: `default_serializes_and_parses`
debe seguir pasando. Para colores nuevos: el roundtrip se cubre automático.
