//! Modal "Nuevo trabajo". Solo UI + validaciones de formato.
//!
//! La integración con git (crear worktree real) y con el state global llega
//! en bloques siguientes de la Fase 3. Hoy el modal recolecta inputs, valida
//! formato y devuelve `ModalAction::Submit` cuando el usuario aprieta "Crear"
//! con un estado válido.

use crate::state::Workspace;
use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalAction {
    None,
    Cancel,
    Submit,
}

#[derive(Debug, Clone)]
pub struct NewJobModalState {
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    pub branch: String,
    pub base_branch: String,
    pub initial_task: String,
}

impl NewJobModalState {
    pub fn initial() -> Self {
        Self {
            workspace_id: None,
            repo_id: None,
            branch: String::new(),
            base_branch: "main".into(),
            initial_task: String::new(),
        }
    }

    pub fn branch_valid(&self) -> bool {
        !self.branch.is_empty() && self.branch.chars().all(is_valid_branch_char)
    }

    pub fn is_valid(&self) -> bool {
        self.workspace_id.is_some()
            && self.repo_id.is_some()
            && self.branch_valid()
            && !self.base_branch.is_empty()
    }
}

fn is_valid_branch_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '/' || c == '_' || c == '-'
}

pub fn show(
    ctx: &egui::Context,
    state: &mut NewJobModalState,
    workspaces: &[Workspace],
    theme: &Theme,
    creating: bool,
    last_error: Option<&str>,
) -> ModalAction {
    let mut action = ModalAction::None;

    let frame = egui::Frame::new()
        .fill(theme.bg_surface)
        .inner_margin(egui::Margin::same(20))
        .corner_radius(egui::CornerRadius::same(8))
        .stroke(egui::Stroke::new(1.0, theme.border));

    let modal_response = egui::Modal::new(egui::Id::new("new_job_modal"))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.set_max_width(560.0);

            render_header(ui, &mut action, creating);
            ui.add_space(12.0);

            ui.add_enabled_ui(!creating, |ui| {
                render_workspace_select(ui, state, workspaces, theme);
                ui.add_space(10.0);

                render_repo_select(ui, state, workspaces, theme);
                ui.add_space(10.0);

                render_branch_input(ui, state, theme);
                ui.add_space(10.0);

                render_base_branch_input(ui, state, theme);
                ui.add_space(10.0);

                render_initial_task_input(ui, state, theme);
            });

            if let Some(err) = last_error {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(format!("Error: {err}"))
                        .small()
                        .color(theme.status_error),
                );
            }

            ui.add_space(16.0);
            render_footer(ui, state, &mut action, creating);
        });

    if !creating {
        if modal_response.backdrop_response.clicked() {
            action = ModalAction::Cancel;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = ModalAction::Cancel;
        }
    }

    action
}

fn render_header(ui: &mut egui::Ui, action: &mut ModalAction, creating: bool) {
    ui.horizontal(|ui| {
        ui.heading("Nuevo trabajo");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close_btn = ui
                .add_enabled(!creating, egui::Button::new("X").small())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if close_btn.clicked() {
                *action = ModalAction::Cancel;
            }
        });
    });
}

fn render_workspace_select(
    ui: &mut egui::Ui,
    state: &mut NewJobModalState,
    workspaces: &[Workspace],
    theme: &Theme,
) {
    ui.label(
        egui::RichText::new("Workspace")
            .small()
            .color(theme.text_muted),
    );
    let selected_label = state
        .workspace_id
        .as_deref()
        .and_then(|id| workspaces.iter().find(|w| w.id == id))
        .map(|w| w.name.clone())
        .unwrap_or_else(|| "Selecciona...".into());

    let width = ui.available_width();
    egui::ComboBox::from_id_salt("ws_combo")
        .selected_text(selected_label)
        .width(width)
        .show_ui(ui, |ui| {
            for ws in workspaces {
                let selected = state.workspace_id.as_deref() == Some(ws.id.as_str());
                if ui.selectable_label(selected, &ws.name).clicked() {
                    state.workspace_id = Some(ws.id.clone());
                    state.repo_id = None;
                }
            }
        });
}

fn render_repo_select(
    ui: &mut egui::Ui,
    state: &mut NewJobModalState,
    workspaces: &[Workspace],
    theme: &Theme,
) {
    ui.label(egui::RichText::new("Repo").small().color(theme.text_muted));

    let current_ws = state
        .workspace_id
        .as_deref()
        .and_then(|id| workspaces.iter().find(|w| w.id == id));
    let selected_label = state
        .repo_id
        .as_deref()
        .and_then(|id| current_ws.and_then(|w| w.repos.iter().find(|r| r.id == id)))
        .map(|r| r.name.clone())
        .unwrap_or_else(|| "Selecciona un workspace primero".into());

    let width = ui.available_width();
    let enabled = current_ws.is_some();
    ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_id_salt("repo_combo")
            .selected_text(selected_label)
            .width(width)
            .show_ui(ui, |ui| {
                if let Some(ws) = current_ws {
                    for repo in &ws.repos {
                        let selected = state.repo_id.as_deref() == Some(repo.id.as_str());
                        if ui.selectable_label(selected, &repo.name).clicked() {
                            state.repo_id = Some(repo.id.clone());
                        }
                    }
                }
            });
    });
}

fn render_branch_input(ui: &mut egui::Ui, state: &mut NewJobModalState, theme: &Theme) {
    ui.label(
        egui::RichText::new("Rama nueva")
            .small()
            .color(theme.text_muted),
    );
    ui.add(
        egui::TextEdit::singleline(&mut state.branch)
            .desired_width(f32::INFINITY)
            .hint_text("feat/cors-fix"),
    );
    if !state.branch.is_empty() && !state.branch_valid() {
        ui.small(
            egui::RichText::new("Solo letras minusculas, numeros, /, _ y -")
                .color(theme.status_error),
        );
    }
}

fn render_base_branch_input(ui: &mut egui::Ui, state: &mut NewJobModalState, theme: &Theme) {
    ui.label(
        egui::RichText::new("Se crea desde")
            .small()
            .color(theme.text_muted),
    );
    ui.add(egui::TextEdit::singleline(&mut state.base_branch).desired_width(f32::INFINITY));
}

fn render_initial_task_input(ui: &mut egui::Ui, state: &mut NewJobModalState, theme: &Theme) {
    ui.label(
        egui::RichText::new("Tarea inicial (opcional)")
            .small()
            .color(theme.text_muted),
    );
    ui.add(
        egui::TextEdit::multiline(&mut state.initial_task)
            .desired_width(f32::INFINITY)
            .desired_rows(3)
            .hint_text("Arregla el CORS para que use whitelist en vez de '*'"),
    );
    ui.small(
        egui::RichText::new("Esto se envia como primer prompt a Claude").color(theme.text_muted),
    );
}

fn render_footer(
    ui: &mut egui::Ui,
    state: &NewJobModalState,
    action: &mut ModalAction,
    creating: bool,
) {
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if creating { "Creando..." } else { "Crear" };
            let enabled = !creating && state.is_valid();
            if ui
                .add_enabled(enabled, egui::Button::new(label))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                *action = ModalAction::Submit;
            }
            ui.add_space(8.0);
            if ui
                .add_enabled(!creating, egui::Button::new("Cancelar"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                *action = ModalAction::Cancel;
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_invalid() {
        let s = NewJobModalState::initial();
        assert!(!s.is_valid());
        assert_eq!(s.base_branch, "main");
    }

    #[test]
    fn branch_validates_lowercase_alphanumeric_and_separators() {
        let mut s = NewJobModalState::initial();
        s.branch = "feat/cors-fix_01".into();
        assert!(s.branch_valid());
    }

    #[test]
    fn branch_rejects_uppercase() {
        let mut s = NewJobModalState::initial();
        s.branch = "Feat/CorsFix".into();
        assert!(!s.branch_valid());
    }

    #[test]
    fn branch_rejects_empty() {
        let s = NewJobModalState::initial();
        assert!(!s.branch_valid());
    }

    #[test]
    fn branch_rejects_special_chars() {
        let mut s = NewJobModalState::initial();
        s.branch = "feat/cors fix".into();
        assert!(!s.branch_valid());
        s.branch = "feat/cors@fix".into();
        assert!(!s.branch_valid());
    }

    #[test]
    fn is_valid_requires_all_fields() {
        let mut s = NewJobModalState::initial();
        s.workspace_id = Some("ws-1".into());
        s.repo_id = Some("repo-1".into());
        s.branch = "feat/x".into();
        assert!(s.is_valid());

        s.base_branch.clear();
        assert!(!s.is_valid());
    }
}
