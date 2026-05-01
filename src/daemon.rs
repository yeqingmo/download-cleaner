use crate::file_ops::{file_signature, is_stable, move_and_remember, should_ignore_path};
use crate::memory::{read_memory, top_destination};
use crate::metadata::extract_source_domain;
use crate::pathing::{file_name, is_inside_dir, log};
use crate::types::{AppConfig, BatchChoice, FileSignature, ReadyDownload, UserChoice};
use crate::ui::{choose_batch_folder, choose_folder, prompt_batch_user, prompt_user};
use anyhow::{anyhow, Context, Result};
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

enum HandleOutcome {
    Done,
    RetryLater,
    Ready(ReadyDownload),
}

pub fn run(config: AppConfig) -> Result<()> {
    fs::create_dir_all(
        config
            .memory_path
            .parent()
            .ok_or_else(|| anyhow!("记忆库路径没有父目录: {}", config.memory_path.display()))?,
    )?;

    let mut pending = HashMap::<PathBuf, Instant>::new();
    let mut ready = Vec::<ReadyDownload>::new();
    let mut batch_deadline = None::<Instant>;
    let mut known = snapshot_existing_files(&config, &mut pending)?;
    let mut processed = HashSet::new();
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(
        move |event| {
            let _ = tx.send(event);
        },
        NotifyConfig::default(),
    )?;

    watcher.watch(&config.downloads_dir, RecursiveMode::NonRecursive)?;

    log(&format!("监听目录: {}", config.downloads_dir.display()));
    log(&format!("记忆库: {}", config.memory_path.display()));
    log(&format!(
        "批处理窗口: {}ms",
        config.batch_window.as_millis()
    ));
    log(if config.scan_existing {
        "启动时会处理现有文件"
    } else {
        "启动时只记录现有文件，不弹历史文件"
    });

    loop {
        let timeout = next_timeout(&pending, batch_deadline).unwrap_or(Duration::from_secs(1));

        match rx.recv_timeout(timeout) {
            Ok(Ok(event)) => schedule_event_paths(event, &config, &mut pending),
            Ok(Err(error)) => log(&format!("监听事件错误: {error}")),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(anyhow!("文件监听通道已断开")),
        }

        let newly_ready = drain_due_paths(&config, &mut known, &mut processed, &mut pending);
        if !newly_ready.is_empty() {
            ready.extend(newly_ready);
            batch_deadline.get_or_insert_with(|| Instant::now() + config.batch_window);
        }

        if batch_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            process_ready_downloads(&config, &mut ready);
            batch_deadline = None;
        }
    }
}

fn snapshot_existing_files(
    config: &AppConfig,
    pending: &mut HashMap<PathBuf, Instant>,
) -> Result<HashSet<FileSignature>> {
    let mut known = HashSet::new();
    for entry in fs::read_dir(&config.downloads_dir)
        .with_context(|| format!("无法读取 Downloads: {}", config.downloads_dir.display()))?
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                log(&format!("读取目录项失败: {error}"));
                continue;
            }
        };
        let path = entry.path();
        if should_ignore_path(&path) {
            continue;
        }
        match file_signature(&path) {
            Ok(signature) => {
                if config.scan_existing {
                    pending.insert(path, Instant::now() + config.complete_delay);
                } else {
                    known.insert(signature);
                }
            }
            Err(error) => log(&format!("记录现有文件失败: {}: {error}", path.display())),
        }
    }
    Ok(known)
}

fn schedule_event_paths(event: Event, config: &AppConfig, pending: &mut HashMap<PathBuf, Instant>) {
    for path in event.paths {
        if !is_inside_dir(&path, &config.downloads_dir) || should_ignore_path(&path) {
            continue;
        }
        pending.insert(path, Instant::now() + config.complete_delay);
    }
}

fn drain_due_paths(
    config: &AppConfig,
    known: &mut HashSet<FileSignature>,
    processed: &mut HashSet<FileSignature>,
    pending: &mut HashMap<PathBuf, Instant>,
) -> Vec<ReadyDownload> {
    let now = Instant::now();
    let mut due_paths: Vec<PathBuf> = pending
        .iter()
        .filter_map(|(path, due)| (*due <= now).then(|| path.clone()))
        .collect();
    due_paths.sort();
    let mut ready = Vec::new();

    for path in due_paths {
        pending.remove(&path);
        match prepare_ready_download(config, known, processed, &path) {
            Ok(HandleOutcome::Done) => {}
            Ok(HandleOutcome::RetryLater) => {
                pending.insert(path, Instant::now() + config.complete_delay);
            }
            Ok(HandleOutcome::Ready(download)) => ready.push(download),
            Err(error) => log(&format!("处理失败: {}: {error:#}", path.display())),
        }
    }

    ready
}

fn prepare_ready_download(
    config: &AppConfig,
    known: &mut HashSet<FileSignature>,
    processed: &mut HashSet<FileSignature>,
    path: &Path,
) -> Result<HandleOutcome> {
    if should_ignore_path(path) || !path.exists() {
        return Ok(HandleOutcome::Done);
    }
    if !is_stable(path, config.complete_delay)? {
        return Ok(HandleOutcome::RetryLater);
    }
    let signature = file_signature(path)?;
    if known.contains(&signature) && !config.scan_existing {
        return Ok(HandleOutcome::Done);
    }
    if processed.contains(&signature) {
        return Ok(HandleOutcome::Done);
    }

    processed.insert(signature);
    let file_name = file_name(path);
    let domain = extract_source_domain(path).unwrap_or_else(|error| {
        log(&format!("读取来源失败: {}: {error}", path.display()));
        "未知来源".to_string()
    });
    Ok(HandleOutcome::Ready(ReadyDownload {
        path: path.to_path_buf(),
        file_name,
        domain,
        modified_ms: file_signature(path)?.modified_ms,
    }))
}

fn process_ready_downloads(config: &AppConfig, ready: &mut Vec<ReadyDownload>) {
    if ready.is_empty() {
        return;
    }
    ready.sort_by(|left, right| {
        left.domain
            .cmp(&right.domain)
            .then(left.modified_ms.cmp(&right.modified_ms))
            .then(left.file_name.cmp(&right.file_name))
    });

    let mut groups = BTreeMap::<String, Vec<ReadyDownload>>::new();
    for download in ready.drain(..) {
        groups
            .entry(batch_key(&download))
            .or_default()
            .push(download);
    }

    for (_key, downloads) in groups {
        let result = if downloads.len() > 1 {
            handle_batch_downloads(config, downloads)
        } else if let Some(download) = downloads.into_iter().next() {
            handle_single_download(config, download)
        } else {
            Ok(())
        };
        if let Err(error) = result {
            log(&format!("批处理失败: {error:#}"));
        }
    }
}

fn batch_key(download: &ReadyDownload) -> String {
    if download.domain == "未知来源" {
        format!("unknown:{}", download.path.display())
    } else {
        download.domain.clone()
    }
}

fn handle_single_download(config: &AppConfig, download: ReadyDownload) -> Result<()> {
    let memory = read_memory(&config.memory_path)?;
    let suggestion = top_destination(&memory, &download.domain, &config.downloads_dir);
    match prompt_user(&download.file_name, &download.domain, suggestion.as_ref())? {
        UserChoice::Ignore => {
            log(&format!("用户选择放着不管: {}", download.file_name));
            Ok(())
        }
        UserChoice::ChooseOther => {
            let target = choose_folder(&download.file_name)?;
            move_and_remember(config, &download.path, &download.domain, &target)
        }
        UserChoice::MoveTo(target) => {
            move_and_remember(config, &download.path, &download.domain, &target)
        }
    }
}

fn handle_batch_downloads(config: &AppConfig, downloads: Vec<ReadyDownload>) -> Result<()> {
    let domain = downloads
        .first()
        .map(|download| download.domain.clone())
        .unwrap_or_else(|| "未知来源".to_string());
    let memory = read_memory(&config.memory_path)?;
    let suggestion = top_destination(&memory, &domain, &config.downloads_dir);

    match prompt_batch_user(&domain, &downloads, suggestion.as_ref())? {
        BatchChoice::IgnoreAll => {
            log(&format!(
                "用户选择批量放着不管: {} 个来自 {domain} 的文件",
                downloads.len()
            ));
            Ok(())
        }
        BatchChoice::ChooseOtherAll => {
            let target = choose_batch_folder(&domain, downloads.len())?;
            move_batch_and_remember(config, &downloads, &domain, &target)
        }
        BatchChoice::MoveAllTo(target) => {
            move_batch_and_remember(config, &downloads, &domain, &target)
        }
        BatchChoice::OneByOne => {
            for download in downloads {
                if let Err(error) = handle_single_download(config, download) {
                    log(&format!("逐个处理失败: {error:#}"));
                }
            }
            Ok(())
        }
    }
}

fn move_batch_and_remember(
    config: &AppConfig,
    downloads: &[ReadyDownload],
    domain: &str,
    target_dir: &Path,
) -> Result<()> {
    for download in downloads {
        if let Err(error) = move_and_remember(config, &download.path, domain, target_dir) {
            log(&format!("批量移动失败: {}: {error:#}", download.file_name));
        }
    }
    Ok(())
}

fn next_timeout(
    pending: &HashMap<PathBuf, Instant>,
    batch_deadline: Option<Instant>,
) -> Option<Duration> {
    let now = Instant::now();
    let pending_timeout = pending
        .values()
        .min()
        .map(|deadline| deadline.saturating_duration_since(now));
    let batch_timeout = batch_deadline.map(|deadline| deadline.saturating_duration_since(now));

    match (pending_timeout, batch_timeout) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}
