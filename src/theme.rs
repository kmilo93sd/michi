//! Tema visual de michi. Punto único de verdad para colores, fuentes y spacings.
//!
//! El theme se carga desde `~/.michi/theme.toml`. Si el archivo no existe se crea
//! con los defaults para que un usuario (o claude code) lo edite a mano.
//!
//! V2: file watcher con `notify` para hot-reload sin reiniciar la app.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use egui::Color32;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub struct Theme {
    pub accent: Color32,

    pub bg_base: Color32,
    pub bg_surface: Color32,
    pub bg_card_selected: Color32,
    pub bg_card_hover: Color32,
    pub border: Color32,

    pub text_primary: Color32,
    pub text_muted: Color32,
    pub text_workspace_label: Color32,
    pub text_repo_label: Color32,

    pub status_idle: Color32,
    pub status_thinking: Color32,
    pub status_paused: Color32,
    pub status_error: Color32,
    pub status_needs_attention: Color32,

    pub font_mono_size: f32,

    pub sidebar_min_width: f32,
    pub sidebar_max_width: f32,
    pub sidebar_default_width: f32,

    pub bottom_bar_height: f32,
    pub job_header_height: f32,
    pub card_row_height: f32,
    pub workspace_header_height: f32,
    pub repo_header_height: f32,
    pub tree_line_ws_x: f32,
    pub tree_line_repo_x: f32,
}

impl Theme {
    pub fn dark_default() -> Self {
        Self {
            accent: Color32::from_rgb(247, 201, 72),

            bg_base: Color32::from_rgb(15, 15, 17),
            bg_surface: Color32::from_rgb(22, 22, 26),
            bg_card_selected: Color32::from_rgb(38, 38, 44),
            bg_card_hover: Color32::from_rgb(28, 28, 33),
            border: Color32::from_rgb(40, 40, 47),

            text_primary: Color32::from_rgb(228, 228, 232),
            text_muted: Color32::from_rgb(140, 140, 156),
            text_workspace_label: Color32::from_rgb(170, 170, 188),
            text_repo_label: Color32::from_rgb(210, 210, 222),

            status_idle: Color32::from_rgb(48, 209, 88),
            status_thinking: Color32::from_rgb(247, 201, 72),
            status_paused: Color32::from_rgb(120, 120, 132),
            status_error: Color32::from_rgb(255, 69, 58),
            status_needs_attention: Color32::from_rgb(10, 132, 255),

            font_mono_size: 13.0,

            sidebar_min_width: 260.0,
            sidebar_max_width: 380.0,
            sidebar_default_width: 300.0,

            bottom_bar_height: 26.0,
            job_header_height: 76.0,
            card_row_height: 56.0,
            workspace_header_height: 42.0,
            repo_header_height: 26.0,
            tree_line_ws_x: 8.0,
            tree_line_repo_x: 24.0,
        }
    }

    /// Path canónico del archivo de tema. `~/.michi/theme.toml`.
    pub fn config_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("no se pudo obtener home dir")?;
        Ok(home.join(".michi").join("theme.toml"))
    }

    /// Carga el theme desde `~/.michi/theme.toml`. Si el archivo no existe lo crea
    /// con los defaults para que el usuario (o un agente AI) lo edite a mano.
    /// Si el parsing falla, logea un warn y vuelve al default.
    pub fn load_or_create_default() -> Self {
        match Self::try_load_or_create() {
            Ok(theme) => theme,
            Err(e) => {
                warn!("error cargando theme.toml, usando default: {e:#}");
                Self::dark_default()
            }
        }
    }

    fn try_load_or_create() -> Result<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            let parent = path.parent().context("config path sin parent dir")?;
            fs::create_dir_all(parent).with_context(|| format!("creando {}", parent.display()))?;
            let default_toml = Self::dark_default().to_toml_string()?;
            fs::write(&path, &default_toml)
                .with_context(|| format!("escribiendo theme default en {}", path.display()))?;
            info!("creado theme.toml default en {}", path.display());
            return Ok(Self::dark_default());
        }

        let raw =
            fs::read_to_string(&path).with_context(|| format!("leyendo {}", path.display()))?;
        let cfg: ThemeConfig =
            toml::from_str(&raw).with_context(|| format!("parseando {}", path.display()))?;
        Theme::try_from(cfg).context("convirtiendo ThemeConfig a Theme")
    }

    fn to_toml_string(&self) -> Result<String> {
        let cfg = ThemeConfig::from(self);
        toml::to_string_pretty(&cfg).context("serializando theme a toml")
    }

    /// Construye un `Visuals` de egui basado en este tema. Aplica colores a paneles,
    /// scroll, separators y texto.
    pub fn build_visuals(&self) -> egui::Visuals {
        let mut v = egui::Visuals::dark();
        v.panel_fill = self.bg_base;
        v.window_fill = self.bg_surface;
        v.extreme_bg_color = self.bg_base;
        v.faint_bg_color = self.bg_surface;
        v.override_text_color = Some(self.text_primary);
        v.widgets.noninteractive.bg_stroke.color = self.border;
        v.widgets.noninteractive.fg_stroke.color = self.text_primary;
        v.widgets.inactive.bg_fill = self.bg_surface;
        v.widgets.inactive.weak_bg_fill = self.bg_surface;
        v.widgets.inactive.fg_stroke.color = self.text_primary;
        v.widgets.hovered.bg_fill = self.bg_card_hover;
        v.widgets.hovered.weak_bg_fill = self.bg_card_hover;
        v.widgets.active.bg_fill = self.bg_card_selected;
        v.widgets.active.weak_bg_fill = self.bg_card_selected;
        v.selection.bg_fill = self.accent.linear_multiply(0.4);
        v.selection.stroke.color = self.accent;
        v.hyperlink_color = self.accent;
        v
    }

    /// Frame para paneles "chrome" (sidebar, bottom bar). Color `bg_surface`.
    pub fn surface_panel_frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.bg_surface)
            .inner_margin(egui::Margin::same(0))
    }

    /// Frame para el área central (terminal placeholder, empty state). Color `bg_base`.
    pub fn base_panel_frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.bg_base)
            .inner_margin(egui::Margin::same(0))
    }
}

/// Forma serializable del tema. Colores como strings hex tipo "#f7c948".
/// Esta capa existe para que el TOML sea AI-friendly y human-friendly.
#[derive(Debug, Serialize, Deserialize)]
struct ThemeConfig {
    accent: String,

    bg_base: String,
    bg_surface: String,
    bg_card_selected: String,
    bg_card_hover: String,
    border: String,

    text_primary: String,
    text_muted: String,
    text_workspace_label: String,
    text_repo_label: String,

    status_idle: String,
    status_thinking: String,
    status_paused: String,
    status_error: String,
    status_needs_attention: String,

    font_mono_size: f32,

    sidebar_min_width: f32,
    sidebar_max_width: f32,
    sidebar_default_width: f32,

    bottom_bar_height: f32,
    job_header_height: f32,
    card_row_height: f32,
    workspace_header_height: f32,
    repo_header_height: f32,
    tree_line_ws_x: f32,
    tree_line_repo_x: f32,
}

impl From<&Theme> for ThemeConfig {
    fn from(t: &Theme) -> Self {
        Self {
            accent: hex_from(t.accent),
            bg_base: hex_from(t.bg_base),
            bg_surface: hex_from(t.bg_surface),
            bg_card_selected: hex_from(t.bg_card_selected),
            bg_card_hover: hex_from(t.bg_card_hover),
            border: hex_from(t.border),
            text_primary: hex_from(t.text_primary),
            text_muted: hex_from(t.text_muted),
            text_workspace_label: hex_from(t.text_workspace_label),
            text_repo_label: hex_from(t.text_repo_label),
            status_idle: hex_from(t.status_idle),
            status_thinking: hex_from(t.status_thinking),
            status_paused: hex_from(t.status_paused),
            status_error: hex_from(t.status_error),
            status_needs_attention: hex_from(t.status_needs_attention),
            font_mono_size: t.font_mono_size,
            sidebar_min_width: t.sidebar_min_width,
            sidebar_max_width: t.sidebar_max_width,
            sidebar_default_width: t.sidebar_default_width,
            bottom_bar_height: t.bottom_bar_height,
            job_header_height: t.job_header_height,
            card_row_height: t.card_row_height,
            workspace_header_height: t.workspace_header_height,
            repo_header_height: t.repo_header_height,
            tree_line_ws_x: t.tree_line_ws_x,
            tree_line_repo_x: t.tree_line_repo_x,
        }
    }
}

impl TryFrom<ThemeConfig> for Theme {
    type Error = anyhow::Error;

    fn try_from(c: ThemeConfig) -> Result<Self> {
        Ok(Self {
            accent: hex_to(&c.accent)?,
            bg_base: hex_to(&c.bg_base)?,
            bg_surface: hex_to(&c.bg_surface)?,
            bg_card_selected: hex_to(&c.bg_card_selected)?,
            bg_card_hover: hex_to(&c.bg_card_hover)?,
            border: hex_to(&c.border)?,
            text_primary: hex_to(&c.text_primary)?,
            text_muted: hex_to(&c.text_muted)?,
            text_workspace_label: hex_to(&c.text_workspace_label)?,
            text_repo_label: hex_to(&c.text_repo_label)?,
            status_idle: hex_to(&c.status_idle)?,
            status_thinking: hex_to(&c.status_thinking)?,
            status_paused: hex_to(&c.status_paused)?,
            status_error: hex_to(&c.status_error)?,
            status_needs_attention: hex_to(&c.status_needs_attention)?,
            font_mono_size: c.font_mono_size,
            sidebar_min_width: c.sidebar_min_width,
            sidebar_max_width: c.sidebar_max_width,
            sidebar_default_width: c.sidebar_default_width,
            bottom_bar_height: c.bottom_bar_height,
            job_header_height: c.job_header_height,
            card_row_height: c.card_row_height,
            workspace_header_height: c.workspace_header_height,
            repo_header_height: c.repo_header_height,
            tree_line_ws_x: c.tree_line_ws_x,
            tree_line_repo_x: c.tree_line_repo_x,
        })
    }
}

fn hex_from(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn hex_to(s: &str) -> Result<Color32> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        bail!("color hex debe tener 6 chars (#rrggbb), recibido: {s:?}");
    }
    let r = u8::from_str_radix(&s[0..2], 16).with_context(|| format!("parsing r de {s:?}"))?;
    let g = u8::from_str_radix(&s[2..4], 16).with_context(|| format!("parsing g de {s:?}"))?;
    let b = u8::from_str_radix(&s[4..6], 16).with_context(|| format!("parsing b de {s:?}"))?;
    Ok(Color32::from_rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let c = Color32::from_rgb(247, 201, 72);
        let s = hex_from(c);
        assert_eq!(s, "#f7c948");
        let back = hex_to(&s).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn hex_accepts_no_prefix() {
        assert_eq!(hex_to("f7c948").unwrap(), Color32::from_rgb(247, 201, 72));
    }

    #[test]
    fn hex_rejects_wrong_length() {
        assert!(hex_to("#f7c94").is_err());
    }

    #[test]
    fn default_serializes_and_parses() {
        let theme = Theme::dark_default();
        let toml_str = theme.to_toml_string().unwrap();
        let cfg: ThemeConfig = toml::from_str(&toml_str).unwrap();
        let back = Theme::try_from(cfg).unwrap();
        assert_eq!(back.accent, theme.accent);
        assert_eq!(back.bg_base, theme.bg_base);
        assert_eq!(back.font_mono_size, theme.font_mono_size);
    }
}
