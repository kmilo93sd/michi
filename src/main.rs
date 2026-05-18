use anyhow::{Context, Result};
use tracing::{info, warn};
use tracing_appender::non_blocking::WorkerGuard;

use michi::app;

const ICON_PNG: &[u8] = include_bytes!("../assets/icon-256.png");

fn main() -> Result<()> {
    let _log_guard = init_tracing().context("inicializando tracing")?;
    info!("michi starting up");

    let viewport = build_viewport();

    let native_options = eframe::NativeOptions {
        viewport,
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

fn build_viewport() -> egui::ViewportBuilder {
    let mut builder = egui::ViewportBuilder::default()
        .with_title("michi")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([800.0, 600.0]);

    match load_icon() {
        Ok(icon) => builder = builder.with_icon(icon),
        Err(e) => warn!("no se pudo cargar el icono embebido: {e:#}"),
    }

    builder
}

fn load_icon() -> Result<egui::IconData> {
    let img = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)
        .context("decodificando icon-256.png")?
        .into_rgba8();
    let (width, height) = img.dimensions();
    Ok(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
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
