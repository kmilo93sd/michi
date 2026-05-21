#!/usr/bin/env bash
# Bootstrap script para michi
# Corre desde C:\Users\kmilo\Documents\projects\michi después de cargo init --bin
#
# Cada `cargo add` agarra la última versión estable de crates.io.
# No hardcodear versiones — confiar en cargo + Cargo.lock para reproducibilidad.

set -euo pipefail

# UI nativa
cargo add eframe --features "default_fonts,glow"
cargo add egui

# Async / concurrencia
cargo add tokio --features "rt,macros,sync,time,process,io-util"

# Errores
cargo add anyhow

# Serialización
cargo add serde --features "derive"
cargo add serde_json

# Logging estructurado
cargo add tracing
cargo add tracing-subscriber --features "env-filter"
cargo add tracing-appender

# Cross-platform paths
cargo add dirs

# Las siguientes se agregan en Fase 4 (terminal embebido), no ahora:
# cargo add portable-pty
# cargo add alacritty_terminal

echo ""
echo "Dependencies agregadas. Versiones finales:"
cargo tree --depth 0
