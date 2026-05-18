use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::CreationContext;
use egui_term::{PtyEvent, TerminalView};
use tracing::{debug, warn};

use crate::state::{AppState, Job, JobStatus, Workspace};
use crate::terminal::{self, JobTerminal};
use crate::theme::Theme;
use crate::ui::new_job_modal::{self, ModalAction, NewJobModalState};
use crate::worker::{
    self, CreateWorktreeRequest, RemoveWorktreeRequest, StatusPollTarget, WorkerEvent,
};

/// Cuánto esperar tras el último cambio antes de persistir el state al disco.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

/// Cada cuánto el worker poller hace `git status` por job.
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub struct App {
    pub workspaces: Vec<Workspace>,
    pub jobs: Vec<Job>,
    pub selected_job_id: Option<String>,
    pub collapsed_workspaces: HashSet<String>,
    pub collapsed_repos: HashSet<String>,
    pub theme: Theme,
    pub new_job_modal_open: bool,
    pub new_job_modal_state: NewJobModalState,
    pub creating_worktree: bool,
    pub last_error: Option<String>,
    worker_tx: Sender<WorkerEvent>,
    worker_rx: Receiver<WorkerEvent>,
    status_targets_tx: Sender<Vec<StatusPollTarget>>,
    dirty_since: Option<Instant>,
    close_confirm: Option<CloseConfirm>,
    /// Pendiente de eleccion: el usuario hizo click en ▶ de un repo y debe
    /// decidir si abrir sesion directa o worktree nuevo.
    start_choice: Option<StartChoice>,
    /// Un terminal por job-id. Lazy: se crea cuando el job se renderiza por
    /// primera vez. Drop del backend cierra el PTY automaticamente.
    terminals: HashMap<String, JobTerminal>,
    pty_tx: Sender<(u64, PtyEvent)>,
    pty_rx: Receiver<(u64, PtyEvent)>,
    /// Contador incremental para asignar id numerico unico a cada
    /// TerminalBackend (egui_term lo requiere para enrutar eventos).
    next_backend_id: u64,
}

/// El usuario hizo click en ▶ de un repo. Mostrar dialogo "directa | worktree".
struct StartChoice {
    workspace_id: String,
    workspace_name: String,
    repo_name: String,
    repo_path: std::path::PathBuf,
}

/// Estado del modal de confirmacion para cerrar un job con cambios pendientes.
struct CloseConfirm {
    job_id: String,
    repo_path: std::path::PathBuf,
    worktree_path: std::path::PathBuf,
    files_changed: u32,
}

enum CloseConfirmAction {
    Force,
    Cancel,
}

enum StartChoiceAction {
    Direct,
    Worktree,
    Cancel,
}

impl App {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        let theme = Theme::load_or_create_default();
        cc.egui_ctx.set_visuals(theme.build_visuals());
        let (worker_tx, worker_rx) = mpsc::channel();
        let (pty_tx, pty_rx) = mpsc::channel();
        let status_targets_tx =
            worker::spawn_status_poller(worker_tx.clone(), STATUS_POLL_INTERVAL);
        let persisted = AppState::load_or_default();
        let app = Self {
            workspaces: persisted.workspaces,
            jobs: persisted.jobs,
            selected_job_id: persisted.selected_job_id,
            collapsed_workspaces: persisted.collapsed_workspaces,
            collapsed_repos: persisted.collapsed_repos,
            theme,
            new_job_modal_open: false,
            new_job_modal_state: NewJobModalState::initial(),
            creating_worktree: false,
            last_error: None,
            worker_tx,
            worker_rx,
            status_targets_tx,
            dirty_since: None,
            close_confirm: None,
            start_choice: None,
            terminals: HashMap::new(),
            pty_tx,
            pty_rx,
            next_backend_id: 1,
        };
        app.push_status_targets();
        app
    }

    /// Crea (lazy) el terminal asociado al job. Si la creacion falla
    /// (working_directory invalido, etc), guarda el error en `last_error`
    /// y devuelve `None`.
    fn ensure_terminal_for(
        &mut self,
        ctx: &egui::Context,
        job_id: &str,
    ) -> Option<&mut JobTerminal> {
        if !self.terminals.contains_key(job_id) {
            let job = self.jobs.iter().find(|j| j.id == job_id)?.clone();
            let backend_id = self.next_backend_id;
            self.next_backend_id += 1;
            match JobTerminal::spawn(
                backend_id,
                ctx.clone(),
                self.pty_tx.clone(),
                &terminal::default_shell(),
                vec![],
                &job.worktree_path,
            ) {
                Ok(t) => {
                    self.terminals.insert(job_id.to_string(), t);
                }
                Err(e) => {
                    warn!("no se pudo spawnear terminal para job {job_id}: {e:#}");
                    self.last_error = Some(format!("terminal: {e:#}"));
                    return None;
                }
            }
        }
        self.terminals.get_mut(job_id)
    }

    /// Modal "¿Sesion directa o nuevo worktree?": dos opciones grandes.
    fn render_start_choice(&mut self, ctx: &egui::Context) {
        let Some(choice) = self.start_choice.as_ref() else {
            return;
        };
        let workspace_id = choice.workspace_id.clone();
        let workspace_name = choice.workspace_name.clone();
        let repo_name = choice.repo_name.clone();
        let repo_path = choice.repo_path.clone();

        let theme = &self.theme;
        let frame = egui::Frame::new()
            .fill(theme.bg_surface)
            .inner_margin(egui::Margin::same(20))
            .corner_radius(egui::CornerRadius::same(8))
            .stroke(egui::Stroke::new(1.0, theme.border));

        let mut action: Option<StartChoiceAction> = None;
        let modal_response = egui::Modal::new(egui::Id::new("start_choice_modal"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.style_mut().visuals = theme.build_visuals();
                ui.spacing_mut().button_padding = egui::vec2(14.0, 10.0);
                ui.set_min_width(440.0);
                ui.set_max_width(560.0);

                ui.heading("Iniciar trabajo");
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("{} / {}", workspace_name, repo_name))
                        .color(theme.text_muted),
                );
                ui.add_space(16.0);

                if ui
                    .add_sized(
                        [ui.available_width(), 56.0],
                        egui::Button::new(
                            egui::RichText::new("Sesion directa")
                                .strong()
                                .color(theme.text_primary),
                        ),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(
                        "Claude corre en el repo tal cual, sin crear branch nueva. \
                         Util para conversaciones rapidas.",
                    )
                    .clicked()
                {
                    action = Some(StartChoiceAction::Direct);
                }
                ui.add_space(6.0);
                ui.small(
                    egui::RichText::new("Claude corre en la branch actual, sin worktree separado.")
                        .color(theme.text_muted),
                );

                ui.add_space(14.0);

                if ui
                    .add_sized(
                        [ui.available_width(), 56.0],
                        egui::Button::new(
                            egui::RichText::new("Nuevo worktree (rama nueva)")
                                .strong()
                                .color(theme.text_primary),
                        ),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(
                        "Crea una rama nueva y un git worktree dedicado. \
                         Para tareas que no deben tocar el repo principal.",
                    )
                    .clicked()
                {
                    action = Some(StartChoiceAction::Worktree);
                }
                ui.add_space(6.0);
                ui.small(
                    egui::RichText::new(
                        "Abre el modal Nuevo trabajo con workspace y repo pre-llenados.",
                    )
                    .color(theme.text_muted),
                );

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("Cancelar")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            action = Some(StartChoiceAction::Cancel);
                        }
                    });
                });
            });

        if modal_response.backdrop_response.clicked() {
            action = Some(StartChoiceAction::Cancel);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = Some(StartChoiceAction::Cancel);
        }

        match action {
            Some(StartChoiceAction::Direct) => {
                self.start_choice = None;
                self.start_direct_session(&workspace_name, &repo_name, &repo_path);
            }
            Some(StartChoiceAction::Worktree) => {
                let repo_id = self
                    .workspaces
                    .iter()
                    .find(|w| w.id == workspace_id)
                    .and_then(|w| w.repos.iter().find(|r| r.name == repo_name))
                    .map(|r| r.id.clone());
                self.start_choice = None;
                if let Some(repo_id) = repo_id {
                    self.open_new_job_modal_for(&workspace_id, &repo_id);
                }
            }
            Some(StartChoiceAction::Cancel) => {
                self.start_choice = None;
            }
            None => {}
        }
    }

    /// Inicia una sesion directa para el repo: crea un Job sin worktree
    /// separado (worktree_path = repo_path). El terminal embebido se spawneara
    /// lazy en repo_path la proxima vez que se renderice el job.
    fn start_direct_session(
        &mut self,
        workspace_name: &str,
        repo_name: &str,
        repo_path: &std::path::Path,
    ) {
        let job = Job::for_direct_session(workspace_name, repo_name, repo_path);
        let id = job.id.clone();
        self.jobs.push(job);
        self.selected_job_id = Some(id);
        self.last_error = None;
        self.mark_dirty();
        self.push_status_targets();
    }

    /// El usuario eligio "Nuevo worktree" en el modal de eleccion: pre-llena
    /// el modal "Nuevo trabajo" con el workspace y repo seleccionados.
    fn open_new_job_modal_for(&mut self, workspace_id: &str, repo_id: &str) {
        self.new_job_modal_state = NewJobModalState::initial();
        self.new_job_modal_state.workspace_id = Some(workspace_id.to_string());
        self.new_job_modal_state.repo_id = Some(repo_id.to_string());
        self.last_error = None;
        self.new_job_modal_open = true;
    }

    /// Drena eventos PTY. Hoy solo loguea y limpia backends muertos; en
    /// bloques siguientes parsea patrones del output para actualizar
    /// JobStatus (idle/thinking/needs-attention).
    fn drain_pty_events(&mut self) {
        while let Ok((backend_id, event)) = self.pty_rx.try_recv() {
            debug!(backend_id, ?event, "pty event");
            if matches!(event, PtyEvent::Exit) {
                self.terminals.retain(|_, t| t.backend.id() != backend_id);
            }
        }
    }

    fn push_status_targets(&self) {
        let targets: Vec<StatusPollTarget> = self
            .jobs
            .iter()
            .map(|j| StatusPollTarget {
                job_id: j.id.clone(),
                worktree_path: j.worktree_path.clone(),
            })
            .collect();
        let _ = self.status_targets_tx.send(targets);
    }

    fn repo_path_for_job(&self, job: &Job) -> Option<std::path::PathBuf> {
        self.workspaces
            .iter()
            .find(|w| w.name == job.workspace)
            .and_then(|w| w.repos.iter().find(|r| r.name == job.repo))
            .map(|r| r.path.clone())
    }

    fn request_close_job(&mut self, job_id: &str) {
        let Some(job) = self.jobs.iter().find(|j| j.id == job_id).cloned() else {
            return;
        };
        let Some(repo_path) = self.repo_path_for_job(&job) else {
            self.last_error = Some(format!(
                "no se encontro el repo {} para el job {}",
                job.repo, job.id
            ));
            return;
        };
        if job.files_changed > 0 {
            self.close_confirm = Some(CloseConfirm {
                job_id: job.id.clone(),
                repo_path,
                worktree_path: job.worktree_path.clone(),
                files_changed: job.files_changed,
            });
        } else {
            self.send_close_job(&job.id, &repo_path, &job.worktree_path, false);
        }
    }

    fn send_close_job(
        &self,
        job_id: &str,
        repo_path: &std::path::Path,
        worktree_path: &std::path::Path,
        force: bool,
    ) {
        worker::spawn_remove_worktree(
            RemoveWorktreeRequest {
                job_id: job_id.to_string(),
                repo_path: repo_path.to_path_buf(),
                worktree_path: worktree_path.to_path_buf(),
                force,
            },
            self.worker_tx.clone(),
        );
    }

    fn render_close_confirm(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.close_confirm.as_ref() else {
            return;
        };
        let theme = &self.theme;
        let frame = egui::Frame::new()
            .fill(theme.bg_surface)
            .inner_margin(egui::Margin::same(20))
            .corner_radius(egui::CornerRadius::same(8))
            .stroke(egui::Stroke::new(1.0, theme.border));

        let mut action: Option<CloseConfirmAction> = None;
        let modal_response = egui::Modal::new(egui::Id::new("close_confirm_modal"))
            .frame(frame)
            .show(ctx, |ui| {
                ui.set_min_width(380.0);
                ui.set_max_width(520.0);
                ui.heading("Cerrar trabajo");
                ui.add_space(12.0);
                ui.label(format!(
                    "Tienes {} archivos modificados sin commitear en esta rama.",
                    confirm.files_changed
                ));
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "Si descartas, el worktree se elimina pero la rama queda intacta. \
                         Los cambios sin commitear se pierden.",
                    )
                    .small()
                    .color(theme.text_muted),
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("Descartar y cerrar")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            action = Some(CloseConfirmAction::Force);
                        }
                        ui.add_space(8.0);
                        if ui
                            .button("Cancelar")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            action = Some(CloseConfirmAction::Cancel);
                        }
                    });
                });
            });

        if modal_response.backdrop_response.clicked() {
            action = Some(CloseConfirmAction::Cancel);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            action = Some(CloseConfirmAction::Cancel);
        }

        match action {
            Some(CloseConfirmAction::Force) => {
                let c = self.close_confirm.take().expect("checked above");
                self.send_close_job(&c.job_id, &c.repo_path, &c.worktree_path, true);
            }
            Some(CloseConfirmAction::Cancel) => {
                self.close_confirm = None;
            }
            None => {}
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty_since = Some(Instant::now());
    }

    fn snapshot_for_persistence(&self) -> AppState {
        AppState {
            workspaces: self.workspaces.clone(),
            jobs: self.jobs.clone(),
            selected_job_id: self.selected_job_id.clone(),
            collapsed_workspaces: self.collapsed_workspaces.clone(),
            collapsed_repos: self.collapsed_repos.clone(),
        }
    }

    fn maybe_persist(&mut self) {
        let Some(since) = self.dirty_since else {
            return;
        };
        if since.elapsed() < SAVE_DEBOUNCE {
            return;
        }
        let snapshot = self.snapshot_for_persistence();
        if let Err(e) = snapshot.save() {
            warn!("no se pudo persistir state.json: {e:#}");
        }
        self.dirty_since = None;
    }

    fn open_new_job_modal(&mut self) {
        self.new_job_modal_state = NewJobModalState::initial();
        self.last_error = None;
        self.new_job_modal_open = true;
    }

    fn close_new_job_modal(&mut self) {
        self.new_job_modal_open = false;
    }

    /// Drena los eventos del worker que llegaron desde el último frame.
    fn drain_worker_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.worker_rx.try_recv() {
            match event {
                WorkerEvent::WorktreeCreated(job) => {
                    self.creating_worktree = false;
                    let id = job.id.clone();
                    self.jobs.push(job);
                    self.selected_job_id = Some(id);
                    self.last_error = None;
                    self.close_new_job_modal();
                    self.mark_dirty();
                    self.push_status_targets();
                }
                WorkerEvent::WorktreeFailed { message } => {
                    self.creating_worktree = false;
                    self.last_error = Some(message);
                }
                WorkerEvent::WorktreeRemoved { job_id } => {
                    self.jobs.retain(|j| j.id != job_id);
                    if self.selected_job_id.as_deref() == Some(job_id.as_str()) {
                        self.selected_job_id = self.jobs.first().map(|j| j.id.clone());
                    }
                    self.last_error = None;
                    self.mark_dirty();
                    self.push_status_targets();
                }
                WorkerEvent::WorktreeRemoveFailed { job_id: _, message } => {
                    self.last_error = Some(message);
                }
                WorkerEvent::JobFilesChanged {
                    job_id,
                    files_changed,
                } => {
                    if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id)
                        && job.files_changed != files_changed
                    {
                        job.files_changed = files_changed;
                        self.mark_dirty();
                    }
                }
            }
            ctx.request_repaint();
        }
    }

    /// Abre el file dialog nativo del OS y, si el usuario elige una carpeta,
    /// la registra como nuevo workspace (descubriendo repos hijos con `.git/`).
    fn pick_and_add_workspace(&mut self) {
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Selecciona la carpeta del workspace")
            .pick_folder()
        else {
            return;
        };
        let workspace = Workspace::from_path(&folder);
        let id = workspace.id.clone();
        self.workspaces.push(workspace);
        self.new_job_modal_state.workspace_id = Some(id);
        self.new_job_modal_state.repo_id = None;
        self.mark_dirty();
    }

    fn submit_new_job(&mut self) {
        let request = match self.build_create_request() {
            Some(r) => r,
            None => {
                warn!("submit_new_job sin workspace/repo valido");
                return;
            }
        };
        self.creating_worktree = true;
        self.last_error = None;
        worker::spawn_create_worktree(request, self.worker_tx.clone());
    }

    fn build_create_request(&self) -> Option<CreateWorktreeRequest> {
        let state = &self.new_job_modal_state;
        let ws_id = state.workspace_id.as_deref()?;
        let repo_id = state.repo_id.as_deref()?;
        let workspace = self.workspaces.iter().find(|w| w.id == ws_id)?;
        let repo = workspace.repos.iter().find(|r| r.id == repo_id)?;
        Some(CreateWorktreeRequest {
            workspace_name: workspace.name.clone(),
            workspace_path: workspace.path.clone(),
            repo_name: repo.name.clone(),
            repo_path: repo.path.clone(),
            branch: state.branch.clone(),
            base_branch: state.base_branch.clone(),
        })
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
        self.mark_dirty();
    }

    fn clear_jobs(&mut self) {
        self.workspaces.clear();
        self.jobs.clear();
        self.selected_job_id = None;
        self.mark_dirty();
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
        let ctx = ui.ctx().clone();
        // egui crea popups (ComboBox dropdown, tooltips, menus) como Areas
        // independientes que NO heredan los Visuals locales del Ui padre. Para
        // que esos popups respeten el theme dark hay que mantener los Visuals
        // sincronizados en el Context cada frame. Es una struct copy, barato.
        ctx.set_visuals(self.theme.build_visuals());

        self.drain_worker_events(&ctx);
        self.drain_pty_events();
        self.maybe_persist();

        if self.dirty_since.is_some() {
            ctx.request_repaint_after(SAVE_DEBOUNCE);
        }

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

        let selected_id = self.selected_job_id.clone();
        let mut empty_state_create_clicked = false;
        let mut close_clicked: Option<String> = None;
        egui::CentralPanel::default()
            .frame(self.theme.base_panel_frame())
            .show_inside(ui, |ui| {
                if self.jobs.is_empty() {
                    empty_state_create_clicked = render_empty_state(ui);
                } else if let Some(id) = selected_id {
                    close_clicked = self.render_selected_job(ui, &id);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Selecciona un trabajo de la barra lateral");
                    });
                }
            });
        if empty_state_create_clicked {
            self.open_new_job_modal();
        }
        if let Some(id) = close_clicked {
            self.request_close_job(&id);
        }

        self.render_close_confirm(ui.ctx());
        self.render_start_choice(ui.ctx());

        if self.new_job_modal_open {
            let action = new_job_modal::show(
                ui.ctx(),
                &mut self.new_job_modal_state,
                &self.workspaces,
                &self.theme,
                self.creating_worktree,
                self.last_error.as_deref(),
            );
            match action {
                ModalAction::Submit => {
                    self.submit_new_job();
                }
                ModalAction::Cancel => {
                    if !self.creating_worktree {
                        self.close_new_job_modal();
                    }
                }
                ModalAction::PickWorkspace => {
                    self.pick_and_add_workspace();
                }
                ModalAction::None => {}
            }
        }
    }
}

impl App {
    /// Render del job seleccionado: header + terminal embebido. Devuelve el
    /// id del job si el usuario hizo click en X (para que el caller dispare
    /// el close flow).
    fn render_selected_job(&mut self, ui: &mut egui::Ui, job_id: &str) -> Option<String> {
        let job = self.jobs.iter().find(|j| j.id == job_id).cloned()?;

        let outcome = render_job_header(ui, &job, &self.theme);

        let ctx = ui.ctx().clone();
        match self.ensure_terminal_for(&ctx, job_id) {
            Some(terminal) => {
                let size = ui.available_size();
                let view = TerminalView::new(ui, &mut terminal.backend)
                    .set_focus(true)
                    .set_size(size);
                ui.add(view);
            }
            None => {
                let rect = ui.available_rect_before_wrap();
                ui.painter().rect_filled(rect, 0.0, self.theme.bg_base);
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("terminal no disponible — ver logs")
                        .color(self.theme.status_error),
                );
            }
        }

        outcome.close_clicked.then(|| job.id.clone())
    }

    fn render_sidebar_tree(&mut self, ui: &mut egui::Ui) {
        let mut clicked_id: Option<String> = None;
        let mut toggle_ws: Option<String> = None;
        let mut toggle_repo: Option<String> = None;
        let mut add_workspace_clicked = false;
        let mut start_choice_for: Option<StartChoice> = None;

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

                            let header_outcome = repo_header(
                                ui,
                                &self.theme,
                                &repo.name,
                                repo_collapsed,
                                repo_jobs.len(),
                            );
                            if header_outcome.toggle_clicked {
                                toggle_repo = Some(repo.id.clone());
                            }
                            if header_outcome.play_clicked {
                                start_choice_for = Some(StartChoice {
                                    workspace_id: ws.id.clone(),
                                    workspace_name: ws.name.clone(),
                                    repo_name: repo.name.clone(),
                                    repo_path: repo.path.clone(),
                                });
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

                // Boton secundario al final de la lista: anadir otro workspace.
                ui.add_space(16.0);
                ui.separator();
                ui.add_space(8.0);
                let add_btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new("+ Anadir workspace").color(self.theme.text_muted),
                    )
                    .frame(false),
                );
                if add_btn
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Selecciona la carpeta padre donde estan tus repos")
                    .clicked()
                {
                    add_workspace_clicked = true;
                }
                ui.add_space(8.0);
            });

        if let Some(id) = clicked_id {
            self.selected_job_id = Some(id);
            self.mark_dirty();
        }
        if let Some(id) = toggle_ws {
            if !self.collapsed_workspaces.remove(&id) {
                self.collapsed_workspaces.insert(id);
            }
            self.mark_dirty();
        }
        if let Some(id) = toggle_repo {
            if !self.collapsed_repos.remove(&id) {
                self.collapsed_repos.insert(id);
            }
            self.mark_dirty();
        }
        if add_workspace_clicked {
            self.pick_and_add_workspace();
        }
        if let Some(choice) = start_choice_for {
            self.start_choice = Some(choice);
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

/// Resultado de interactuar con el header de un repo.
struct RepoHeaderOutcome {
    /// El usuario hizo click en el area del header (chevron + nombre) →
    /// toggle de colapsado.
    toggle_clicked: bool,
    /// El usuario hizo click en ▶ → quiere iniciar un trabajo en este repo.
    play_clicked: bool,
}

fn repo_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    collapsed: bool,
    job_count: usize,
) -> RepoHeaderOutcome {
    let full_width = ui.available_width();
    let (rect, area_response) = ui.allocate_exact_size(
        egui::vec2(full_width, theme.repo_header_height),
        egui::Sense::hover(),
    );

    if area_response.hovered() {
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

    let toggle_resp = child.add(
        egui::Label::new(
            egui::RichText::new(format!("{} {}", chevron, name)).color(theme.text_repo_label),
        )
        .sense(egui::Sense::click()),
    );
    let toggle_resp = toggle_resp.on_hover_cursor(egui::CursorIcon::PointingHand);

    // Boton ▶ visible solo en hover del row (Linear-style: revela acciones)
    let mut play_clicked = false;
    if area_response.hovered() {
        child.add_space(8.0);
        let play_resp = child
            .add(
                egui::Button::new(egui::RichText::new("\u{25B6}").small().color(theme.accent))
                    .frame(false),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Iniciar trabajo en este repo");
        if play_resp.clicked() {
            play_clicked = true;
        }
    }

    if job_count > 0 {
        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{}", job_count))
                    .small()
                    .color(theme.text_muted),
            );
        });
    }

    RepoHeaderOutcome {
        toggle_clicked: toggle_resp.clicked(),
        play_clicked,
    }
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

struct JobPaneOutcome {
    close_clicked: bool,
}

fn render_job_header(ui: &mut egui::Ui, job: &Job, theme: &Theme) -> JobPaneOutcome {
    let mut outcome = JobPaneOutcome {
        close_clicked: false,
    };
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
                    if ui
                        .button("X")
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Cerrar trabajo y eliminar worktree")
                        .clicked()
                    {
                        outcome.close_clicked = true;
                    }
                    let _ = ui
                        .button("Commit & Push")
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    let _ = ui
                        .button("Diff")
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                });
            });
        });
    outcome
}
