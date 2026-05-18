use std::collections::BTreeMap;

use eframe::CreationContext;

use crate::state::{Job, JobStatus};

const ACCENT: egui::Color32 = egui::Color32::from_rgb(247, 201, 72);
const CARD_BG_SELECTED: egui::Color32 = egui::Color32::from_rgb(40, 40, 44);
const CARD_BG_HOVER: egui::Color32 = egui::Color32::from_rgb(32, 32, 36);
const WORKSPACE_LABEL: egui::Color32 = egui::Color32::from_rgb(140, 140, 150);

pub struct App {
    pub jobs: Vec<Job>,
    pub selected_job_id: Option<String>,
}

impl App {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        Self {
            jobs: Vec::new(),
            selected_job_id: None,
        }
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
        self.jobs = Job::mock_set();
        self.selected_job_id = self.jobs.first().map(|j| j.id.clone());
    }

    fn clear_jobs(&mut self) {
        self.jobs.clear();
        self.selected_job_id = None;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("bottom_bar")
            .exact_height(24.0)
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.small(
                        "Ctrl+N nuevo \u{B7} Ctrl+Tab siguiente \u{B7} \
                         Ctrl+Shift+Tab anterior \u{B7} Ctrl+W cerrar trabajo",
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small(egui::RichText::new("dev:").weak());
                        if self.jobs.is_empty() {
                            if ui.small_button("cargar mock").clicked() {
                                self.load_mock();
                            }
                        } else if ui.small_button("limpiar").clicked() {
                            self.clear_jobs();
                        }
                    });
                });
            });

        egui::SidePanel::left("sidebar")
            .min_width(260.0)
            .max_width(320.0)
            .default_width(280.0)
            .resizable(true)
            .show_inside(ui, |ui| {
                ui.add_space(10.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new("+ Nuevo trabajo"),
                    )
                    .clicked()
                {
                    // TODO Fase 3: abrir modal nuevo trabajo
                }
                ui.add_space(10.0);

                if !self.jobs.is_empty() {
                    ui.horizontal(|ui| {
                        ui.small(
                            egui::RichText::new(format!(
                                "{} trabajos \u{B7} {} pensando",
                                self.jobs.len(),
                                self.count_thinking()
                            ))
                            .weak(),
                        );
                    });
                    ui.add_space(6.0);
                }

                ui.separator();

                if self.jobs.is_empty() {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.small(egui::RichText::new("Aun no hay trabajos.").weak());
                    });
                } else {
                    self.render_sidebar_jobs(ui);
                }
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if self.jobs.is_empty() {
                render_empty_state(ui);
            } else if let Some(job) = self.selected_job() {
                render_job_pane(ui, job);
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Selecciona un trabajo de la barra lateral");
                });
            }
        });
    }
}

impl App {
    fn render_sidebar_jobs(&mut self, ui: &mut egui::Ui) {
        let grouped = group_by_workspace(&self.jobs);
        let mut clicked_id: Option<String> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (workspace, jobs) in grouped {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(workspace.to_uppercase())
                            .small()
                            .color(WORKSPACE_LABEL),
                    );
                    ui.add_space(4.0);

                    for job in jobs {
                        let selected = self.selected_job_id.as_deref() == Some(&job.id);
                        if render_job_card(ui, job, selected).clicked() {
                            clicked_id = Some(job.id.clone());
                        }
                    }
                }
            });

        if let Some(id) = clicked_id {
            self.selected_job_id = Some(id);
        }
    }
}

fn group_by_workspace(jobs: &[Job]) -> BTreeMap<&str, Vec<&Job>> {
    let mut map: BTreeMap<&str, Vec<&Job>> = BTreeMap::new();
    for j in jobs {
        map.entry(j.workspace.as_str()).or_default().push(j);
    }
    map
}

fn render_job_card(ui: &mut egui::Ui, job: &Job, selected: bool) -> egui::Response {
    let full_width = ui.available_width();
    let row_height = 56.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(full_width, row_height),
        egui::Sense::click(),
    );

    let bg = if selected {
        CARD_BG_SELECTED
    } else if response.hovered() {
        CARD_BG_HOVER
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4.0, bg);

    if selected {
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height()));
        ui.painter().rect_filled(bar, 0.0, ACCENT);
    }

    let inner = rect.shrink2(egui::vec2(12.0, 8.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );

    child.horizontal(|ui| {
        ui.colored_label(job.status.color(), job.status.dot().to_string());
        ui.label(egui::RichText::new(&job.repo).strong());
    });
    child.label(egui::RichText::new(&job.branch).small().weak());

    let subtitle_color = if job.status == JobStatus::NeedsAttention {
        Some(JobStatus::NeedsAttention.color())
    } else {
        None
    };
    let subtitle_text = egui::RichText::new(job.subtitle()).small();
    if let Some(c) = subtitle_color {
        child.label(subtitle_text.color(c));
    } else {
        child.label(subtitle_text.weak());
    }

    response
}

fn render_empty_state(ui: &mut egui::Ui) {
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
        let _ = ui.add_sized([220.0, 36.0], egui::Button::new("+ Crear primer trabajo"));
        ui.add_space(8.0);
        ui.small(egui::RichText::new("o Ctrl+N").weak());
    });
}

fn render_job_pane(ui: &mut egui::Ui, job: &Job) {
    egui::TopBottomPanel::top("main_header")
        .exact_height(72.0)
        .show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.strong(format!("{} \u{B7} {}", job.repo, job.branch));
            ui.label(job.worktree_path.to_string_lossy().replace('\\', "/"));
            ui.horizontal(|ui| {
                ui.label(format!("{} archivos modificados", job.files_changed));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let _ = ui.button("X");
                    let _ = ui.button("Commit & Push");
                    let _ = ui.button("Diff");
                });
            });
        });

    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgb(20, 20, 22))
        .show(ui, |ui| {
            ui.allocate_space(ui.available_size());
            ui.label("terminal placeholder (Fase 4)");
        });
}
