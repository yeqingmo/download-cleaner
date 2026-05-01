use crate::pathing::same_path;
use crate::types::Memory;
use anyhow::{anyhow, Context, Result};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

const LOCK_STALE_AFTER: Duration = Duration::from_secs(30);
const LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(50);
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn read_memory(path: &Path) -> Result<Memory> {
    read_memory_file(path)
}

pub fn update_memory<F>(path: &Path, updater: F) -> Result<()>
where
    F: FnOnce(&mut Memory),
{
    let _lock = acquire_lock(path)?;
    let mut memory = read_memory_file(path)?;
    updater(&mut memory);
    write_memory_unlocked(path, &memory)
}

fn read_memory_file(path: &Path) -> Result<Memory> {
    if !path.exists() {
        return Ok(Memory::new());
    }

    let bytes = fs::read(path).with_context(|| format!("无法读取记忆库: {}", path.display()))?;
    let memory = serde_json::from_slice::<Memory>(&bytes)
        .with_context(|| format!("记忆库 JSON 格式错误: {}", path.display()))?;
    Ok(memory)
}

fn write_memory_unlocked(path: &Path, memory: &Memory) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("记忆库路径没有父目录: {}", path.display()))?;
    fs::create_dir_all(parent)?;

    let json = serde_json::to_vec_pretty(memory)?;
    let temp_path = create_temp_file(path, &json)?;
    fs::rename(&temp_path, path)?;
    Ok(())
}

fn create_temp_file(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("smart_dl_memory.json");

    for _ in 0..8 {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = path.with_file_name(format!(
            "{file_name}.tmp-{}-{nonce}-{counter}",
            std::process::id()
        ));

        #[cfg(unix)]
        let options = {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true).mode(0o600);
            options
        };
        #[cfg(not(unix))]
        let mut options = {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            options
        };

        match options.open(&temp_path) {
            Ok(mut file) => {
                if let Err(error) = (|| -> Result<()> {
                    file.write_all(bytes)?;
                    file.sync_all()?;
                    Ok(())
                })() {
                    let _ = fs::remove_file(&temp_path);
                    return Err(error);
                }
                return Ok(temp_path);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("无法创建临时记忆库文件: {}", temp_path.display()));
            }
        }
    }

    Err(anyhow!("创建临时记忆库文件失败：命名冲突次数过多"))
}

struct MemoryLock {
    path: PathBuf,
}

impl Drop for MemoryLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn acquire_lock(memory_path: &Path) -> Result<MemoryLock> {
    let lock_path = memory_lock_path(memory_path)?;
    let started = Instant::now();

    loop {
        #[cfg(unix)]
        let options = {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true).mode(0o600);
            options
        };
        #[cfg(not(unix))]
        let mut options = {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            options
        };

        match options.open(&lock_path) {
            Ok(mut file) => {
                let _ = writeln!(file, "pid={}", std::process::id());
                return Ok(MemoryLock { path: lock_path });
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                if is_stale_lock(&lock_path) {
                    let _ = fs::remove_file(&lock_path);
                    continue;
                }
                if started.elapsed() >= LOCK_WAIT_TIMEOUT {
                    return Err(anyhow!(
                        "记忆库锁等待超时: {}",
                        lock_path.display()
                    ));
                }
                thread::sleep(LOCK_RETRY_INTERVAL);
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("无法创建记忆库锁文件: {}", lock_path.display()));
            }
        }
    }
}

fn memory_lock_path(memory_path: &Path) -> Result<PathBuf> {
    let file_name = memory_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("smart_dl_memory.json");
    Ok(memory_path.with_file_name(format!("{file_name}.lock")))
}

fn is_stale_lock(lock_path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(lock_path) else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    modified.elapsed().map(|age| age > LOCK_STALE_AFTER).unwrap_or(false)
}

pub fn top_destination(
    memory: &Memory,
    domain: &str,
    downloads_dir: &Path,
) -> Option<(PathBuf, u64)> {
    memory.get(domain).and_then(|destinations| {
        destinations
            .iter()
            .filter(|(path, _count)| !same_path(&PathBuf::from(path), downloads_dir))
            .max_by_key(|(_path, count)| *count)
            .map(|(path, count)| (PathBuf::from(path), *count))
    })
}

pub fn memory_summary(memory: &Memory) -> String {
    if memory.is_empty() {
        return "记忆库为空。先处理几次下载后，这里会出现常用归档目录。".to_string();
    }

    let mut lines = Vec::new();
    lines.push(format!("域名条目: {}", memory.len()));

    for (domain, targets) in memory.iter().take(8) {
        if let Some((path, count)) = targets.iter().max_by_key(|(_, count)| *count) {
            let short = PathBuf::from(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("目标目录")
                .to_string();
            lines.push(format!("{domain} -> {short} ({count})"));
        }
    }

    if memory.len() > 8 {
        lines.push("...".to_string());
    }

    lines.join("\n")
}
