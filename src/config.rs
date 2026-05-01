use crate::pathing::{expand_home, home_dir};
use crate::types::AppConfig;
use anyhow::Result;
use std::env;
use std::path::PathBuf;
use std::time::Duration;

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let home = home_dir()?;
        let downloads_dir = env::var_os("DOWNLOADS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("Downloads"));
        let memory_path = env::var_os("MEMORY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config").join("smart_dl_memory.json"));
        let complete_delay_ms = env::var("COMPLETE_DELAY_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(500);
        let batch_window_ms = env::var("BATCH_WINDOW_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1_500);

        Ok(Self {
            downloads_dir: expand_home(downloads_dir)?,
            memory_path: expand_home(memory_path)?,
            complete_delay: Duration::from_millis(complete_delay_ms),
            batch_window: Duration::from_millis(batch_window_ms),
            scan_existing: env::var("SCAN_EXISTING").ok().as_deref() == Some("1"),
        })
    }
}
