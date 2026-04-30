const state = {
  snapshot: null,
  currentId: null,
  loadingIds: new Set(),
};

const $ = (selector) => document.querySelector(selector);

const elements = {
  uptime: $("#daemon-uptime"),
  toggleDaemon: $("#toggle-daemon"),
  eventList: $("#event-list"),
  memoryList: $("#memory-list"),
  search: $("#domain-search"),
  refreshFiles: $("#refresh-files"),
  normalizeMemory: $("#normalize-memory"),
  exportJson: $("#export-json"),
  toast: $("#toast"),
  suggestionTitle: $("#suggestion-title"),
  suggestionCopy: $("#suggestion-copy"),
  promptTitle: $("#prompt-title"),
  promptBody: $("#prompt-body"),
  ignoreCurrent: $("#ignore-current"),
  chooseCurrent: $("#choose-current"),
  moveCurrent: $("#move-current"),
  metricNew: $("#metric-new"),
  metricHit: $("#metric-hit"),
  metricPending: $("#metric-pending"),
  metricMemory: $("#metric-memory"),
  watchPath: $("#watch-path"),
  launchdStatus: $("#launchd-status"),
  memoryPath: $("#memory-path"),
};

function escapeHtml(value = "") {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function folderName(path = "") {
  return path.split("/").filter(Boolean).pop() || "Downloads";
}

function showToast(message) {
  elements.toast.textContent = message;
  elements.toast.classList.add("show");
  window.clearTimeout(showToast.timer);
  showToast.timer = window.setTimeout(() => {
    elements.toast.classList.remove("show");
  }, 2400);
}

async function api(path, options = {}) {
  const response = await fetch(path, {
    headers: {
      "Content-Type": "application/json",
      ...(options.headers || {}),
    },
    ...options,
  });

  const payload = await response.json().catch(() => ({}));

  if (!response.ok) {
    throw new Error(payload.error || "本机服务返回异常");
  }

  return payload;
}

function renderOffline(error) {
  elements.suggestionTitle.textContent = "需要启动本地服务";
  elements.suggestionCopy.textContent = "在项目目录运行 node server.mjs，前端才能读取真实 ~/Downloads。";
  elements.promptTitle.textContent = "本机 API 未连接";
  elements.promptBody.textContent = error?.message || "当前页面没有拿到 /api/snapshot。";
  elements.metricNew.textContent = "0";
  elements.metricHit.textContent = "0%";
  elements.metricPending.textContent = "0";
  elements.metricMemory.textContent = "0 MB";
  elements.uptime.textContent = "未连接";
  elements.eventList.innerHTML = `
    <div class="empty-state">
      <strong>还没连到本机服务</strong>
      <span>运行 node server.mjs 后刷新页面，这里会显示真实 Downloads 文件。</span>
    </div>
  `;
  elements.memoryList.innerHTML = `
    <div class="empty-state">
      <strong>等待读取记忆库</strong>
      <span>路径：~/.config/smart_dl_memory.json</span>
    </div>
  `;
}

async function loadSnapshot({ quiet = false } = {}) {
  try {
    state.snapshot = await api("/api/snapshot");
    render();
    if (!quiet) {
      showToast("已刷新本机 Downloads");
    }
  } catch (error) {
    renderOffline(error);
    if (!quiet) {
      showToast("本机服务未启动");
    }
  }
}

function renderMetrics() {
  const { events = [], status = {} } = state.snapshot || {};
  const actionable = events.filter((event) => event.state !== "ignored");
  const suggested = actionable.filter((event) => event.hasSuggestion);
  const pending = actionable.filter((event) => event.state === "pending").length;
  const hitRate = actionable.length ? Math.round((suggested.length / actionable.length) * 100) : 0;

  elements.metricNew.textContent = events.length;
  elements.metricHit.textContent = `${hitRate}%`;
  elements.metricPending.textContent = pending;
  elements.metricMemory.textContent = status.memoryMb ? `${status.memoryMb.toFixed(1)} MB` : "0 MB";
  elements.uptime.textContent = status.running ? `已运行 ${status.uptime}` : "已暂停";
  elements.watchPath.textContent = status.downloadsDir || "~/Downloads";
  elements.launchdStatus.textContent = status.launchAgentInstalled ? "com.yy.download-cleaner.plist 已安装" : "未检测到 LaunchAgent";
  elements.memoryPath.textContent = status.memoryPath || "~/.config/smart_dl_memory.json";
  elements.toggleDaemon.classList.toggle("is-paused", !status.running);
  elements.toggleDaemon.innerHTML = status.running
    ? `
      <span class="button-icon">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 5v14M16 5v14" /></svg>
      </span>
      暂停监听
    `
    : `
      <span class="button-icon">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m8 5 11 7-11 7V5Z" /></svg>
      </span>
      继续监听
    `;
}

function renderSuggestion() {
  const events = state.snapshot?.events || [];
  const activeEvent =
    events.find((event) => event.state === "pending") ||
    events.find((event) => event.state === "local") ||
    events.find((event) => event.state !== "ignored");

  if (!activeEvent) {
    state.currentId = null;
    elements.suggestionTitle.textContent = "Downloads 暂无可处理文件";
    elements.suggestionCopy.textContent = "隐藏文件和下载临时文件已过滤。";
    elements.promptTitle.textContent = "没有待处理下载";
    elements.promptBody.textContent = "新的文件出现后会自动显示在这里。";
    elements.ignoreCurrent.disabled = true;
    elements.chooseCurrent.disabled = true;
    elements.moveCurrent.disabled = true;
    elements.moveCurrent.textContent = "移动";
    return;
  }

  state.currentId = activeEvent.id;
  const targetName = folderName(activeEvent.target);
  elements.suggestionTitle.textContent = activeEvent.hasSuggestion
    ? `${activeEvent.domain} -> ${activeEvent.target}`
    : `${activeEvent.file} 还没有历史建议`;
  elements.suggestionCopy.textContent = activeEvent.hasSuggestion
    ? `记忆库命中 ${activeEvent.targetCount} 次，可以直接移至 ${targetName}。`
    : "可以选择目录归档一次，之后同来源会记住这个目标。";
  elements.promptTitle.textContent = `检测到新下载: ${activeEvent.file}`;
  elements.promptBody.textContent = activeEvent.hasSuggestion
    ? `来源: ${activeEvent.domain}。根据你的习惯，建议移至 ${targetName}。`
    : `来源: ${activeEvent.domain}。暂无记忆库建议，可以选择其他目录。`;
  elements.ignoreCurrent.disabled = false;
  elements.chooseCurrent.disabled = false;
  elements.moveCurrent.disabled = false;
  elements.moveCurrent.textContent = activeEvent.hasSuggestion ? `移动到 ${targetName}` : "选择目录";
}

function renderEvents() {
  const term = elements.search.value.trim().toLowerCase();
  const events = state.snapshot?.events || [];
  const filteredEvents = events.filter((event) => {
    const haystack = `${event.file} ${event.domain} ${event.target} ${event.size}`.toLowerCase();
    return haystack.includes(term);
  });

  if (!filteredEvents.length) {
    elements.eventList.innerHTML = `
      <div class="empty-state">
        <strong>没有匹配的 Downloads 文件</strong>
        <span>换个关键词，或者点右上角刷新。</span>
      </div>
    `;
    return;
  }

  elements.eventList.innerHTML = filteredEvents
    .map((event) => {
      const stateClass = event.state === "pending" || event.state === "ignored" ? event.state : "";
      const busy = state.loadingIds.has(event.id);
      const disabled = busy ? "disabled" : "";
      const moveLabel = event.hasSuggestion ? `移到 ${escapeHtml(folderName(event.target))}` : "选择目录";
      const path = event.targetDisplay || event.target;

      return `
        <article class="event-item" data-id="${escapeHtml(event.id)}">
          <span class="event-state ${stateClass}"></span>
          <div class="event-main">
            <p class="event-file">${escapeHtml(event.file)}</p>
            <p class="event-meta">${escapeHtml(event.domain)} · ${escapeHtml(path)} · ${escapeHtml(event.size)} · ${escapeHtml(event.time)}</p>
          </div>
          <div class="row-actions">
            <span class="event-action">${escapeHtml(event.action)}</span>
            <button class="action-button" type="button" data-action="ignore" data-id="${escapeHtml(event.id)}" ${disabled}>放着不管</button>
            <button class="action-button" type="button" data-action="choose" data-id="${escapeHtml(event.id)}" ${disabled}>选择...</button>
            <button class="action-button primary" type="button" data-action="${event.hasSuggestion ? "move" : "choose"}" data-id="${escapeHtml(event.id)}" ${disabled}>${moveLabel}</button>
          </div>
        </article>
      `;
    })
    .join("");
}

function renderMemory() {
  const term = elements.search.value.trim().toLowerCase();
  const memory = state.snapshot?.memory || {};
  const domains = Object.entries(memory).filter(([domain, destinations]) => {
    const haystack = `${domain} ${Object.keys(destinations).join(" ")}`.toLowerCase();
    return haystack.includes(term);
  });

  if (!domains.length) {
    elements.memoryList.innerHTML = `
      <div class="empty-state">
        <strong>记忆库还是空的</strong>
        <span>移动一次文件后，会写入 ~/.config/smart_dl_memory.json。</span>
      </div>
    `;
    return;
  }

  elements.memoryList.innerHTML = domains
    .map(([domain, destinations]) => {
      const entries = Object.entries(destinations).sort((a, b) => b[1] - a[1]);
      const total = entries.reduce((sum, [, count]) => sum + count, 0);
      const max = Math.max(...entries.map(([, count]) => count), 1);
      const rows = entries
        .map(([path, count]) => {
          const width = Math.max(8, Math.round((count / max) * 100));
          return `
            <div class="memory-path">
              <div>
                <div class="path-label">
                  <span>${escapeHtml(path)}</span>
                  <span>${Math.round((count / total) * 100)}%</span>
                </div>
                <div class="bar-track">
                  <div class="bar-fill" style="width: ${width}%"></div>
                </div>
              </div>
              <span class="path-count">${count}</span>
            </div>
          `;
        })
        .join("");

      return `
        <article class="memory-domain">
          <div class="memory-title">
            <strong>${escapeHtml(domain)}</strong>
            <span class="memory-total">${total} 次</span>
          </div>
          ${rows}
        </article>
      `;
    })
    .join("");
}

function render() {
  renderMetrics();
  renderSuggestion();
  renderEvents();
  renderMemory();
}

async function performFileAction(action, id) {
  if (!id) {
    showToast("没有选中的文件");
    return;
  }

  state.loadingIds.add(id);
  renderEvents();

  try {
    const endpoint =
      action === "ignore" ? "/api/files/ignore" : action === "move" ? "/api/files/move" : "/api/files/choose";
    const result = await api(endpoint, {
      method: "POST",
      body: JSON.stringify({ id }),
    });
    state.snapshot = result.snapshot;
    render();
    showToast(result.message || "操作已完成");
  } catch (error) {
    showToast(error.message);
  } finally {
    state.loadingIds.delete(id);
    renderEvents();
  }
}

async function toggleDaemon() {
  try {
    const result = await api("/api/daemon/toggle", { method: "POST" });
    state.snapshot = result.snapshot;
    render();
    showToast(result.message);
  } catch (error) {
    showToast(error.message);
  }
}

function normalizeMemory() {
  renderMemory();
  showToast("前端已按权重排序显示");
}

function exportMemoryJson() {
  const json = JSON.stringify(state.snapshot?.memory || {}, null, 2);

  if (!navigator.clipboard?.writeText) {
    showToast("当前浏览器未开放剪贴板写入");
    return;
  }

  navigator.clipboard.writeText(json).then(
    () => showToast("记忆库 JSON 已复制"),
    () => showToast("浏览器未允许剪贴板写入"),
  );
}

function startPolling() {
  window.setInterval(() => {
    if (state.snapshot?.status?.running) {
      loadSnapshot({ quiet: true });
    }
  }, 5000);
}

function drawLiquidCanvas() {
  const canvas = $("#liquid-canvas");
  const context = canvas.getContext("2d");
  let width = 0;
  let height = 0;
  let frame = 0;

  function resize() {
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    width = window.innerWidth;
    height = window.innerHeight;
    canvas.width = width * ratio;
    canvas.height = height * ratio;
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
  }

  function paint() {
    frame += 0.006;
    context.clearRect(0, 0, width, height);

    const gradient = context.createLinearGradient(0, 0, width, height);
    gradient.addColorStop(0, "rgba(255,255,255,0.32)");
    gradient.addColorStop(0.45, "rgba(114,189,255,0.24)");
    gradient.addColorStop(1, "rgba(255,218,158,0.2)");
    context.fillStyle = gradient;
    context.fillRect(0, 0, width, height);

    const ribbons = [
      ["rgba(71,151,255,0.16)", 0.24, 0.18],
      ["rgba(64,214,172,0.13)", 0.52, 0.11],
      ["rgba(255,181,90,0.12)", 0.74, 0.16],
      ["rgba(255,255,255,0.22)", 0.38, 0.08],
    ];

    ribbons.forEach(([color, base, amplitude], index) => {
      const top = height * base;
      const wave = height * amplitude;
      const drift = frame * (1 + index * 0.18) + index;

      context.beginPath();
      context.moveTo(-40, top + Math.sin(drift) * wave);
      context.bezierCurveTo(
        width * 0.22,
        top - wave * 1.8 + Math.cos(drift * 1.2) * wave,
        width * 0.42,
        top + wave * 1.2 + Math.sin(drift * 1.4) * wave,
        width * 0.64,
        top - wave * 0.4 + Math.cos(drift * 1.1) * wave,
      );
      context.bezierCurveTo(
        width * 0.82,
        top - wave * 1.4 + Math.sin(drift * 1.6) * wave,
        width + 50,
        top + Math.cos(drift) * wave,
        width + 80,
        top + wave,
      );
      context.lineTo(width + 80, top + wave * 3.2);
      context.bezierCurveTo(
        width * 0.78,
        top + wave * 2.2 + Math.cos(drift * 0.9) * wave,
        width * 0.36,
        top + wave * 3.4 + Math.sin(drift) * wave,
        -40,
        top + wave * 2.4,
      );
      context.closePath();
      context.fillStyle = color;
      context.fill();
    });

    window.requestAnimationFrame(paint);
  }

  resize();
  window.addEventListener("resize", resize);
  paint();
}

elements.toggleDaemon.addEventListener("click", toggleDaemon);
elements.search.addEventListener("input", () => {
  renderEvents();
  renderMemory();
});
elements.refreshFiles.addEventListener("click", () => loadSnapshot());
elements.normalizeMemory.addEventListener("click", normalizeMemory);
elements.exportJson.addEventListener("click", exportMemoryJson);
elements.ignoreCurrent.addEventListener("click", () => performFileAction("ignore", state.currentId));
elements.chooseCurrent.addEventListener("click", () => performFileAction("choose", state.currentId));
elements.moveCurrent.addEventListener("click", () => {
  const event = state.snapshot?.events?.find((item) => item.id === state.currentId);
  performFileAction(event?.hasSuggestion ? "move" : "choose", state.currentId);
});
elements.eventList.addEventListener("click", (event) => {
  const button = event.target.closest("button[data-action]");
  if (!button) {
    return;
  }

  performFileAction(button.dataset.action, button.dataset.id);
});

drawLiquidCanvas();
loadSnapshot({ quiet: true });
startPolling();
