use anyhow::{Context, Result};
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;

mod app;
mod git;
mod state;
mod terminal;
mod theme;
mod ui;

fn main() -> Result<()> {
    let _log_guard = init_tracing().context("inicializando tracing")?;
    info!("michi starting up");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("michi")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "michi",
        native_options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;

    Ok(())
}

fn init_tracing() -> Result<WorkerGuard> {
    let log_dir = dirs::home_dir()
        .context("no se pudo obtener home dir")?
        .join(".michi")
        .join("logs");
    std::fs::create_dir_all(&log_dir)
        .with_context(|| format!("creando log dir {}", log_dir.display()))?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, "michi.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("michi=info")),
        )
        .init();

    Ok(guard)
}
