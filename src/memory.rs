use crate::pathing::same_path;
use crate::types::Memory;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub fn read_memory(path: &Path) -> Result<Memory> {
    if !path.exists() {
        return Ok(Memory::new());
    }

    let bytes = fs::read(path).with_context(|| format!("无法读取记忆库: {}", path.display()))?;
    let memory = serde_json::from_slice::<Memory>(&bytes)
        .with_context(|| format!("记忆库 JSON 格式错误: {}", path.display()))?;
    Ok(memory)
}

pub fn write_memory(path: &Path, memory: &Memory) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("记忆库路径没有父目录: {}", path.display()))?;
    fs::create_dir_all(parent)?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("smart_dl_memory.json");
    let temp_path = path.with_file_name(format!("{file_name}.tmp-{}", std::process::id()));
    let json = serde_json::to_vec_pretty(memory)?;
    fs::write(&temp_path, json)?;
    fs::rename(&temp_path, path)?;
    Ok(())
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
