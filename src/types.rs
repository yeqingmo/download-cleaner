use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

pub type Memory = BTreeMap<String, BTreeMap<String, u64>>;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub downloads_dir: PathBuf,
    pub memory_path: PathBuf,
    pub complete_delay: Duration,
    pub batch_window: Duration,
    pub scan_existing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileSignature {
    pub path: PathBuf,
    pub size: u64,
    pub modified_ms: u128,
}

#[derive(Debug, Clone)]
pub struct ReadyDownload {
    pub path: PathBuf,
    pub file_name: String,
    pub domain: String,
    pub modified_ms: u128,
}

#[derive(Debug, Clone)]
pub enum UserChoice {
    Ignore,
    MoveTo(PathBuf),
    ChooseOther,
}

#[derive(Debug, Clone)]
pub enum BatchChoice {
    IgnoreAll,
    MoveAllTo(PathBuf),
    ChooseOtherAll,
    OneByOne,
}
