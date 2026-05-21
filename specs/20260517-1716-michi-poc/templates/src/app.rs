use eframe::CreationContext;
use egui::Context;

pub struct App {
    // Placeholder. Fase 2 llena con jobs, selected_job_id, etc.
}

impl App {
    pub fn new(_cc: &CreationContext<'_>) -> Self {
        Self {}
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("michi");
            ui.label("Bootstrap OK. UI completa llega en Fase 2.");
        });
    }
}
