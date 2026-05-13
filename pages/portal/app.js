const channels = {
  design: {
    title: "product-lab / agent-handoff",
    subtitle: "Scope: thread / explicit delivery",
    messages: [
      ["human", "canfeng", "handoff → @codex-joi：梳理这个 thread 的目标、阻塞点和下一步行动。"],
      ["agent", "codex-joi", "收到。已打开 turn，并从 channel 历史中读取相关事件与共享 artifact。"],
      ["service", "service:ci", "artifact linked：latest-run.json 已关联到当前 thread。"],
    ],
  },
  deploy: {
    title: "release-room / guarded-change",
    subtitle: "Scope: channel / approval required",
    messages: [
      ["service", "service:ci", "release candidate detected. waiting for human approval before promotion."],
      ["agent", "codex-joi", "风险集中在配置漂移和回滚窗口。建议先读取 trace，再执行发布。"],
    ],
  },
  incident: {
    title: "ops-bridge / live-triage",
    subtitle: "Cross-scope action requests",
    messages: [
      ["human", "canfeng", "把异常信号交给 ops agent，同时保留人工确认入口。"],
      ["agent", "codex-joi", "action.request 已进入 inbox。即使离开当前 scope，也不会丢失这个待处理决策。"],
    ],
  },
};

let currentChannel = "design";
let currentTab = "chat";
let memoryVersion = 1;
const memories = [
  "Actor has one shape across humans, agents and services",
  "Scope owns history, context and query boundaries",
  "Delivery comes from explicit handoff relations",
  "Turn groups streaming execution into a reviewable unit",
];

const messageList = document.querySelector("#message-list");
const taskBoard = document.querySelector("#task-board");
const memoryBoard = document.querySelector("#memory-board");
const scopeTitle = document.querySelector("#scope-title");
const scopeSubtitle = document.querySelector("#scope-subtitle");
const streamingBar = document.querySelector("#streaming-bar");
const promptForm = document.querySelector("#prompt-form");
const promptInput = document.querySelector("#prompt-input");
const downloadMeta = document.querySelector("#download-meta");
const downloadGrid = document.querySelector("#download-grid");

function renderMessages() {
  const state = channels[currentChannel];
  scopeTitle.textContent = state.title;
  scopeSubtitle.textContent = state.subtitle;
  messageList.innerHTML = state.messages
    .map(([kind, actor, text]) => `
      <article class="bubble bubble--${kind}">
        <span class="avatar avatar--${kind === "agent" ? "agent" : kind === "service" ? "service" : "human"}"></span>
        <div>
          <header><b>${actor}</b><small>${kind}</small></header>
          <p>${text}</p>
        </div>
      </article>
    `)
    .join("");
  messageList.scrollTop = messageList.scrollHeight;
}

function renderMemory() {
  memoryBoard.innerHTML = `
    <div class="memory-head">
      <b>Memory snapshot v${memoryVersion}</b>
      <button class="btn" id="memory-refresh">Refresh memory</button>
    </div>
    ${memories
      .map((item, index) => `<article><span>${String(index + 1).padStart(2, "0")}</span><p>${item}</p></article>`)
      .join("")}
  `;
  document.querySelector("#memory-refresh").addEventListener("click", refreshMemory);
}

function setTab(tab) {
  currentTab = tab;
  document.querySelectorAll(".tab").forEach((el) => el.classList.toggle("is-active", el.dataset.tab === tab));
  messageList.classList.toggle("hidden", tab !== "chat");
  taskBoard.classList.toggle("hidden", tab !== "tasks");
  memoryBoard.classList.toggle("hidden", tab !== "memory");
  if (tab === "memory") renderMemory();
}

function refreshMemory() {
  memoryVersion += 1;
  memories.unshift(`New local context snapshot captured at ${new Date().toLocaleTimeString()}`);
  memories.splice(5);
  renderMemory();
  streamingBar.textContent = `context snapshot refreshed / v${memoryVersion}`;
}

function appendMessage(kind, actor, text) {
  channels[currentChannel].messages.push([kind, actor, text]);
  renderMessages();
}

function renderDownloads() {
  const release = window.JOI_RELEASE_DOWNLOADS || {};
  const artifacts = Array.isArray(release.artifacts) ? release.artifacts : [];

  if (!downloadGrid || !downloadMeta) return;
  if (artifacts.length === 0) {
    downloadGrid.innerHTML = `
      <article class="download-card">
        <b>No release uploaded yet</b>
        <p>Run <code>scripts/package-release.sh</code> to build packages, upload them, and refresh this list.</p>
      </article>
    `;
    return;
  }

  downloadMeta.textContent = `Version ${release.version} / ${release.gitSha} / ${release.group}`;
  downloadGrid.innerHTML = artifacts
    .map((item) => `
      <article class="download-card download-card--${item.kind}">
        <small>${item.kind}</small>
        <b>${item.label}</b>
        <p>${formatBytes(item.size)} · sha256 ${String(item.sha256).slice(0, 12)}…</p>
        <a class="btn btn--primary" href="${item.downloadUrl}" target="_blank" rel="noreferrer">Download</a>
      </article>
    `)
    .join("");
}

function formatBytes(size) {
  if (!Number.isFinite(size)) return "-";
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

document.querySelectorAll(".channel").forEach((button) => {
  button.addEventListener("click", () => {
    currentChannel = button.dataset.channel;
    document.querySelectorAll(".channel").forEach((el) => el.classList.toggle("is-active", el === button));
    appendMessage("service", "joi-server", `scope subscribed：#${currentChannel} 的新事件会 fanout 到当前连接。`);
    setTab("chat");
  });
});

document.querySelectorAll(".tab").forEach((button) => {
  button.addEventListener("click", () => setTab(button.dataset.tab));
});

promptForm.addEventListener("submit", (event) => {
  event.preventDefault();
  const text = promptInput.value.trim();
  if (!text) return;
  appendMessage("human", "canfeng", text);
  promptInput.value = "";
  streamingBar.textContent = "codex-joi streaming...";
  setTimeout(() => {
    appendMessage("agent", "codex-joi", "已读取当前 scope 的历史，并把建议写回为 content.add。需要权限时会发出 action.request。");
    streamingBar.textContent = "codex-joi turn closed";
  }, 420);
});

document.querySelector("#handoff-button").addEventListener("click", () => {
  appendMessage("human", "canfeng", "handoff → @codex-joi：请接手这个 thread，并在需要执行外部动作前请求确认。");
});

document.querySelector("#trace-button").addEventListener("click", () => {
  appendMessage("agent", "codex-joi", "turn.trace.append：loaded scope history, resolved artifacts, prepared adapter prompt.");
});

document.querySelector("#approval-button").addEventListener("click", () => {
  appendMessage("service", "service:ci", "action.request：允许 agent 执行带副作用的外部操作吗？ choices=[Approve, Reject]");
});

document.querySelector("#reset-button").addEventListener("click", refreshMemory);

renderMessages();
renderMemory();
renderDownloads();
