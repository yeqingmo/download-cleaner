use anyhow::{anyhow, Context, Result};
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use plist::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};
use url::Url;

const TEMP_SUFFIXES: &[&str] = &[".crdownload", ".part", ".download", ".tmp", ".opdownload"];
const WHERE_FROMS_XATTR: &str = "com.apple.metadata:kMDItemWhereFroms";

// 记忆库格式与 AGENT.md 保持一致：
// {
//   "github.com": {
//     "/Users/yy/Code": 5
//   }
// }
type Memory = BTreeMap<String, BTreeMap<String, u64>>;

#[derive(Debug, Clone)]
struct AppConfig {
    downloads_dir: PathBuf,
    memory_path: PathBuf,
    complete_delay: Duration,
    batch_window: Duration,
    scan_existing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileSignature {
    path: PathBuf,
    size: u64,
    modified_ms: u128,
}

#[derive(Debug, Clone)]
struct ReadyDownload {
    path: PathBuf,
    file_name: String,
    domain: String,
    modified_ms: u128,
}

#[derive(Debug, Clone)]
enum UserChoice {
    Ignore,
    MoveTo(PathBuf),
    ChooseOther,
}

#[derive(Debug, Clone)]
enum BatchChoice {
    IgnoreAll,
    MoveAllTo(PathBuf),
    ChooseOtherAll,
    OneByOne,
}

fn main() -> Result<()> {
    let config = AppConfig::from_env()?;
    run(config)
}

impl AppConfig {
    fn from_env() -> Result<Self> {
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
            .unwrap_or(1_500);
        let batch_window_ms = env::var("BATCH_WINDOW_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(2_000);

        Ok(Self {
            downloads_dir: expand_home(downloads_dir)?,
            memory_path: expand_home(memory_path)?,
            complete_delay: Duration::from_millis(complete_delay_ms),
            batch_window: Duration::from_millis(batch_window_ms),
            scan_existing: env::var("SCAN_EXISTING").ok().as_deref() == Some("1"),
        })
    }
}

fn run(config: AppConfig) -> Result<()> {
    fs::create_dir_all(
        config
            .memory_path
            .parent()
            .ok_or_else(|| anyhow!("记忆库路径没有父目录: {}", config.memory_path.display()))?,
    )?;

    // pending 是一个很小的 debounce 队列。浏览器下载通常会先写临时文件，
    // 再 rename 成最终文件名；即使已经是最终文件名，也可能仍在增长。
    // 这里先延迟一小段时间，再用 size + mtime 二次确认文件稳定。
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
                    // 调试模式：现有文件也进入处理队列，方便手动整理存量 Downloads。
                    pending.insert(path, Instant::now() + config.complete_delay);
                } else {
                    // 默认不打扰用户处理历史文件，只记录启动时已经存在的签名。
                    // 后续只有新增或发生变化的文件才会进入弹窗流程。
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

enum HandleOutcome {
    Done,
    RetryLater,
    Ready(ReadyDownload),
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
    let choice = prompt_user(&download.file_name, &download.domain, suggestion.as_ref())?;

    match choice {
        UserChoice::Ignore => {
            log(&format!("用户选择放着不管: {}", download.file_name));
            Ok(())
        }
        UserChoice::ChooseOther => {
            let target = choose_folder(&download.file_name)?;
            move_and_remember(config, &download.path, &download.domain, &target)
        }
        UserChoice::MoveTo(target) => move_and_remember(config, &download.path, &download.domain, &target),
    }
}

fn handle_batch_downloads(config: &AppConfig, downloads: Vec<ReadyDownload>) -> Result<()> {
    let domain = downloads
        .first()
        .map(|download| download.domain.clone())
        .unwrap_or_else(|| "未知来源".to_string());
    let memory = read_memory(&config.memory_path)?;
    let suggestion = top_destination(&memory, &domain, &config.downloads_dir);
    let choice = prompt_batch_user(&domain, &downloads, suggestion.as_ref())?;

    match choice {
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
        BatchChoice::MoveAllTo(target) => move_batch_and_remember(config, &downloads, &domain, &target),
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

fn prompt_user(
    file_name: &str,
    domain: &str,
    suggestion: Option<&(PathBuf, u64)>,
) -> Result<UserChoice> {
    let title = format!("检测到新下载: {file_name}");

    if let Some((target, _count)) = suggestion {
        let target_name = folder_name(target);
        let move_button = format!("移动到 {target_name}");
        let body = format!("来源: {domain}。根据你的习惯，建议移至 {target_name}。");
        let script = format!(
            r#"display dialog "{}" with title "{}" buttons {{"放着不管", "选择其他...", "{}"}} default button "{}" with icon note
return button returned of result"#,
            escape_applescript(&body),
            escape_applescript(&title),
            escape_applescript(&move_button),
            escape_applescript(&move_button),
        );
        let answer = run_osascript(&script)?;

        if answer == "放着不管" {
            Ok(UserChoice::Ignore)
        } else if answer == "选择其他..." {
            Ok(UserChoice::ChooseOther)
        } else {
            Ok(UserChoice::MoveTo(target.clone()))
        }
    } else {
        let body = format!("来源: {domain}。暂无历史建议，可以选择一个归档目录。");
        let script = format!(
            r#"display dialog "{}" with title "{}" buttons {{"放着不管", "选择其他..."}} default button "选择其他..." with icon note
return button returned of result"#,
            escape_applescript(&body),
            escape_applescript(&title),
        );
        let answer = run_osascript(&script)?;

        if answer == "放着不管" {
            Ok(UserChoice::Ignore)
        } else {
            Ok(UserChoice::ChooseOther)
        }
    }
}

fn choose_folder(file_name: &str) -> Result<PathBuf> {
    let prompt = format!("选择「{file_name}」的归档目录");
    let script = format!(
        r#"POSIX path of (choose folder with prompt "{}")"#,
        escape_applescript(&prompt)
    );
    let output = run_osascript(&script)?;
    Ok(PathBuf::from(output.trim()))
}

fn run_osascript(script: &str) -> Result<String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .context("无法执行 osascript")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("osascript 失败: {}", stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn move_and_remember(
    config: &AppConfig,
    path: &Path,
    domain: &str,
    target_dir: &Path,
) -> Result<()> {
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

    // 移动成功后再更新记忆库，避免“记录成功但文件没动”的状态不一致。
    let mut memory = read_memory(&config.memory_path)?;
    let target = target_dir.to_string_lossy().to_string();
    *memory
        .entry(domain.to_string())
        .or_default()
        .entry(target)
        .or_default() += 1;
    write_memory(&config.memory_path, &memory)?;

    log(&format!(
        "已移动: {} -> {}",
        file_name(path),
        target_dir.display()
    ));
    Ok(())
}

fn read_memory(path: &Path) -> Result<Memory> {
    if !path.exists() {
        return Ok(Memory::new());
    }

    let bytes = fs::read(path).with_context(|| format!("无法读取记忆库: {}", path.display()))?;
    let memory = serde_json::from_slice::<Memory>(&bytes)
        .with_context(|| format!("记忆库 JSON 格式错误: {}", path.display()))?;
    Ok(memory)
}

fn write_memory(path: &Path, memory: &Memory) -> Result<()> {
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

fn top_destination(memory: &Memory, domain: &str, downloads_dir: &Path) -> Option<(PathBuf, u64)> {
    memory.get(domain).and_then(|destinations| {
        destinations
            .iter()
            .filter(|(path, _count)| !same_path(&PathBuf::from(path), downloads_dir))
            .max_by_key(|(_path, count)| *count)
            .map(|(path, count)| (PathBuf::from(path), *count))
    })
}

fn extract_source_domain(path: &Path) -> Result<String> {
    // Chrome/Safari/Edge 下载文件通常会把来源写进这个扩展属性。
    // 内容是 binary plist，常见结构是字符串数组，所以用 plist crate 解码。
    let Some(bytes) = xattr::get(path, WHERE_FROMS_XATTR)
        .with_context(|| format!("读取 xattr 失败: {}", path.display()))?
    else {
        return Ok("未知来源".to_string());
    };

    let value =
        Value::from_reader(Cursor::new(bytes)).context("解析 kMDItemWhereFroms plist 失败")?;
    let mut candidates = Vec::new();
    collect_plist_strings(&value, &mut candidates);

    for candidate in candidates {
        if let Some(domain) = first_domain_in_text(&candidate) {
            return Ok(domain);
        }
    }

    Ok("未知来源".to_string())
}

fn collect_plist_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(value) => output.push(value.clone()),
        Value::Array(values) => {
            for value in values {
                collect_plist_strings(value, output);
            }
        }
        Value::Dictionary(values) => {
            for value in values.values() {
                collect_plist_strings(value, output);
            }
        }
        _ => {}
    }
}

fn first_domain_in_text(text: &str) -> Option<String> {
    for token in text.split(|ch: char| {
        ch.is_whitespace() || matches!(ch, '"' | '\'' | '(' | ')' | '<' | '>' | ',')
    }) {
        let trimmed = token.trim_matches(|ch: char| matches!(ch, '.' | ';' | ']' | '['));

        if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            continue;
        }

        if let Ok(url) = Url::parse(trimmed) {
            if let Some(host) = url.host_str() {
                return Some(host.strip_prefix("www.").unwrap_or(host).to_string());
            }
        }
    }

    None
}

fn is_stable(path: &Path, delay: Duration) -> Result<bool> {
    let first = fs::metadata(path)?;
    std::thread::sleep(delay);
    let second = fs::metadata(path)?;

    Ok(first.len() == second.len() && modified_ms(&first)? == modified_ms(&second)?)
}

fn file_signature(path: &Path) -> Result<FileSignature> {
    let metadata = fs::metadata(path)?;

    Ok(FileSignature {
        path: path.to_path_buf(),
        size: metadata.len(),
        modified_ms: modified_ms(&metadata)?,
    })
}

fn modified_ms(metadata: &fs::Metadata) -> Result<u128> {
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    Ok(modified
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis())
}

fn should_ignore_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };

    let lower = name.to_lowercase();
    name.starts_with('.') || TEMP_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix))
}

fn next_timeout(pending: &HashMap<PathBuf, Instant>) -> Option<Duration> {
    let now = Instant::now();
    pending
        .values()
        .min()
        .map(|deadline| deadline.saturating_duration_since(now))
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
            // 如果目标目录在另一个磁盘卷，rename 会返回 EXDEV。
            // 这时退化为 copy + remove，行为上仍然是“移动”。
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

fn expand_home(path: PathBuf) -> Result<PathBuf> {
    let Some(raw) = path.to_str() else {
        return Ok(path);
    };

    if raw == "~" {
        return home_dir();
    }

    if let Some(rest) = raw.strip_prefix("~/") {
        return Ok(home_dir()?.join(rest));
    }

    Ok(path)
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("无法读取 HOME 环境变量"))
}

fn is_inside_dir(path: &Path, dir: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    path == dir || path.starts_with(dir)
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.canonicalize().unwrap_or_else(|_| left.to_path_buf())
        == right.canonicalize().unwrap_or_else(|_| right.to_path_buf())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("未知文件")
        .to_string()
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("目标目录")
        .to_string()
}

fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn log(message: &str) {
    eprintln!("[download-cleaner] {message}");
}
