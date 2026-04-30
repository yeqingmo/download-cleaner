import { execFile } from "node:child_process";
import { constants } from "node:fs";
import {
  access,
  cp,
  mkdir,
  readFile,
  readdir,
  rename,
  rm,
  stat,
  writeFile,
  watch,
} from "node:fs/promises";
import { basename, dirname, extname, join, resolve, sep } from "node:path";
import { homedir } from "node:os";
import { pathToFileURL } from "node:url";

const HOME = homedir();
const DOWNLOADS_DIR = resolve(process.env.DOWNLOADS_DIR || join(HOME, "Downloads"));
const MEMORY_PATH = resolve(process.env.MEMORY_PATH || join(HOME, ".config", "smart_dl_memory.json"));
const TEMP_SUFFIXES = [".crdownload", ".part", ".download", ".tmp", ".opdownload"];
const COMPLETE_DELAY_MS = Number(process.env.COMPLETE_DELAY_MS || 1200);
const POLL_INTERVAL_MS = Number(process.env.POLL_INTERVAL_MS || 3000);
const STARTUP_SCAN = process.env.SCAN_EXISTING === "1";

const known = new Set();
const processed = new Set();
const timers = new Map();
let dialogChain = Promise.resolve();

function log(message) {
  console.log(`[download-cleaner] ${new Date().toISOString()} ${message}`);
}

function sleep(ms) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, ms));
}

function execFileText(command, args, timeout = 3000) {
  return new Promise((resolvePromise, rejectPromise) => {
    execFile(command, args, { timeout, maxBuffer: 1024 * 1024 }, (error, stdout, stderr) => {
      if (error) {
        rejectPromise(error);
        return;
      }

      resolvePromise(`${stdout}${stderr}`);
    });
  });
}

function escapeAppleScript(value) {
  return String(value).replaceAll("\\", "\\\\").replaceAll('"', '\\"');
}

function isInsideDownloads(path) {
  const resolved = resolve(path);
  return resolved === DOWNLOADS_DIR || resolved.startsWith(`${DOWNLOADS_DIR}${sep}`);
}

function isTempDownload(name) {
  const lower = name.toLowerCase();
  return name.startsWith(".") || TEMP_SUFFIXES.some((suffix) => lower.endsWith(suffix));
}

function folderName(path) {
  return path.split(sep).filter(Boolean).pop() || "Downloads";
}

function parseDomain(raw) {
  const urls = raw.match(/https?:\/\/[^\s"'()<>]+/g) || [];

  for (const value of urls) {
    try {
      return new URL(value).hostname.replace(/^www\./, "") || "未知来源";
    } catch {
      // 继续尝试下一条来源 URL。
    }
  }

  return "未知来源";
}

async function readJson(filePath, fallback) {
  try {
    return JSON.parse(await readFile(filePath, "utf8"));
  } catch {
    return fallback;
  }
}

async function writeJson(filePath, value) {
  await mkdir(dirname(filePath), { recursive: true });
  const tempPath = `${filePath}.tmp-${process.pid}`;
  await writeFile(tempPath, `${JSON.stringify(value, null, 2)}\n`, "utf8");
  await rename(tempPath, filePath);
}

async function exists(path) {
  try {
    await access(path, constants.F_OK);
    return true;
  } catch {
    return false;
  }
}

async function readSourceDomain(filePath) {
  try {
    const mdls = await execFileText("mdls", ["-name", "kMDItemWhereFroms", "-raw", filePath]);
    return parseDomain(mdls);
  } catch {
    try {
      const xattr = await execFileText("xattr", ["-p", "com.apple.metadata:kMDItemWhereFroms", filePath]);
      return parseDomain(xattr);
    } catch {
      return "未知来源";
    }
  }
}

function topDestination(memory, domain) {
  const destinations = memory[domain] || {};
  return Object.entries(destinations).sort((a, b) => b[1] - a[1])[0] || null;
}

async function uniqueDestination(directory, name) {
  const extension = extname(name);
  const stem = extension ? name.slice(0, -extension.length) : name;
  let candidate = join(directory, name);
  let index = 2;

  while (await exists(candidate)) {
    candidate = join(directory, `${stem} ${index}${extension}`);
    index += 1;
  }

  return candidate;
}

async function movePath(source, destination) {
  try {
    await rename(source, destination);
  } catch (error) {
    if (error.code !== "EXDEV") {
      throw error;
    }

    await cp(source, destination, { recursive: true, errorOnExist: true, force: false });
    await rm(source, { recursive: true, force: false });
  }
}

async function chooseFolder(fileName) {
  const prompt = `选择「${fileName}」的归档目录`;
  const script = `POSIX path of (choose folder with prompt "${escapeAppleScript(prompt)}")`;
  const output = await execFileText("osascript", ["-e", script], 120000);
  return output.trim();
}

async function showMoveDialog({ fileName, domain, suggestion }) {
  const title = `检测到新下载: ${fileName}`;

  if (!suggestion) {
    const body = `来源: ${domain}。暂无历史建议，可以选择一个归档目录。`;
    const script = [
      `display dialog "${escapeAppleScript(body)}"`,
      `with title "${escapeAppleScript(title)}"`,
      'buttons {"放着不管", "选择其他..."}',
      'default button "选择其他..."',
      "with icon note",
      "return button returned of result",
    ].join(" ");
    return (await execFileText("osascript", ["-e", script], 120000)).trim();
  }

  const targetName = folderName(suggestion[0]);
  const moveButton = `移动到 ${targetName}`;
  const body = `来源: ${domain}。根据你的习惯，建议移至 ${targetName}。`;
  const script = [
    `display dialog "${escapeAppleScript(body)}"`,
    `with title "${escapeAppleScript(title)}"`,
    `buttons {"放着不管", "选择其他...", "${escapeAppleScript(moveButton)}"}`,
    `default button "${escapeAppleScript(moveButton)}"`,
    "with icon note",
    "return button returned of result",
  ].join(" ");

  return (await execFileText("osascript", ["-e", script], 120000)).trim();
}

async function moveAndRemember(filePath, targetDirectory, domain) {
  const directory = resolve(targetDirectory);

  if (!isInsideDownloads(filePath)) {
    throw new Error(`拒绝移动 Downloads 之外的路径: ${filePath}`);
  }

  if (directory === DOWNLOADS_DIR) {
    log(`保留在 Downloads: ${basename(filePath)}`);
    return;
  }

  await mkdir(directory, { recursive: true });
  const destination = await uniqueDestination(directory, basename(filePath));
  await movePath(filePath, destination);

  const memory = await readJson(MEMORY_PATH, {});
  memory[domain] = memory[domain] || {};
  memory[domain][directory] = (memory[domain][directory] || 0) + 1;
  await writeJson(MEMORY_PATH, memory);

  log(`已移动: ${basename(filePath)} -> ${directory}`);
}

async function isStableFile(filePath) {
  const first = await stat(filePath);
  await sleep(COMPLETE_DELAY_MS);
  const second = await stat(filePath);
  return first.size === second.size && Math.round(first.mtimeMs) === Math.round(second.mtimeMs);
}

async function handleCompletedDownload(filePath) {
  if (!isInsideDownloads(filePath) || isTempDownload(basename(filePath))) {
    return;
  }

  if (!(await exists(filePath))) {
    return;
  }

  if (!(await isStableFile(filePath))) {
    scheduleCheck(filePath);
    return;
  }

  const stats = await stat(filePath);
  const signature = `${filePath}:${Math.round(stats.mtimeMs)}:${stats.size}`;

  if (processed.has(signature)) {
    return;
  }

  processed.add(signature);

  const fileName = basename(filePath);
  const domain = await readSourceDomain(filePath);
  const memory = await readJson(MEMORY_PATH, {});
  const suggestion = topDestination(memory, domain);
  const answer = await showMoveDialog({ fileName, domain, suggestion });

  if (answer === "放着不管") {
    log(`用户选择放着不管: ${fileName}`);
    return;
  }

  if (answer === "选择其他..." || !suggestion) {
    const selected = await chooseFolder(fileName);
    await moveAndRemember(filePath, selected, domain);
    return;
  }

  await moveAndRemember(filePath, suggestion[0], domain);
}

function enqueueCompletedDownload(filePath) {
  dialogChain = dialogChain
    .then(() => handleCompletedDownload(filePath))
    .catch((error) => {
      log(`处理失败: ${error.message}`);
    });
}

function scheduleCheck(filePath) {
  if (timers.has(filePath)) {
    clearTimeout(timers.get(filePath));
  }

  timers.set(
    filePath,
    setTimeout(() => {
      timers.delete(filePath);
      enqueueCompletedDownload(filePath);
    }, COMPLETE_DELAY_MS),
  );
}

async function considerPath(filePath) {
  if (!isInsideDownloads(filePath) || isTempDownload(basename(filePath))) {
    return;
  }

  try {
    const stats = await stat(filePath);
    const signature = `${filePath}:${Math.round(stats.mtimeMs)}:${stats.size}`;

    if (!STARTUP_SCAN && !known.has(signature)) {
      known.add(signature);
      scheduleCheck(filePath);
    } else if (STARTUP_SCAN) {
      scheduleCheck(filePath);
    }
  } catch {
    // rename 事件也可能是删除或临时文件切换，忽略即可。
  }
}

async function scanForChanges() {
  const entries = await readdir(DOWNLOADS_DIR, { withFileTypes: true });

  for (const entry of entries) {
    if (isTempDownload(entry.name)) {
      continue;
    }

    await considerPath(join(DOWNLOADS_DIR, entry.name));
  }
}

async function pollLoop() {
  log(`使用轮询模式，每 ${POLL_INTERVAL_MS}ms 扫描一次`);

  while (true) {
    await scanForChanges();
    await sleep(POLL_INTERVAL_MS);
  }
}

async function watchLoop() {
  const watcher = watch(DOWNLOADS_DIR, { persistent: true });

  for await (const event of watcher) {
    if (!event.filename || isTempDownload(event.filename)) {
      continue;
    }

    await considerPath(join(DOWNLOADS_DIR, event.filename.toString()));
  }
}

async function snapshotExistingFiles() {
  const entries = await readdir(DOWNLOADS_DIR, { withFileTypes: true });

  for (const entry of entries) {
    if (isTempDownload(entry.name)) {
      continue;
    }

    const filePath = join(DOWNLOADS_DIR, entry.name);

    try {
      const stats = await stat(filePath);
      known.add(`${filePath}:${Math.round(stats.mtimeMs)}:${stats.size}`);

      if (STARTUP_SCAN) {
        scheduleCheck(filePath);
      }
    } catch {
      // 启动时文件可能正在移动，忽略即可。
    }
  }
}

async function start() {
  await mkdir(dirname(MEMORY_PATH), { recursive: true });
  await snapshotExistingFiles();

  log(`监听目录: ${DOWNLOADS_DIR}`);
  log(`记忆库: ${MEMORY_PATH}`);
  log(STARTUP_SCAN ? "启动时会扫描现有文件" : "启动时只记录现有文件，不弹历史文件");

  try {
    await watchLoop();
  } catch (error) {
    if (error.code !== "EMFILE" && error.code !== "ENOSPC") {
      throw error;
    }

    log(`系统文件监听不可用，切换到轮询: ${error.code}`);
    await pollLoop();
  }
}

export { start };

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  start().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
