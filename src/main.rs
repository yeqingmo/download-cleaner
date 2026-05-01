mod config;
mod daemon;
mod file_ops;
mod gui_panel;
mod manager;
mod memory;
mod metadata;
mod pathing;
mod types;
mod ui;

use anyhow::Result;
use std::env;
use types::AppConfig;

fn main() -> Result<()> {
    let config = AppConfig::from_env()?;
    let mode = env::args().nth(1);

    if matches!(mode.as_deref(), Some("panel")) {
        gui_panel::run_gui(&config)
    } else if matches!(mode.as_deref(), Some("panel-script")) {
        manager::run_manager(&config)
    } else if matches!(mode.as_deref(), Some("panel-summary")) {
        let memory = memory::read_memory(&config.memory_path)?;
        ui::run_native_panel(&config, &memory)
    } else {
        daemon::run(config)
    }
}
