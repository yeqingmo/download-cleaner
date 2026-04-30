import { createServer } from "node:http";
import { execFile } from "node:child_process";
import { constants, existsSync } from "node:fs";
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
} from "node:fs/promises";
import { extname, join, dirname, resolve, basename, sep } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = dirname(fileURLToPath(import.meta.url));
const HOME = homedir();
const DOWNLOADS_DIR = process.env.DOWNLOADS_DIR || join(HOME, "Downloads");
const MEMORY_PATH = process.env.MEMORY_PATH || join(HOME, ".config", "smart_dl_memory.json");
const UI_STATE_PATH = join(HOME, ".config", "smart_dl_monitor_state.json");
const LAUNCH_AGENT_PATH = join(HOME, "Library", "LaunchAgents", "com.yy.download-cleaner.plist");
const PORT = Number(process.env.PORT || 4173);
const TEMP_SUFFIXES = [".crdownload", ".part", ".download", ".tmp", ".opdownload"];
const SOURCE_CACHE = new Map();
const startedAt = Date.now();

function execFileText(command, args, timeout = 1600) {
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

function fileId(name, stats) {
  return Buffer.from(`${name}|${Math.round(stats.mtimeMs)}|${stats.size}`).toString("base64url");
}

function isTempDownload(name) {
  const lower = name.toLowerCase();
  return name.startsWith(".") || TEMP_SUFFIXES.some((suffix) => lower.endsWith(suffix));
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

async function readSourceDomain(filePath, cacheKey) {
  if (SOURCE_CACHE.has(cacheKey)) {
    return SOURCE_CACHE.get(cacheKey);
  }

  let domain = "未知来源";

  try {
    const mdls = await execFileText("mdls", ["-name", "kMDItemWhereFroms", "-raw", filePath]);
    domain = parseDomain(mdls);
  } catch {
    try {
      const xattr = await execFileText("xattr", ["-p", "com.apple.metadata:kMDItemWhereFroms", filePath]);
      domain = parseDomain(xattr);
    } catch {
      domain = "未知来源";
    }
  }

  SOURCE_CACHE.set(cacheKey, domain);
  return domain;
}

function formatBytes(bytes) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }

  const units = ["KB", "MB", "GB", "TB"];
  let size = bytes / 1024;
  let index = 0;

  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }

  return `${size >= 10 ? size.toFixed(0) : size.toFixed(1)} ${units[index]}`;
}

function formatUptime(seconds) {
  const hours = Math.floor(seconds / 3600).toString().padStart(2, "0");
  const minutes = Math.floor((seconds % 3600) / 60).toString().padStart(2, "0");
  const rest = Math.floor(seconds % 60).toString().padStart(2, "0");
  return `${hours}:${minutes}:${rest}`;
}

function relativeTime(ms) {
  const delta = Math.max(0, Date.now() - ms);
  const minute = 60 * 1000;
  const hour = 60 * minute;
  const day = 24 * hour;

  if (delta < minute) {
    return "刚刚";
  }

  if (delta < hour) {
    return `${Math.floor(delta / minute)} 分钟前`;
  }

  if (delta < day) {
    return `${Math.floor(delta / hour)} 小时前`;
  }

  return `${Math.floor(delta / day)} 天前`;
}

function topDestination(memory, domain) {
  const destinations = memory[domain] || {};
  const entries = Object.entries(destinations).sort((a, b) => b[1] - a[1]);
  return entries[0] || null;
}

function displayPath(path) {
  return path.replace(HOME, "~");
}

function expandHome(path) {
  if (path === "~") {
    return HOME;
  }

  if (path.startsWith("~/")) {
    return join(HOME, path.slice(2));
  }

  return path;
}

async function exists(path) {
  try {
    await access(path, constants.F_OK);
    return true;
  } catch {
    return false;
  }
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

async function readUiState() {
  return readJson(UI_STATE_PATH, { ignored: {}, running: true });
}

async function saveUiState(value) {
  await writeJson(UI_STATE_PATH, value);
}

async function listDownloadEntries(memory, uiState) {
  const dirents = await readdir(DOWNLOADS_DIR, { withFileTypes: true });
  const entries = [];

  for (const dirent of dirents) {
    if (isTempDownload(dirent.name)) {
      continue;
    }

    const path = join(DOWNLOADS_DIR, dirent.name);

    try {
      const stats = await stat(path);

      if (!stats.isFile() && !stats.isDirectory()) {
        continue;
      }

      entries.push({ dirent, path, stats });
    } catch {
      // 文件可能在扫描时被移动或删除，跳过即可。
    }
  }

  entries.sort((a, b) => b.stats.mtimeMs - a.stats.mtimeMs);

  const limited = entries.slice(0, 80);
  const events = [];

  for (const entry of limited) {
    const id = fileId(entry.dirent.name, entry.stats);
    const domain = await readSourceDomain(entry.path, `${entry.path}:${Math.round(entry.stats.mtimeMs)}`);
    const suggestion = topDestination(memory, domain);
    const hasSuggestion = Boolean(suggestion && resolve(expandHome(suggestion[0])) !== resolve(DOWNLOADS_DIR));
    const target = hasSuggestion ? suggestion[0] : DOWNLOADS_DIR;
    const ignored = Boolean(uiState.ignored?.[id]);
    const state = ignored ? "ignored" : hasSuggestion ? "pending" : "local";

    events.push({
      id,
      file: entry.dirent.name,
      path: entry.path,
      domain,
      target,
      targetDisplay: displayPath(target),
      targetCount: suggestion?.[1] || 0,
      hasSuggestion,
      state,
      action: ignored ? "已放着不管" : hasSuggestion ? "待处理" : "待归类",
      time: relativeTime(entry.stats.mtimeMs),
      size: entry.stats.isDirectory() ? "文件夹" : formatBytes(entry.stats.size),
      mtimeMs: entry.stats.mtimeMs,
      isToday: new Date(entry.stats.mtimeMs).toDateString() === new Date().toDateString(),
    });
  }

  return events;
}

async function snapshot() {
  const memory = await readJson(MEMORY_PATH, {});
  const uiState = await readUiState();
  const events = await listDownloadEntries(memory, uiState);
  const memoryUsage = process.memoryUsage();

  return {
    status: {
      running: uiState.running !== false,
      uptime: formatUptime((Date.now() - startedAt) / 1000),
      downloadsDir: DOWNLOADS_DIR,
      memoryPath: MEMORY_PATH,
      launchAgentInstalled: existsSync(LAUNCH_AGENT_PATH),
      memoryMb: memoryUsage.rss / 1024 / 1024,
      todayCount: events.filter((event) => event.isToday).length,
    },
    events: events.map(({ path, ...event }) => event),
    memory,
  };
}

async function findEvent(id) {
  const memory = await readJson(MEMORY_PATH, {});
  const uiState = await readUiState();
  const events = await listDownloadEntries(memory, uiState);
  const event = events.find((item) => item.id === id);

  if (!event) {
    const error = new Error("这个文件已经不在 Downloads 里了");
    error.statusCode = 404;
    throw error;
  }

  return { event, memory, uiState };
}

async function ignoreFile(id) {
  const { event, uiState } = await findEvent(id);
  uiState.ignored = uiState.ignored || {};
  uiState.ignored[id] = true;
  await saveUiState(uiState);
  return `已放着不管：${event.file}`;
}

async function moveFile(id, targetDirectory) {
  const { event, memory, uiState } = await findEvent(id);
  const rawTarget = targetDirectory || (event.hasSuggestion ? event.target : null);

  if (!rawTarget) {
    const error = new Error("还没有历史建议，请先选择目录");
    error.statusCode = 400;
    throw error;
  }

  const directory = resolve(expandHome(rawTarget));

  if (directory === resolve(DOWNLOADS_DIR)) {
    await ignoreFile(id);
    return `已留在 Downloads：${event.file}`;
  }

  await mkdir(directory, { recursive: true });
  const destination = await uniqueDestination(directory, basename(event.path));
  await movePath(event.path, destination);

  memory[event.domain] = memory[event.domain] || {};
  memory[event.domain][directory] = (memory[event.domain][directory] || 0) + 1;
  await writeJson(MEMORY_PATH, memory);

  if (uiState.ignored?.[id]) {
    delete uiState.ignored[id];
    await saveUiState(uiState);
  }

  return `已移动到 ${displayPath(directory)}：${event.file}`;
}

async function chooseAndMove(id) {
  const { event } = await findEvent(id);
  const prompt = `选择「${event.file.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}」的归档目录`;
  const script = `POSIX path of (choose folder with prompt "${prompt}")`;
  const output = await execFileText("osascript", ["-e", script], 120000);
  const target = output.trim();

  if (!target) {
    const error = new Error("没有选择目录");
    error.statusCode = 400;
    throw error;
  }

  return moveFile(id, target);
}

async function toggleDaemon() {
  const uiState = await readUiState();
  uiState.running = uiState.running === false;
  await saveUiState(uiState);
  return uiState.running ? "监听已恢复" : "监听已暂停";
}

async function readBody(request) {
  const chunks = [];

  for await (const chunk of request) {
    chunks.push(chunk);
  }

  if (!chunks.length) {
    return {};
  }

  return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

function sendJson(response, statusCode, value) {
  response.writeHead(statusCode, {
    "Content-Type": "application/json; charset=utf-8",
    "Cache-Control": "no-store",
  });
  response.end(JSON.stringify(value));
}

async function handleApi(request, response, pathname) {
  if (request.method === "GET" && pathname === "/api/snapshot") {
    sendJson(response, 200, await snapshot());
    return;
  }

  if (request.method === "POST" && pathname === "/api/daemon/toggle") {
    const message = await toggleDaemon();
    sendJson(response, 200, { message, snapshot: await snapshot() });
    return;
  }

  if (request.method === "POST" && pathname === "/api/files/ignore") {
    const body = await readBody(request);
    const message = await ignoreFile(body.id);
    sendJson(response, 200, { message, snapshot: await snapshot() });
    return;
  }

  if (request.method === "POST" && pathname === "/api/files/move") {
    const body = await readBody(request);
    const message = await moveFile(body.id, body.target);
    sendJson(response, 200, { message, snapshot: await snapshot() });
    return;
  }

  if (request.method === "POST" && pathname === "/api/files/choose") {
    const body = await readBody(request);
    const message = await chooseAndMove(body.id);
    sendJson(response, 200, { message, snapshot: await snapshot() });
    return;
  }

  sendJson(response, 404, { error: "未知 API" });
}

const mimeTypes = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".md": "text/markdown; charset=utf-8",
};

async function serveStatic(response, pathname) {
  const requested = pathname === "/" ? "/index.html" : pathname;
  const filePath = resolve(ROOT, `.${decodeURIComponent(requested)}`);

  if (filePath !== ROOT && !filePath.startsWith(`${ROOT}${sep}`)) {
    response.writeHead(403);
    response.end("Forbidden");
    return;
  }

  try {
    const content = await readFile(filePath);
    response.writeHead(200, {
      "Content-Type": mimeTypes[extname(filePath)] || "application/octet-stream",
      "Cache-Control": "no-store",
    });
    response.end(content);
  } catch {
    response.writeHead(404);
    response.end("Not Found");
  }
}

const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url, `http://${request.headers.host || "127.0.0.1"}`);

    if (url.pathname.startsWith("/api/")) {
      await handleApi(request, response, url.pathname);
      return;
    }

    await serveStatic(response, url.pathname);
  } catch (error) {
    sendJson(response, error.statusCode || 500, { error: error.message || "服务异常" });
  }
});

export { snapshot };

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  server.listen(PORT, "127.0.0.1", () => {
    console.log(`Download Cleaner Monitor: http://127.0.0.1:${PORT}`);
    console.log(`Watching: ${DOWNLOADS_DIR}`);
  });
}
