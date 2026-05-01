use crate::file_ops::{file_signature, is_stable, move_and_remember, should_ignore_path};
use crate::memory::{read_memory, top_destination};
use crate::metadata::extract_source_domain;
use crate::pathing::{file_name, is_inside_dir, log};
use crate::types::{AppConfig, BatchChoice, FileSignature, ReadyDownload, UserChoice};
use crate::ui::{choose_batch_folder, choose_folder, prompt_batch_user, prompt_user};
use anyhow::{anyhow, Context, Result};
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const PROCESSED_TTL: Duration = Duration::from_secs(60 * 60);
const PROCESSED_MAX: usize = 20_000;
const PROCESSED_PRUNE_TO: usize = 10_000;
const SINGLE_GROUP_GRACE: Duration = Duration::from_millis(800);
const PENDING_MAX_WAIT: Duration = Duration::from_secs(20);

struct DomainQueue {
    downloads: Vec<ReadyDownload>,
    deadline: Instant,
    batching: bool,
}

struct PendingEntry {
    due: Instant,
    first_seen: Instant,
}

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

    let mut pending = HashMap::<PathBuf, PendingEntry>::new();
    let mut domain_queues = HashMap::<String, DomainQueue>::new();
    let mut known = snapshot_existing_files(&config, &mut pending)?;
    known.shrink_to_fit();
    let mut processed = HashMap::<FileSignature, Instant>::new();
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
        let timeout = next_timeout(&pending, &domain_queues).unwrap_or(Duration::from_secs(1));

        match rx.recv_timeout(timeout) {
            Ok(Ok(event)) => schedule_event_paths(event, &config, &mut pending),
            Ok(Err(error)) => log(&format!("监听事件错误: {error}")),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(anyhow!("文件监听通道已断开")),
        }

        prune_processed(&mut processed);
        let newly_ready = drain_due_paths(&config, &mut known, &mut processed, &mut pending);
        if !newly_ready.is_empty() {
            enqueue_ready_downloads(&config, &mut domain_queues, newly_ready);
        }
        process_due_queues(&config, &mut domain_queues);
        compact_pending(&mut pending);
        compact_domain_queues(&mut domain_queues);
        compact_processed(&mut processed);
    }
}

fn snapshot_existing_files(
    config: &AppConfig,
    pending: &mut HashMap<PathBuf, PendingEntry>,
) -> Result<HashSet<FileSignature>> {
    let mut known = HashSet::new();
    let now = Instant::now();
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
        match is_regular_file_path(&path) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => {
                log(&format!("检查文件类型失败: {}: {error}", path.display()));
                continue;
            }
        }
        match file_signature(&path) {
            Ok(signature) => {
                if config.scan_existing {
                    pending.insert(path, PendingEntry {
                        due: now + config.complete_delay,
                        first_seen: now,
                    });
                } else {
                    known.insert(signature);
                }
            }
            Err(error) => log(&format!("记录现有文件失败: {}: {error}", path.display())),
        }
    }
    Ok(known)
}

fn schedule_event_paths(event: Event, config: &AppConfig, pending: &mut HashMap<PathBuf, PendingEntry>) {
    for path in event.paths {
        if !is_inside_dir(&path, &config.downloads_dir) || should_ignore_path(&path) {
            continue;
        }
        let now = Instant::now();
        pending
            .entry(path)
            .and_modify(|entry| {
                entry.due = now + config.complete_delay;
            })
            .or_insert(PendingEntry {
                due: now + config.complete_delay,
                first_seen: now,
            });
    }
}

fn drain_due_paths(
    config: &AppConfig,
    known: &mut HashSet<FileSignature>,
    processed: &mut HashMap<FileSignature, Instant>,
    pending: &mut HashMap<PathBuf, PendingEntry>,
) -> Vec<ReadyDownload> {
    let now = Instant::now();
    let mut due_paths: Vec<PathBuf> = pending
        .iter()
        .filter(|(_, entry)| entry.due <= now)
        .map(|(path, _)| path.clone())
        .collect();
    due_paths.sort();
    let mut ready = Vec::new();

    for path in due_paths {
        let Some(entry) = pending.remove(&path) else {
            continue;
        };
        match prepare_ready_download(config, known, processed, &path, entry.first_seen) {
            Ok(HandleOutcome::Done) => {}
            Ok(HandleOutcome::RetryLater) => {
                let now = Instant::now();
                pending.insert(
                    path,
                    PendingEntry {
                        due: now + config.complete_delay,
                        first_seen: entry.first_seen,
                    },
                );
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
    processed: &mut HashMap<FileSignature, Instant>,
    path: &Path,
    first_seen: Instant,
) -> Result<HandleOutcome> {
    if should_ignore_path(path) || !path.exists() {
        return Ok(HandleOutcome::Done);
    }
    if !is_regular_file_path(path)? {
        return Ok(HandleOutcome::Done);
    }
    if !is_stable(path, config.complete_delay)? {
        if Instant::now().duration_since(first_seen) >= PENDING_MAX_WAIT {
            log(&format!(
                "文件稳定等待超时，强制处理: {}",
                path.display()
            ));
        } else {
            return Ok(HandleOutcome::RetryLater);
        }
    }
    let signature = file_signature(path)?;
    if known.contains(&signature) && !config.scan_existing {
        return Ok(HandleOutcome::Done);
    }
    if processed.contains_key(&signature) {
        return Ok(HandleOutcome::Done);
    }

    processed.insert(signature.clone(), Instant::now());
    let file_name = file_name(path);
    let domain = extract_source_domain(path).unwrap_or_else(|error| {
        log(&format!("读取来源失败: {}: {error}", path.display()));
        "未知来源".to_string()
    });
    Ok(HandleOutcome::Ready(ReadyDownload {
        path: path.to_path_buf(),
        file_name,
        domain,
        modified_ms: signature.modified_ms,
    }))
}

fn enqueue_ready_downloads(
    config: &AppConfig,
    queues: &mut HashMap<String, DomainQueue>,
    downloads: Vec<ReadyDownload>,
) {
    let now = Instant::now();
    for download in downloads {
        let key = batch_key(&download);
        if let Some(queue) = queues.get_mut(&key) {
            queue.downloads.push(download);
            if !queue.batching && queue.downloads.len() >= 2 {
                queue.batching = true;
                queue.deadline = now + config.batch_window;
            }
        } else {
            queues.insert(
                key,
                DomainQueue {
                    downloads: vec![download],
                    deadline: now + SINGLE_GROUP_GRACE,
                    batching: false,
                },
            );
        }
    }
}

fn process_due_queues(config: &AppConfig, queues: &mut HashMap<String, DomainQueue>) {
    if queues.is_empty() {
        return;
    }

    let now = Instant::now();
    let mut due_keys: Vec<String> = queues
        .iter()
        .filter(|(_key, queue)| queue.deadline <= now)
        .map(|(key, _queue)| key.clone())
        .collect();
    due_keys.sort();

    for key in due_keys {
        let Some(mut queue) = queues.remove(&key) else {
            continue;
        };

        queue.downloads.sort_by(|left, right| {
            left.modified_ms
                .cmp(&right.modified_ms)
                .then(left.file_name.cmp(&right.file_name))
        });

        let result = if queue.downloads.len() > 1 {
            handle_batch_downloads(config, queue.downloads)
        } else if let Some(download) = queue.downloads.into_iter().next() {
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
    pending: &HashMap<PathBuf, PendingEntry>,
    queues: &HashMap<String, DomainQueue>,
) -> Option<Duration> {
    let now = Instant::now();
    let pending_timeout = pending
        .values()
        .map(|entry| entry.due.saturating_duration_since(now))
        .min();
    let queue_timeout = queues
        .values()
        .map(|queue| queue.deadline.saturating_duration_since(now))
        .min();

    match (pending_timeout, queue_timeout) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn prune_processed(processed: &mut HashMap<FileSignature, Instant>) {
    let now = Instant::now();
    processed.retain(|_signature, inserted_at| now.duration_since(*inserted_at) <= PROCESSED_TTL);

    if processed.len() <= PROCESSED_MAX {
        return;
    }

    let mut entries: Vec<(FileSignature, Instant)> =
        processed.iter().map(|(signature, ts)| (signature.clone(), *ts)).collect();
    entries.sort_by_key(|(_signature, ts)| *ts);
    let remove_count = entries.len().saturating_sub(PROCESSED_PRUNE_TO);
    for (signature, _ts) in entries.into_iter().take(remove_count) {
        processed.remove(&signature);
    }
}

fn compact_processed(processed: &mut HashMap<FileSignature, Instant>) {
    if processed.is_empty() {
        processed.shrink_to_fit();
        return;
    }
    if processed.capacity() > PROCESSED_MAX * 2 && processed.len() <= PROCESSED_PRUNE_TO {
        processed.shrink_to_fit();
    }
}

fn compact_pending(pending: &mut HashMap<PathBuf, PendingEntry>) {
    if pending.is_empty() {
        pending.shrink_to_fit();
    }
}

fn compact_domain_queues(queues: &mut HashMap<String, DomainQueue>) {
    if queues.is_empty() {
        queues.shrink_to_fit();
    }
}

fn is_regular_file_path(path: &Path) -> Result<bool> {
    Ok(fs::symlink_metadata(path)?.file_type().is_file())
}
