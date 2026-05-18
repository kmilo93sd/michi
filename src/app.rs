use std::collections::HashSet;

use eframe::CreationContext;

use crate::state::{Job, JobStatus, Workspace};
use crate::theme::Theme;
use crate::ui::new_job_modal::{self, ModalAction, NewJobModalState};

pub struct App {
    pub workspaces: Vec<Workspace>,
    pub jobs: Vec<Job>,
    pub selected_job_id: Option<String>,
    pub collapsed_workspaces: HashSet<String>,
    pub collapsed_repos: HashSet<String>,
    pub theme: Theme,
    pub new_job_modal_open: bool,
    pub new_job_modal_state: NewJobModalState,
}

impl App {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        let theme = Theme::load_or_create_default();
        cc.egui_ctx.set_visuals(theme.build_visuals());
        Self {
            workspaces: Vec::new(),
            jobs: Vec::new(),
            selected_job_id: None,
            collapsed_workspaces: HashSet::new(),
            collapsed_repos: HashSet::new(),
            theme,
            new_job_modal_open: false,
            new_job_modal_state: NewJobModalState::initial(),
        }
    }

    fn open_new_job_modal(&mut self) {
        self.new_job_modal_state = NewJobModalState::initial();
        self.new_job_modal_open = true;
    }

    fn close_new_job_modal(&mut self) {
        self.new_job_modal_open = false;
    }

    pub fn selected_job(&self) -> Option<&Job> {
        let id = self.selected_job_id.as_deref()?;
        self.jobs.iter().find(|j| j.id == id)
    }

    fn count_thinking(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| j.status == JobStatus::Thinking)
            .count()
    }

    fn load_mock(&mut self) {
        self.workspaces = Workspace::mock_set();
        self.jobs = Job::mock_set();
        self.selected_job_id = self.jobs.first().map(|j| j.id.clone());
    }

    fn clear_jobs(&mut self) {
        self.workspaces.clear();
        self.jobs.clear();
        self.selected_job_id = None;
    }

    fn jobs_for_repo(&self, workspace: &str, repo: &str) -> Vec<&Job> {
        self.jobs
            .iter()
            .filter(|j| j.workspace == workspace && j.repo == repo)
            .collect()
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::bottom("bottom_bar")
            .exact_size(self.theme.bottom_bar_height)
            .frame(
                self.theme
                    .surface_panel_frame()
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.small(
                        "Ctrl+N nuevo \u{B7} Ctrl+Tab siguiente \u{B7} \
                         Ctrl+Shift+Tab anterior \u{B7} Ctrl+W cerrar trabajo",
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small(egui::RichText::new("dev:").weak());
                        if self.jobs.is_empty() {
                            if ui
                                .small_button("cargar mock")
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.load_mock();
                            }
                        } else if ui
                            .small_button("limpiar")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.clear_jobs();
                        }
                    });
                });
            });

        egui::Panel::left("sidebar")
            .size_range(self.theme.sidebar_min_width..=self.theme.sidebar_max_width)
            .default_size(self.theme.sidebar_default_width)
            .resizable(true)
            .frame(
                self.theme
                    .surface_panel_frame()
                    .inner_margin(egui::Margin::symmetric(12, 0)),
            )
            .show_inside(ui, |ui| {
                ui.style_mut().override_font_id =
                    Some(egui::FontId::monospace(self.theme.font_mono_size));

                ui.add_space(10.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new("+ Nuevo trabajo"),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    self.open_new_job_modal();
                }
                ui.add_space(10.0);

                if !self.jobs.is_empty() {
                    ui.small(
                        egui::RichText::new(format!(
                            "{} trabajos \u{B7} {} pensando",
                            self.jobs.len(),
                            self.count_thinking()
                        ))
                        .weak(),
                    );
                    ui.add_space(6.0);
                }

                ui.separator();

                if self.workspaces.is_empty() {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.small(egui::RichText::new("Aun no hay trabajos.").weak());
                    });
                } else {
                    self.render_sidebar_tree(ui);
                }
            });

        let mut empty_state_create_clicked = false;
        egui::CentralPanel::default()
            .frame(self.theme.base_panel_frame())
            .show_inside(ui, |ui| {
                if self.jobs.is_empty() {
                    empty_state_create_clicked = render_empty_state(ui);
                } else if let Some(job) = self.selected_job() {
                    render_job_pane(ui, job, &self.theme);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Selecciona un trabajo de la barra lateral");
                    });
                }
            });
        if empty_state_create_clicked {
            self.open_new_job_modal();
        }

        if self.new_job_modal_open {
            let action = new_job_modal::show(
                ui.ctx(),
                &mut self.new_job_modal_state,
                &self.workspaces,
                &self.theme,
            );
            match action {
                ModalAction::Submit => {
                    // TODO Fase 3 Bloque G: wire al git/worktree.rs y crear Job real.
                    // Hoy solo cerramos el modal.
                    self.close_new_job_modal();
                }
                ModalAction::Cancel => {
                    self.close_new_job_modal();
                }
                ModalAction::None => {}
            }
        }
    }
}

impl App {
    fn render_sidebar_tree(&mut self, ui: &mut egui::Ui) {
        let mut clicked_id: Option<String> = None;
        let mut toggle_ws: Option<String> = None;
        let mut toggle_repo: Option<String> = None;

        // Clone para evitar lifetime acrobatics: el loop necesita iterar workspaces
        // y simultaneamente leer self.collapsed_workspaces y jobs_for_repo.
        let workspaces = self.workspaces.clone();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for ws in &workspaces {
                    ui.add_space(8.0);
                    let ws_collapsed = self.collapsed_workspaces.contains(&ws.id);
                    if workspace_header(ui, &self.theme, ws, ws_collapsed).clicked() {
                        toggle_ws = Some(ws.id.clone());
                    }

                    if !ws_collapsed {
                        for repo in &ws.repos {
                            let repo_collapsed = self.collapsed_repos.contains(&repo.id);
                            let repo_jobs = self.jobs_for_repo(&ws.name, &repo.name);

                            if repo_header(
                                ui,
                                &self.theme,
                                &repo.name,
                                repo_collapsed,
                                repo_jobs.len(),
                            )
                            .clicked()
                            {
                                toggle_repo = Some(repo.id.clone());
                            }

                            if !repo_collapsed {
                                for job in repo_jobs {
                                    let selected = self.selected_job_id.as_deref() == Some(&job.id);
                                    if render_job_card(ui, job, selected, &self.theme).clicked() {
                                        clicked_id = Some(job.id.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            });

        if let Some(id) = clicked_id {
            self.selected_job_id = Some(id);
        }
        if let Some(id) = toggle_ws
            && !self.collapsed_workspaces.remove(&id)
        {
            self.collapsed_workspaces.insert(id);
        }
        if let Some(id) = toggle_repo
            && !self.collapsed_repos.remove(&id)
        {
            self.collapsed_repos.insert(id);
        }
    }
}

fn workspace_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    ws: &Workspace,
    collapsed: bool,
) -> egui::Response {
    let full_width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(full_width, theme.workspace_header_height),
        egui::Sense::click(),
    );

    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, theme.bg_card_hover);
    }

    let chevron = if collapsed { "\u{25B8}" } else { "\u{25BE}" };
    let inner = rect.shrink2(egui::vec2(6.0, 4.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );

    child.label(
        egui::RichText::new(format!("{} {}", chevron, ws.name.to_uppercase()))
            .small()
            .color(theme.text_workspace_label),
    );
    child.label(
        egui::RichText::new(format!(
            "{} specs \u{B7} {} skills",
            ws.specs_count, ws.skills_count
        ))
        .small()
        .color(theme.text_muted),
    );

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn repo_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    collapsed: bool,
    job_count: usize,
) -> egui::Response {
    let full_width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(full_width, theme.repo_header_height),
        egui::Sense::click(),
    );

    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, theme.bg_card_hover);
    }

    paint_tree_line(ui, rect, theme, theme.tree_line_ws_x);

    let chevron = if collapsed { "\u{25B8}" } else { "\u{25BE}" };
    let inner = rect.shrink2(egui::vec2(22.0, 4.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    child.label(egui::RichText::new(format!("{} {}", chevron, name)).color(theme.text_repo_label));
    if job_count > 0 {
        child.add_space(8.0);
        child.label(
            egui::RichText::new(format!("{}", job_count))
                .small()
                .color(theme.text_muted),
        );
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn paint_tree_line(ui: &egui::Ui, rect: egui::Rect, theme: &Theme, offset_x: f32) {
    let x = rect.min.x + offset_x;
    ui.painter().line_segment(
        [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
        egui::Stroke::new(1.0, theme.border),
    );
}

fn render_job_card(ui: &mut egui::Ui, job: &Job, selected: bool, theme: &Theme) -> egui::Response {
    let full_width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(full_width, theme.card_row_height),
        egui::Sense::click(),
    );

    let bg = if selected {
        theme.bg_card_selected
    } else if response.hovered() {
        theme.bg_card_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, bg);

    if selected {
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
        ui.painter().rect_filled(bar, 0.0, theme.accent);
    }

    paint_tree_line(ui, rect, theme, theme.tree_line_ws_x);
    paint_tree_line(ui, rect, theme, theme.tree_line_repo_x);

    let inner = rect.shrink2(egui::vec2(40.0, 6.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );

    child.horizontal(|ui| {
        ui.colored_label(job.status.color(theme), job.status.dot().to_string());
        ui.label(egui::RichText::new(&job.branch).strong());
    });

    let subtitle_color = if job.status == JobStatus::NeedsAttention {
        Some(theme.status_needs_attention)
    } else {
        None
    };
    let subtitle_text = egui::RichText::new(job.subtitle()).small();
    if let Some(c) = subtitle_color {
        child.label(subtitle_text.color(c));
    } else {
        child.label(subtitle_text.weak());
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn render_empty_state(ui: &mut egui::Ui) -> bool {
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        ui.add_space(96.0);
        ui.heading("Sin trabajos todavia");
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(
                "Cada trabajo es un Claude Code corriendo en su propio worktree de git.\n\
                 Crea uno para empezar a paralelizar tu trabajo sin pisarte entre repos.",
            )
            .weak(),
        );
        ui.add_space(24.0);
        if ui
            .add_sized([220.0, 36.0], egui::Button::new("+ Crear primer trabajo"))
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            clicked = true;
        }
        ui.add_space(8.0);
        ui.small(egui::RichText::new("o Ctrl+N").weak());
    });
    clicked
}

fn render_job_pane(ui: &mut egui::Ui, job: &Job, theme: &Theme) {
    let header_frame = egui::Frame::new()
        .fill(theme.bg_surface)
        .inner_margin(egui::Margin::symmetric(12, 8));
    egui::Panel::top("main_header")
        .exact_size(theme.job_header_height)
        .frame(header_frame)
        .show_inside(ui, |ui| {
            ui.strong(format!("{} \u{B7} {}", job.repo, job.branch));
            ui.label(job.worktree_path.to_string_lossy().replace('\\', "/"));
            ui.horizontal(|ui| {
                ui.label(format!("{} archivos modificados", job.files_changed));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let _ = ui
                        .button("X")
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    let _ = ui
                        .button("Commit & Push")
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    let _ = ui
                        .button("Diff")
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                });
            });
        });

    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0.0, theme.bg_base);
    ui.add_space(12.0);
    ui.label(egui::RichText::new("terminal placeholder (Fase 4)").color(theme.text_muted));
}
