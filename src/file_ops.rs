use crate::memory::update_memory;
use crate::pathing::{expand_home, file_name, home_dir, log, same_path};
use crate::types::{AppConfig, FileSignature};
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const TEMP_SUFFIXES: &[&str] = &[".crdownload", ".part", ".download", ".tmp", ".opdownload"];

pub fn should_ignore_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };

    let lower = name.to_lowercase();
    name.starts_with('.') || TEMP_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix))
}

pub fn is_stable(path: &Path, delay: Duration) -> Result<bool> {
    let first = fs::metadata(path)?;
    std::thread::sleep(delay);
    let second = fs::metadata(path)?;

    Ok(first.len() == second.len() && modified_ms(&first)? == modified_ms(&second)?)
}

pub fn file_signature(path: &Path) -> Result<FileSignature> {
    let metadata = fs::metadata(path)?;
    Ok(FileSignature {
        path: path.to_path_buf(),
        size: metadata.len(),
        modified_ms: modified_ms(&metadata)?,
    })
}

pub fn move_and_remember(
    config: &AppConfig,
    path: &Path,
    domain: &str,
    target_dir: &Path,
) -> Result<()> {
    ensure_regular_file(path)?;
    let target_dir = expand_home(target_dir.to_path_buf())?;

    if same_path(&target_dir, &config.downloads_dir) {
        log(&format!(
            "目标仍是 Downloads，保持原位: {}",
            file_name(path)
        ));
        return Ok(());
    }

    fs::create_dir_all(&target_dir)
        .with_context(|| format!("无法创建目标目录: {}", target_dir.display()))?;
    let destination = unique_destination(&target_dir, &file_name(path));

    move_path(path, &destination)
        .with_context(|| format!("移动失败: {} -> {}", path.display(), destination.display()))?;

    let target = target_dir.to_string_lossy().to_string();
    update_memory(&config.memory_path, |memory| {
        *memory
            .entry(domain.to_string())
            .or_default()
            .entry(target)
            .or_default() += 1;
    })?;

    log(&format!(
        "已移动: {} -> {}",
        file_name(path),
        target_dir.display()
    ));
    Ok(())
}

pub fn trash_path(path: &Path) -> Result<()> {
    ensure_regular_file(path)?;
    let trash_dir = home_dir()?.join(".Trash");
    fs::create_dir_all(&trash_dir)?;
    let destination = unique_destination(&trash_dir, &file_name(path));
    move_path(path, &destination).with_context(|| {
        format!(
            "移入废纸篓失败: {} -> {}",
            path.display(),
            destination.display()
        )
    })?;
    log(&format!("已移入废纸篓: {}", file_name(path)));
    Ok(())
}

fn modified_ms(metadata: &fs::Metadata) -> Result<u128> {
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    Ok(modified
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis())
}

fn ensure_regular_file(path: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("无法读取文件元数据: {}", path.display()))?;
    if metadata.file_type().is_file() {
        return Ok(());
    }
    Err(anyhow!(
        "仅支持处理普通文件，拒绝路径: {}",
        path.display()
    ))
}

fn unique_destination(target_dir: &Path, file_name: &str) -> PathBuf {
    let extension = Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default();
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(file_name);

    let mut candidate = target_dir.join(file_name);
    let mut index = 2;

    while candidate.exists() {
        candidate = target_dir.join(format!("{stem} {index}{extension}"));
        index += 1;
    }

    candidate
}

fn move_path(source: &Path, destination: &Path) -> Result<()> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if is_cross_device_error(&error) => {
            if source.is_dir() {
                copy_dir_all(source, destination)?;
                fs::remove_dir_all(source)?;
            } else {
                fs::copy(source, destination)?;
                fs::remove_file(source)?;
            }
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn copy_dir_all(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = destination.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }

    Ok(())
}

fn is_cross_device_error(error: &io::Error) -> bool {
    error.raw_os_error() == Some(18)
}
