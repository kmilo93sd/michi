use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::CreationContext;
use egui_term::{PtyEvent, TerminalView};
use tracing::{debug, warn};

use crate::state::{self, AppState, Job, JobStatus, Workspace};
use crate::system;
use crate::terminal::{self, JobTerminal};
use crate::theme::Theme;
use crate::ui::new_job_modal::{self, ModalAction, NewJobModalState};
use crate::worker::{
    self, CreateWorktreeRequest, RemoveWorktreeRequest, StatusPollTarget, WorkerEvent,
};
use crate::workspace_prep;

/// Cuánto esperar tras el último cambio antes de persistir el state al disco.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

/// Cada cuánto el worker poller hace `git status` por job.
const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub struct App {
    pub workspaces: Vec<Workspace>,
    pub jobs: Vec<Job>,
    pub selected_job_id: Option<String>,
    pub collapsed_workspaces: HashSet<String>,
    pub theme: Theme,
    pub new_job_modal_open: bool,
    pub new_job_modal_state: NewJobModalState,
    pub creating_worktree: bool,
    pub last_error: Option<String>,
    /// Mensaje positivo efimero (preparacion exitosa, etc).
    pub last_info: Option<String>,
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

/// El usuario hizo click en ▶ de un workspace o repo. Mostrar dialogo
/// "directa | worktree". Si `repo` es `None`, el usuario inicio desde el
/// workspace header (sesion directa = cwd workspace, worktree = elegir repo).
/// Si es `Some(..)`, ya tiene el repo elegido.
struct StartChoice {
    workspace_id: String,
    workspace_name: String,
    workspace_path: std::path::PathBuf,
    repo: Option<RepoChoice>,
}

struct RepoChoice {
    id: String,
    name: String,
    path: std::path::PathBuf,
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

/// Acciones del menu contextual de un workspace en sidebar (click derecho).
#[derive(Clone, Copy)]
enum WorkspaceMenu {
    DirectSession,
    NewWorktree,
    OpenFolder,
    PrepareWorkspace,
    Remove,
}

/// Acciones del banner "Preparar workspace" que aparece en la card cuando
/// `workspace_prep::inspect(...).is_bare()` y el usuario no lo descarto.
#[derive(Clone, Copy)]
enum PrepBannerAction {
    Prepare,
    Dismiss,
}

/// Acciones del menu contextual de una card de job en sidebar.
#[derive(Clone, Copy)]
enum JobMenu {
    Select,
    OpenFolder,
    Close,
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
            theme,
            new_job_modal_open: false,
            new_job_modal_state: NewJobModalState::initial(),
            creating_worktree: false,
            last_error: None,
            last_info: None,
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

    /// Modal "¿Sesion directa o nuevo worktree?". Soporta dos modos:
    /// - Sin repo (click en ▶ del workspace): directa = cwd workspace,
    ///   worktree = abrir modal Nuevo trabajo con workspace pre-llenado
    ///   (usuario elige repo + branch).
    /// - Con repo (click en ▶ de un repo): directa = cwd repo,
    ///   worktree = abrir modal con workspace+repo pre-llenados.
    fn render_start_choice(&mut self, ctx: &egui::Context) {
        let Some(choice) = self.start_choice.as_ref() else {
            return;
        };
        let workspace_id = choice.workspace_id.clone();
        let workspace_name = choice.workspace_name.clone();
        let workspace_path = choice.workspace_path.clone();
        let repo = choice.repo.as_ref().map(|r| RepoChoice {
            id: r.id.clone(),
            name: r.name.clone(),
            path: r.path.clone(),
        });
        let scope_label = match &repo {
            Some(r) => format!("{} / {}", workspace_name, r.name),
            None => format!("{} (workspace)", workspace_name),
        };
        let direct_tooltip = if repo.is_some() {
            "Claude corre en el repo tal cual, sin crear branch nueva."
        } else {
            "Claude corre en el workspace con acceso a todos los repos hijos. Sin git operations."
        };
        let direct_subline = if repo.is_some() {
            "Claude corre en la branch actual del repo, sin worktree separado."
        } else {
            "Claude tiene contexto del workspace completo (todos los repos)."
        };
        let worktree_tooltip = if repo.is_some() {
            "Crea una rama nueva y un git worktree dedicado en este repo."
        } else {
            "Abre el modal Nuevo trabajo con el workspace pre-llenado. Eliges repo y branch."
        };
        let worktree_subline = if repo.is_some() {
            "Abre el modal con workspace y repo pre-llenados."
        } else {
            "Abre el modal con workspace pre-llenado. Eliges el repo."
        };

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
                ui.set_min_width(460.0);
                ui.set_max_width(580.0);

                ui.heading("Iniciar trabajo");
                ui.add_space(4.0);
                ui.label(egui::RichText::new(scope_label).color(theme.text_muted));
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
                    .on_hover_text(direct_tooltip)
                    .clicked()
                {
                    action = Some(StartChoiceAction::Direct);
                }
                ui.add_space(6.0);
                ui.small(egui::RichText::new(direct_subline).color(theme.text_muted));

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
                    .on_hover_text(worktree_tooltip)
                    .clicked()
                {
                    action = Some(StartChoiceAction::Worktree);
                }
                ui.add_space(6.0);
                ui.small(egui::RichText::new(worktree_subline).color(theme.text_muted));

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
                match repo {
                    Some(r) => self.start_direct_session(&workspace_name, &r.name, &r.path),
                    None => self.start_workspace_session(&workspace_name, &workspace_path),
                }
            }
            Some(StartChoiceAction::Worktree) => {
                self.start_choice = None;
                let repo_id_opt = repo.as_ref().map(|r| r.id.clone());
                self.open_new_job_modal_for_scope(&workspace_id, repo_id_opt.as_deref());
            }
            Some(StartChoiceAction::Cancel) => {
                self.start_choice = None;
            }
            None => {}
        }
    }

    /// Inicia una sesion de workspace: claude corre en workspace.path con
    /// acceso a todos los repos hijos. No hay git worktree.
    fn start_workspace_session(&mut self, workspace_name: &str, workspace_path: &std::path::Path) {
        let job = Job::for_workspace_session(workspace_name, workspace_path);
        let id = job.id.clone();
        self.jobs.push(job);
        self.selected_job_id = Some(id);
        self.last_error = None;
        self.mark_dirty();
        self.push_status_targets();
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
    /// el modal "Nuevo trabajo" con el workspace y (opcionalmente) repo.
    fn open_new_job_modal_for_scope(&mut self, workspace_id: &str, repo_id: Option<&str>) {
        self.new_job_modal_state = NewJobModalState::initial();
        self.new_job_modal_state.workspace_id = Some(workspace_id.to_string());
        self.new_job_modal_state.repo_id = repo_id.map(str::to_string);
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
        // Sesion directa o de workspace: no hay worktree dedicado, solo
        // borramos la entrada en memoria. No tocamos git.
        if job.is_in_place_session() {
            self.close_in_place_session(&job.id);
            return;
        }
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

    /// Cierra una sesion in-place (directa o workspace): quita el job de la
    /// lista en memoria y libera el terminal embebido. No corre `git worktree
    /// remove` porque no hay worktree dedicado.
    fn close_in_place_session(&mut self, job_id: &str) {
        self.jobs.retain(|j| j.id != job_id);
        self.terminals.remove(job_id);
        if self.selected_job_id.as_deref() == Some(job_id) {
            self.selected_job_id = self.jobs.first().map(|j| j.id.clone());
        }
        self.last_error = None;
        self.mark_dirty();
        self.push_status_targets();
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

    fn jobs_for_workspace(&self, workspace: &str) -> Vec<&Job> {
        self.jobs
            .iter()
            .filter(|j| j.workspace == workspace)
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
                    // Si hay un mensaje activo (error rojo o info accent), reemplaza
                    // los hints de shortcuts. Click en X lo cierra. Sin auto-dismiss
                    // para que el usuario tenga tiempo de leerlo.
                    let mut dismiss_message = false;
                    if let Some(msg) = self.last_error.as_deref() {
                        ui.small(egui::RichText::new(msg).color(self.theme.status_error));
                        if ui
                            .small_button("X")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            dismiss_message = true;
                        }
                        if dismiss_message {
                            self.last_error = None;
                        }
                    } else if let Some(msg) = self.last_info.as_deref() {
                        ui.small(egui::RichText::new(msg).color(self.theme.accent));
                        if ui
                            .small_button("X")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            dismiss_message = true;
                        }
                        if dismiss_message {
                            self.last_info = None;
                        }
                    } else {
                        ui.small(
                            "Ctrl+N nuevo \u{B7} Ctrl+Tab siguiente \u{B7} \
                             Ctrl+Shift+Tab anterior \u{B7} Ctrl+W cerrar trabajo",
                        );
                    }
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
                // Sin workspaces no se puede crear un trabajo: el CTA
                // primario vive abajo en el tree ("+ Anadir workspace") y
                // tambien en el empty state central. No mostramos el boton
                // aqui para evitar que el usuario abra un modal inutil.
                if !self.workspaces.is_empty() {
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
                }

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

                // El sidebar tree se renderiza siempre: maneja la lista
                // vacia internamente y, sobre todo, deja accesible el boton
                // "+ Anadir workspace" tambien cuando todavia no hay ninguno.
                self.render_sidebar_tree(ui);
            });

        let selected_id = self.selected_job_id.clone();
        let mut empty_state_action = EmptyStateAction::None;
        let mut close_clicked: Option<String> = None;
        let has_workspaces = !self.workspaces.is_empty();
        egui::CentralPanel::default()
            .frame(self.theme.base_panel_frame())
            .show_inside(ui, |ui| {
                if self.jobs.is_empty() {
                    empty_state_action = render_empty_state(ui, has_workspaces);
                } else if let Some(id) = selected_id {
                    close_clicked = self.render_selected_job(ui, &id);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Selecciona un trabajo de la barra lateral");
                    });
                }
            });
        match empty_state_action {
            EmptyStateAction::CreateJob => self.open_new_job_modal(),
            EmptyStateAction::AddWorkspace => self.pick_and_add_workspace(),
            EmptyStateAction::None => {}
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
        let mut add_workspace_clicked = false;
        let mut start_choice_for: Option<StartChoice> = None;
        let mut workspace_menu_pick: Option<(Workspace, WorkspaceMenu)> = None;
        let mut job_menu_pick: Option<(String, JobMenu)> = None;
        let mut prep_action_for: Option<(String, PrepBannerAction)> = None;

        // Clone para evitar lifetime acrobatics: el loop necesita iterar workspaces
        // y simultaneamente leer self.collapsed_workspaces y jobs_for_workspace.
        let workspaces = self.workspaces.clone();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for ws in &workspaces {
                    ui.add_space(8.0);
                    let ws_collapsed = self.collapsed_workspaces.contains(&ws.id);
                    // Un solo inspect por workspace por frame. Lo reusamos
                    // para el check verde del header y el banner de bare.
                    let status = workspace_prep::inspect(&ws.path);
                    let ws_outcome = workspace_header(ui, &self.theme, ws, &status, ws_collapsed);
                    if ws_outcome.toggle_clicked {
                        toggle_ws = Some(ws.id.clone());
                    }
                    if ws_outcome.play_clicked {
                        start_choice_for = Some(StartChoice {
                            workspace_id: ws.id.clone(),
                            workspace_name: ws.name.clone(),
                            workspace_path: ws.path.clone(),
                            repo: None,
                        });
                    }
                    if let Some(pick) = ws_outcome.menu_pick {
                        workspace_menu_pick = Some((ws.clone(), pick));
                    }

                    // Banner "Preparar workspace" si esta bare y el usuario
                    // no lo descarto.
                    if !ws.prep_dismissed
                        && status.is_bare()
                        && let Some(action) = prep_banner(ui, &self.theme)
                    {
                        prep_action_for = Some((ws.id.clone(), action));
                    }

                    if !ws_collapsed {
                        let ws_jobs = self.jobs_for_workspace(&ws.name);
                        if ws_jobs.is_empty() {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.add_space(self.theme.tree_line_ws_x + 8.0);
                                ui.small(
                                    egui::RichText::new("Sin trabajos activos")
                                        .color(self.theme.text_muted),
                                );
                            });
                            ui.add_space(4.0);
                        } else {
                            for job in ws_jobs {
                                let selected = self.selected_job_id.as_deref() == Some(&job.id);
                                let outcome = render_job_card(ui, job, selected, &self.theme);
                                if outcome.clicked {
                                    clicked_id = Some(job.id.clone());
                                }
                                if let Some(pick) = outcome.menu_pick {
                                    job_menu_pick = Some((job.id.clone(), pick));
                                }
                            }
                        }
                    }
                }

                // Cuando no hay workspaces, el CTA principal es anadir uno:
                // sin workspace no se puede crear ningun trabajo, asi que
                // mostramos un boton prominente en vez del link discreto.
                if workspaces.is_empty() {
                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| {
                        ui.small(
                            egui::RichText::new("Aun no hay workspaces.")
                                .color(self.theme.text_muted),
                        );
                    });
                    ui.add_space(8.0);
                    let primary = ui.add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new("+ Anadir workspace"),
                    );
                    if primary
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("Selecciona la carpeta padre donde estan tus repos")
                        .clicked()
                    {
                        add_workspace_clicked = true;
                    }
                } else {
                    // Boton secundario al final de la lista cuando ya hay
                    // al menos un workspace: link discreto, sin frame.
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
                }
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
        if add_workspace_clicked {
            self.pick_and_add_workspace();
        }
        if let Some(choice) = start_choice_for {
            self.start_choice = Some(choice);
        }
        if let Some((ws, action)) = workspace_menu_pick {
            self.handle_workspace_menu(&ws, action);
        }
        if let Some((job_id, action)) = job_menu_pick {
            self.handle_job_menu(&job_id, action);
        }
        if let Some((ws_id, action)) = prep_action_for {
            match action {
                PrepBannerAction::Prepare => self.prepare_workspace_recommended(&ws_id),
                PrepBannerAction::Dismiss => self.dismiss_workspace_prep(&ws_id),
            }
        }
    }

    /// Aplica la preparacion recomendada del workspace identificado por
    /// `workspace_id`: lee su path, inspecciona que falta, crea lo que falta
    /// y reporta inline. No sobreescribe archivos existentes (cubierto por
    /// `workspace_prep::prepare`).
    fn prepare_workspace_recommended(&mut self, workspace_id: &str) {
        let Some(path) = self
            .workspaces
            .iter()
            .find(|w| w.id == workspace_id)
            .map(|w| w.path.clone())
        else {
            return;
        };
        let status = workspace_prep::inspect(&path);
        let opts = workspace_prep::PrepareOpts::recommended_for(&status);
        match workspace_prep::prepare(&path, opts) {
            Ok(report) => {
                let mut summary: Vec<&str> = Vec::new();
                if !report.created.is_empty() {
                    summary.push("scaffolding");
                }
                if report.git_initialized {
                    summary.push("git init");
                }
                if summary.is_empty() {
                    self.last_info = Some("Workspace ya estaba preparado.".into());
                } else {
                    self.last_info = Some(format!("Workspace preparado: {}.", summary.join(", ")));
                }
                self.last_error = None;
                // Refrescar el snapshot del workspace en memoria (claude_md_present,
                // specs_count, skills_count cambian tras crear scaffolding).
                if let Some(w) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
                    let refreshed = Workspace::from_path(&w.path);
                    w.claude_md_present = refreshed.claude_md_present;
                    w.specs_count = refreshed.specs_count;
                    w.skills_count = refreshed.skills_count;
                    w.repos = refreshed.repos;
                }
                self.mark_dirty();
            }
            Err(e) => {
                self.last_error = Some(format!("no se pudo preparar workspace: {e:#}"));
            }
        }
    }

    /// Marca el banner "Preparar workspace" como descartado en este workspace.
    /// La accion sigue accesible desde el context menu.
    fn dismiss_workspace_prep(&mut self, workspace_id: &str) {
        if let Some(w) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
            w.prep_dismissed = true;
            self.mark_dirty();
        }
    }

    fn handle_workspace_menu(&mut self, ws: &Workspace, action: WorkspaceMenu) {
        match action {
            WorkspaceMenu::DirectSession => {
                self.start_workspace_session(&ws.name, &ws.path);
            }
            WorkspaceMenu::NewWorktree => {
                self.open_new_job_modal_for_scope(&ws.id, None);
            }
            WorkspaceMenu::OpenFolder => {
                if let Err(e) = system::open_folder(&ws.path) {
                    self.last_error = Some(format!("no se pudo abrir la carpeta: {e:#}"));
                }
            }
            WorkspaceMenu::PrepareWorkspace => {
                self.prepare_workspace_recommended(&ws.id);
            }
            WorkspaceMenu::Remove => {
                let affected =
                    state::workspace::remove_workspace(&mut self.workspaces, &self.jobs, &ws.id);
                if !affected.is_empty() {
                    self.jobs.retain(|j| !affected.contains(&j.id));
                    self.terminals.retain(|jid, _| !affected.contains(jid));
                    if let Some(sel) = &self.selected_job_id
                        && affected.contains(sel)
                    {
                        self.selected_job_id = self.jobs.first().map(|j| j.id.clone());
                    }
                    self.push_status_targets();
                }
                self.mark_dirty();
            }
        }
    }

    fn handle_job_menu(&mut self, job_id: &str, action: JobMenu) {
        match action {
            JobMenu::Select => {
                self.selected_job_id = Some(job_id.to_string());
                self.mark_dirty();
            }
            JobMenu::OpenFolder => {
                let path = self
                    .jobs
                    .iter()
                    .find(|j| j.id == job_id)
                    .map(|j| j.worktree_path.clone());
                if let Some(p) = path
                    && let Err(e) = system::open_folder(&p)
                {
                    self.last_error = Some(format!("no se pudo abrir la carpeta: {e:#}"));
                }
            }
            JobMenu::Close => {
                self.request_close_job(job_id);
            }
        }
    }
}

/// Resultado de interactuar con el header de un workspace.
struct WorkspaceHeaderOutcome {
    /// Toggle de colapsado (click en chevron + nombre).
    toggle_clicked: bool,
    /// El usuario hizo click en ▶ → quiere iniciar un trabajo en este workspace.
    play_clicked: bool,
    /// Click derecho en el row eligio una accion del menu contextual.
    menu_pick: Option<WorkspaceMenu>,
}

fn workspace_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    ws: &Workspace,
    status: &workspace_prep::WorkspacePreparationStatus,
    collapsed: bool,
) -> WorkspaceHeaderOutcome {
    let full_width = ui.available_width();
    let (rect, area_response) = ui.allocate_exact_size(
        egui::vec2(full_width, theme.workspace_header_height),
        egui::Sense::click(),
    );

    // Usamos contains_pointer en vez de hovered: hovered se hace false cuando
    // el cursor entra en un widget hijo con sense propio (el boton "+"), lo
    // que provocaba flicker cuando el "+" aparecia/desaparecia.
    let row_hovered = area_response.contains_pointer();
    if row_hovered {
        ui.painter().rect_filled(rect, 4.0, theme.bg_card_hover);
    }
    // Cursor pointer en todo el row indica que es interactivo (toggle).
    let area_response = area_response.on_hover_cursor(egui::CursorIcon::PointingHand);

    // Mismo menu se asigna al row entero Y al "+": el "+" es un widget hijo
    // con Sense::click(), asi que el cursor sobre el "+" hace que egui le
    // direccione el secondary_clicked al "+" y NO al row padre. Para que el
    // click derecho funcione en cualquier zona del row (incluyendo el "+"),
    // adjuntamos el mismo menu en ambos targets.
    let mut menu_pick: Option<WorkspaceMenu> = None;
    let menu_builder = |ui: &mut egui::Ui, picked: &mut Option<WorkspaceMenu>| {
        if ui.button("Sesion directa del workspace").clicked() {
            *picked = Some(WorkspaceMenu::DirectSession);
            ui.close_kind(egui::UiKind::Menu);
        }
        if ui.button("Nuevo trabajo (worktree)").clicked() {
            *picked = Some(WorkspaceMenu::NewWorktree);
            ui.close_kind(egui::UiKind::Menu);
        }
        ui.separator();
        if ui.button("Preparar workspace").clicked() {
            *picked = Some(WorkspaceMenu::PrepareWorkspace);
            ui.close_kind(egui::UiKind::Menu);
        }
        if ui.button("Abrir carpeta").clicked() {
            *picked = Some(WorkspaceMenu::OpenFolder);
            ui.close_kind(egui::UiKind::Menu);
        }
        ui.separator();
        if ui.button("Quitar workspace").clicked() {
            *picked = Some(WorkspaceMenu::Remove);
            ui.close_kind(egui::UiKind::Menu);
        }
    };
    area_response.context_menu(|ui| menu_builder(ui, &mut menu_pick));

    let chevron = if collapsed { "\u{25B8}" } else { "\u{25BE}" };
    let inner = rect.shrink2(egui::vec2(6.0, 4.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );

    // Linea 1: chevron + nombre + dot de "configurado" a la izquierda,
    // "+" pegado a la derecha. El "+" esta SIEMPRE renderizado (sin
    // parpadeo). El dot verde aparece solo cuando el workspace no es bare.
    let mut play_clicked = false;
    child.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} {}", chevron, ws.name.to_uppercase()))
                .small()
                .color(theme.text_workspace_label),
        );
        if !status.is_bare() {
            ui.add_space(4.0);
            // U+25CF (filled circle) en lugar de checkmark: la fuente
            // monoespaciada del sidebar lo renderiza, el checkmark caia en
            // tofu. Color verde idle = mismo lenguaje visual que los status
            // dots de jobs activos.
            ui.label(
                egui::RichText::new("\u{25CF}")
                    .small()
                    .color(theme.status_idle),
            )
            .on_hover_text(workspace_status_summary(status));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (plus_clicked, plus_response) =
                plus_button(ui, theme, row_hovered, "Iniciar trabajo en este workspace");
            plus_response.context_menu(|ui| menu_builder(ui, &mut menu_pick));
            if plus_clicked {
                play_clicked = true;
            }
        });
    });

    // Linea 2: subtitulo con specs y skills
    child.label(
        egui::RichText::new(format!(
            "{} specs \u{B7} {} skills",
            ws.specs_count, ws.skills_count
        ))
        .small()
        .color(theme.text_muted),
    );

    // Toggle = click izquierdo en cualquier parte del row que no haya sido
    // capturada por el "+". Como el "+" se agrega despues del area_response,
    // egui priorizara su sense cuando el cursor este sobre el, y el row solo
    // recibira clicks fuera de el.
    let toggle_clicked = area_response.clicked();

    WorkspaceHeaderOutcome {
        toggle_clicked,
        play_clicked,
        menu_pick,
    }
}

/// Resumen humano para el tooltip del check verde "workspace configurado".
fn workspace_status_summary(status: &workspace_prep::WorkspacePreparationStatus) -> String {
    let mut parts = Vec::new();
    if status.has_claude_md {
        parts.push("CLAUDE.md");
    }
    if status.has_claude_dir {
        parts.push(".claude/");
    }
    if status.has_mcp_json {
        parts.push(".mcp.json");
    }
    if status.has_specs_dir {
        parts.push("specs/");
    }
    let detected = if parts.is_empty() {
        "sin artefactos".to_string()
    } else {
        parts.join(" \u{B7} ")
    };
    let git_line = if status.has_root_git {
        "Repo git en root."
    } else if status.has_child_git_dirs {
        "Carpeta padre con repos hijos git."
    } else {
        "Sin repo git."
    };
    format!("Workspace configurado:\n{detected}\n{git_line}")
}

/// Boton "+" reutilizable: presencia constante, color sutil en idle y accent
/// cuando el row padre esta hovereado. Devuelve `(clicked, response)`. El
/// caller usa `response` para adjuntar `context_menu` (asi el click derecho
/// sobre el "+" abre el mismo menu que sobre el resto del row).
fn plus_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    row_hovered: bool,
    tooltip: &str,
) -> (bool, egui::Response) {
    let color = if row_hovered {
        theme.accent
    } else {
        theme.text_muted
    };
    let resp = ui
        .add(
            egui::Button::new(
                egui::RichText::new("\u{002B}")
                    .strong()
                    .color(color)
                    .size(16.0),
            )
            .frame(false)
            .min_size(egui::vec2(22.0, 22.0)),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip);
    (resp.clicked(), resp)
}

/// Banner inline que aparece bajo el header de un workspace pelado.
/// Renderiza dos lineas: un texto sutil explicando el estado + una fila de
/// botones (Preparar / Ignorar). El boton "Personalizar" no esta en el MVP:
/// la accion recomendada cubre el 95% de los casos y mantiene la UX en 1
/// click. Si manana se necesita, se agrega un tercer boton aqui.
fn prep_banner(ui: &mut egui::Ui, theme: &Theme) -> Option<PrepBannerAction> {
    let mut action: Option<PrepBannerAction> = None;
    let frame = egui::Frame::new()
        .fill(theme.bg_card_hover)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .corner_radius(egui::CornerRadius::same(4));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            egui::RichText::new("Workspace sin contexto (CLAUDE.md / .claude / .mcp.json).")
                .small()
                .color(theme.text_muted),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("Preparar").color(theme.accent).strong())
                        .frame(false),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Crear CLAUDE.md, .claude/, specs/, .mcp.json y git init si aplica")
                .clicked()
            {
                action = Some(PrepBannerAction::Prepare);
            }
            ui.add_space(12.0);
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("Ignorar").color(theme.text_muted))
                        .frame(false),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Ocultar este banner. Sigue accesible desde el context menu.")
                .clicked()
            {
                action = Some(PrepBannerAction::Dismiss);
            }
        });
    });
    action
}

fn paint_tree_line(ui: &egui::Ui, rect: egui::Rect, theme: &Theme, offset_x: f32) {
    let x = rect.min.x + offset_x;
    ui.painter().line_segment(
        [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
        egui::Stroke::new(1.0, theme.border),
    );
}

struct JobCardOutcome {
    clicked: bool,
    menu_pick: Option<JobMenu>,
}

fn render_job_card(ui: &mut egui::Ui, job: &Job, selected: bool, theme: &Theme) -> JobCardOutcome {
    let full_width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(full_width, theme.card_row_height),
        egui::Sense::click(),
    );

    let mut menu_pick: Option<JobMenu> = None;
    response.context_menu(|ui| {
        if ui.button("Ir al trabajo").clicked() {
            menu_pick = Some(JobMenu::Select);
            ui.close_kind(egui::UiKind::Menu);
        }
        ui.separator();
        if ui.button("Abrir carpeta").clicked() {
            menu_pick = Some(JobMenu::OpenFolder);
            ui.close_kind(egui::UiKind::Menu);
        }
        ui.separator();
        if ui.button("Cerrar trabajo").clicked() {
            menu_pick = Some(JobMenu::Close);
            ui.close_kind(egui::UiKind::Menu);
        }
    });

    let row_hovered = response.contains_pointer();
    let bg = if selected {
        theme.bg_card_selected
    } else if row_hovered {
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

    let inner = rect.shrink2(egui::vec2(22.0, 6.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::LEFT)),
    );

    // Linea 1: status dot + branch a la izquierda, nombre del repo como tag
    // discreto a la derecha. Para sesiones in-place (directo/workspace) no
    // mostramos repo: el `(directo)` / `(workspace)` del branch ya lo aclara.
    child.horizontal(|ui| {
        ui.colored_label(job.status.color(theme), job.status.dot().to_string());
        ui.label(egui::RichText::new(&job.branch).strong());
        if !job.is_in_place_session() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(&job.repo)
                        .small()
                        .color(theme.text_muted),
                );
            });
        }
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

    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    JobCardOutcome {
        clicked: response.clicked(),
        menu_pick,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EmptyStateAction {
    None,
    CreateJob,
    AddWorkspace,
}

fn render_empty_state(ui: &mut egui::Ui, has_workspaces: bool) -> EmptyStateAction {
    let mut action = EmptyStateAction::None;
    ui.vertical_centered(|ui| {
        ui.add_space(96.0);
        if has_workspaces {
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
                action = EmptyStateAction::CreateJob;
            }
            ui.add_space(8.0);
            ui.small(egui::RichText::new("o Ctrl+N").weak());
        } else {
            ui.heading("Anade tu primer workspace");
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(
                    "Un workspace es la carpeta padre que contiene tus repos.\n\
                     Desde ahi michi puede crear worktrees y orquestar varios Claude Code en paralelo.",
                )
                .weak(),
            );
            ui.add_space(24.0);
            if ui
                .add_sized([220.0, 36.0], egui::Button::new("+ Anadir workspace"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                action = EmptyStateAction::AddWorkspace;
            }
        }
    });
    action
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
            // Titulo: para sesiones in-place mostramos solo "{workspace} ·
            // {branch}" (el repo es "(workspace)" o "(directo)" y no aporta).
            // Para jobs con worktree real, "{repo} · {branch}".
            let title = if job.is_in_place_session() {
                format!("{} \u{B7} {}", job.workspace, job.branch)
            } else {
                format!("{} \u{B7} {}", job.repo, job.branch)
            };
            ui.strong(title);
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
