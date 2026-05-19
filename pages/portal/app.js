const state = {
  view: "chat",
  scopeId: "product-lab",
  chatTab: "chat",
  taskMode: "board",
  selectedMember: "discovery",
  selectedMachine: "local-macbook",
  messageSeq: 6,
  toast: null,
};

const channels = [
  {
    id: "product-lab",
    title: "a1-dev-canfeng",
    visibility: "public",
    members: ["canfeng", "router", "discovery", "delivery", "service-ci"],
    threads: [
      { id: "discovery-desk", title: "discovery-desk", unread: 2 },
      { id: "deliver-task", title: "deliver-task", unread: 1 },
    ],
  },
  {
    id: "release-room",
    title: "release-room",
    visibility: "private",
    members: ["canfeng", "delivery", "service-ci"],
    threads: [
      { id: "guarded-change", title: "guarded-change", unread: 1 },
    ],
  },
  {
    id: "ops-bridge",
    title: "ops-bridge",
    visibility: "public",
    members: ["canfeng", "router", "ops-agent"],
    threads: [
      { id: "live-triage", title: "live-triage", unread: 0 },
    ],
  },
];

const messages = {
  "product-lab": [
    ["human", "canfeng", "handoff -> @router: 这条反馈像是已有缺陷，先判断是否需要进入修复流程。"],
    ["agent", "router", "已在 channel scope 打开判断 turn。复杂任务会创建 task 并交给 discovery。"],
    ["service", "joi-server", "event stored: content.add + hands_off_to(actor_router) / delivery pending"],
  ],
  "discovery-desk": [
    ["agent", "discovery", "已读取 root event、历史 artifact 和仓库索引，正在产出 task-brief.v1。"],
    ["service", "repo-cache", "artifact linked: clone_manifest.v1 已准备好 worktree / ro_link mounts。"],
    ["human", "canfeng", "DoD 里补一条：修复后必须附 CI 结果和 MR 链接。"],
  ],
  "deliver-task": [
    ["agent", "delivery", "已按 task-brief 进入 delivery thread，当前 cwd 来自 channel-aware workspace。"],
    ["service", "mr-detector", "mr-status-diff.v1 published: CI pending / no conflict / 1 reviewer comment."],
  ],
  "release-room": [
    ["service", "service:ci", "release candidate detected. waiting for human approval before promotion."],
    ["agent", "delivery", "风险集中在配置漂移和回滚窗口。建议先读取 trace，再执行发布。"],
  ],
  "guarded-change": [
    ["human", "canfeng", "先展示审批入口，执行动作必须可拒绝。"],
    ["service", "service:ci", "action.request: 允许 agent 执行带副作用的外部操作吗？"],
  ],
  "ops-bridge": [
    ["human", "canfeng", "把异常信号交给 ops agent，同时保留人工确认入口。"],
    ["agent", "router", "action.request 已进入 inbox。即使离开当前 scope，也不会丢失这个待处理决策。"],
  ],
  "live-triage": [
    ["service", "monitor", "latency p95 crossed threshold; trace bundle attached."],
  ],
};

const tasks = [
  { id: "task-184", channel: "product-lab", title: "Triage feedback and create task brief", status: "in_progress", owner: "discovery", artifacts: 2 },
  { id: "task-185", channel: "product-lab", title: "Fix existing bug with MR detector loop", status: "waiting_review", owner: "delivery", artifacts: 4 },
  { id: "task-186", channel: "release-room", title: "Approve guarded release promotion", status: "claimed", owner: "canfeng", artifacts: 1 },
  { id: "task-187", channel: "ops-bridge", title: "Route incident signal to ops agent", status: "todo", owner: "router", artifacts: 3 },
  { id: "task-188", channel: "product-lab", title: "Archive task result and release notes", status: "done", owner: "router", artifacts: 2 },
];

const actors = [
  { id: "canfeng", kind: "human", name: "canfeng", status: "online", machine: "local-macbook", model: "Human operator" },
  { id: "discovery", kind: "agent", name: "discovery", status: "streaming", machine: "local-macbook", model: "Claude / interactive_command" },
  { id: "delivery", kind: "agent", name: "delivery", status: "idle", machine: "runner-02", model: "Codex CLI / command" },
  { id: "router", kind: "agent", name: "router", status: "online", machine: "local-macbook", model: "Codex CLI" },
  { id: "service-ci", kind: "service", name: "service:ci", status: "waiting", machine: "runner-02", model: "CI service" },
];

const machines = [
  { id: "local-macbook", name: "Local MacBook", status: "connected", root: "~/.joi", agents: ["canfeng", "discovery", "router"], cpu: "14%", memory: "2.1 GB" },
  { id: "runner-02", name: "Runner 02", status: "remote", root: "/srv/joi", agents: ["delivery", "service-ci"], cpu: "38%", memory: "6.4 GB" },
];

const side = document.querySelector("#workbench-side");
const main = document.querySelector("#workbench-main");
const downloadMeta = document.querySelector("#download-meta");
const downloadGrid = document.querySelector("#download-grid");

function scope() {
  return (
    channels.find((channel) => channel.id === state.scopeId) ||
    channels.flatMap((channel) => channel.threads.map((thread) => ({ ...thread, channel }))).find((thread) => thread.id === state.scopeId)
  );
}

function scopeMessages() {
  return messages[state.scopeId] || [];
}

function setView(view) {
  state.view = view;
  if (view === "chat") state.chatTab = "chat";
  render();
}

function setScope(scopeId) {
  state.scopeId = scopeId;
  state.view = "chat";
  state.chatTab = "chat";
  render();
}

function pushToast(text) {
  state.toast = text;
  render();
  window.clearTimeout(pushToast.timer);
  pushToast.timer = window.setTimeout(() => {
    state.toast = null;
    render();
  }, 1800);
}

function appendMessage(kind, actor, text) {
  if (!messages[state.scopeId]) messages[state.scopeId] = [];
  messages[state.scopeId].push([kind, actor, text]);
  state.messageSeq += 1;
  render();
  const list = document.querySelector(".message-list");
  if (list) list.scrollTop = list.scrollHeight;
}

function renderSide() {
  const title = state.view === "tasks" ? "Tasks" : state.view === "inbox" ? "Inbox" : state.view === "members" ? "Members" : state.view === "machines" ? "Computers" : "Chat";
  side.className = "side-pane channels";
  side.innerHTML = `
    <header class="panel-title"><b>${title}</b><span>Connected workspace</span></header>
    <section>
      <button class="side-action" data-action="search">⌘K Search</button>
      <button class="side-action" data-view="inbox">△ Inbox <small>2</small></button>
      <button class="side-action" data-action="saved">◇ Saved</button>
    </section>
    <section>
      <div class="section-label">Channels ${channels.length}</div>
      ${channels
        .map((channel) => `
          <button class="channel ${state.scopeId === channel.id ? "is-active" : ""}" data-scope="${channel.id}">
            <span># ${channel.title}</span><small>${channel.members.length}</small>
          </button>
          <div class="thread-list">
            ${channel.threads
              .map((thread) => `
                <button class="thread ${state.scopeId === thread.id ? "is-active" : ""}" data-scope="${thread.id}">
                  <span>↳ ${thread.title}</span>${thread.unread ? `<small>${thread.unread}</small>` : ""}
                </button>
              `)
              .join("")}
          </div>
        `)
        .join("")}
      <div class="section-label">Direct Messages 0</div>
    </section>
  `;
}

function renderMain() {
  const routes = {
    chat: renderChat,
    tasks: renderTasks,
    inbox: renderInbox,
    members: renderMembers,
    machines: renderMachines,
    settings: renderSettings,
  };
  main.innerHTML = routes[state.view]();
  main.className = `workbench-main main-panel view-${state.view}`;
}

function renderChat() {
  const current = scope();
  const isThread = Boolean(current?.channel);
  const title = current?.title || current?.id || state.scopeId;
  const subtitle = isThread ? `in #${current.channel.title}` : current?.visibility === "private" ? "Private channel" : "Channel workspace";
  return `
    <header class="topbar">
      <div class="scope-icon">${isThread ? "□" : "#"}</div>
      <div><h2>${title}</h2><p>${subtitle}</p></div>
      <div class="topbar-actions">
        <button class="btn" data-action="stop">■</button>
        <button class="btn" data-action="rename">⚙</button>
        <button class="btn" data-view="members">☷ ${memberCount(current)}</button>
      </div>
    </header>
    <div class="tabs">
      <button class="tab ${state.chatTab === "chat" ? "is-active" : ""}" data-tab="chat">Chat</button>
      <button class="tab ${state.chatTab === "scopeTasks" ? "is-active" : ""}" data-tab="scopeTasks">Tasks</button>
    </div>
    ${
      state.chatTab === "chat"
        ? `
          <div class="announcement"><strong>handoff</strong><span>Directed delivery keeps the next actor, scope, task, and approval state visible.</span></div>
          <div class="content-area">
            <div class="message-list">
              ${scopeMessages().map(([kind, actor, text]) => bubble(kind, actor, text)).join("")}
            </div>
          </div>
          <div class="composer">
            <div class="streaming" id="streaming-bar">event stream open / ${state.messageSeq} events</div>
            <form id="prompt-form">
              <input id="prompt-input" autocomplete="off" placeholder="Message #${title}, @agent, or /task" />
              <button class="btn btn--primary" type="submit">Send</button>
            </form>
          </div>
        `
        : `
          <div class="content-area">
            <div class="task-board task-board--compact">
              ${tasks.filter((task) => task.channel === parentChannelId()).map(taskCard).join("")}
            </div>
          </div>
        `
    }
  `;
}

function renderTasks() {
  const grouped = ["todo", "claimed", "in_progress", "waiting_review", "done"];
  return `
    <header class="topbar">
      <div class="scope-icon">☑</div>
      <div><h2>Tasks</h2><p>${tasks.length} tasks anchored to channel messages</p></div>
      <div class="topbar-actions">
        <button class="btn ${state.taskMode === "board" ? "btn--primary" : ""}" data-mode="board">Board</button>
        <button class="btn ${state.taskMode === "list" ? "btn--primary" : ""}" data-mode="list">List</button>
        <button class="btn btn--primary" data-action="new-task">+ New Task</button>
      </div>
    </header>
    <div class="task-toolbar">
      <button class="btn"># ${parentChannelId()}</button>
      <span>task/create, task/update, artifacts and assignments</span>
    </div>
    <div class="content-area">
      ${
        state.taskMode === "board"
          ? `<div class="kanban">${grouped.map((status) => taskColumn(status)).join("")}</div>`
          : `<div class="task-list">${tasks.map(taskRow).join("")}</div>`
      }
    </div>
  `;
}

function renderInbox() {
  return `
    <header class="topbar">
      <div class="scope-icon">△</div>
      <div><h2>Inbox</h2><p>Pending action requests across scopes</p></div>
    </header>
    <div class="content-area inbox-list">
      <article class="approval-card">
        <b>service:ci requests deploy permission</b>
        <p>release-room / guarded-change wants to promote a candidate build.</p>
        <div><button class="btn btn--primary" data-action="approve">Approve</button><button class="btn" data-action="reject">Reject</button></div>
      </article>
      <article class="approval-card">
        <b>router requests ops handoff</b>
        <p>ops-bridge / live-triage needs an agent to read trace artifacts.</p>
        <div><button class="btn btn--primary" data-action="handoff">Handoff</button><button class="btn">Defer</button></div>
      </article>
    </div>
  `;
}

function renderMembers() {
  const selected = actors.find((actor) => actor.id === state.selectedMember) || actors[0];
  return `
    <header class="topbar">
      <div class="scope-icon">☷</div>
      <div><h2>Members</h2><p>Humans, agents and services share one actor shape</p></div>
      <div class="topbar-actions"><button class="btn btn--primary" data-action="invite">+ Invite actor</button></div>
    </header>
    <div class="member-layout">
      <aside class="member-list">
        ${actors.map((actor) => memberRow(actor)).join("")}
      </aside>
      <section class="member-detail">
        <div class="avatar avatar--${selected.kind}"></div>
        <h3>${selected.name}</h3>
        <p>${selected.kind} / ${selected.status}</p>
        <dl>
          <dt>Machine</dt><dd>${selected.machine}</dd>
          <dt>Runtime</dt><dd>${selected.model}</dd>
          <dt>Scope</dt><dd>${parentChannelId()}</dd>
        </dl>
        <button class="btn btn--primary" data-action="dm">Message</button>
        <button class="btn" data-action="copy">Copy actor id</button>
      </section>
    </div>
  `;
}

function renderMachines() {
  const selected = machines.find((machine) => machine.id === state.selectedMachine) || machines[0];
  return `
    <header class="topbar">
      <div class="scope-icon">▱</div>
      <div><h2>Computers</h2><p>Agent processes stay on local or connected machines</p></div>
      <div class="topbar-actions"><button class="btn btn--primary" data-action="add-machine">+ Add computer</button></div>
    </header>
    <div class="machine-layout">
      <aside class="machine-list">${machines.map(machineRow).join("")}</aside>
      <section class="machine-detail">
        <h3>${selected.name}</h3>
        <p>${selected.status} / ${selected.root}</p>
        <div class="metric-grid"><b>CPU ${selected.cpu}</b><b>MEM ${selected.memory}</b><b>${selected.agents.length} actors</b></div>
        <div class="agent-grid">
          ${selected.agents.map((id) => actors.find((actor) => actor.id === id)).filter(Boolean).map((actor) => `
            <article><span class="avatar avatar--${actor.kind}"></span><b>${actor.name}</b><small>${actor.model}</small></article>
          `).join("")}
        </div>
      </section>
    </div>
  `;
}

function renderSettings() {
  return `
    <header class="topbar">
      <div class="scope-icon">⚙</div>
      <div><h2>Settings</h2><p>Workspace profile and connection controls</p></div>
    </header>
    <div class="settings-panel">
      <label><span>Workspace</span><input value="Joi Lab" readonly /></label>
      <label><span>Server URL</span><input value="http://127.0.0.1:8787" readonly /></label>
      <label><span>Desktop notifications</span><button class="btn btn--primary" data-action="toggle">Enabled</button></label>
    </div>
  `;
}

function memberCount(current) {
  if (!current) return 0;
  if (current.members) return current.members.length;
  return current.channel?.members.length || 0;
}

function parentChannelId() {
  const current = scope();
  return current?.channel?.id || current?.id || "product-lab";
}

function bubble(kind, actor, text) {
  const mappedKind = kind === "service" ? "service" : kind === "agent" ? "agent" : "human";
  return `
    <article class="bubble bubble--${mappedKind}">
      <span class="avatar avatar--${mappedKind}"></span>
      <div>
        <header><b>${actor}</b><small>${kind}</small></header>
        <p>${text}</p>
      </div>
    </article>
  `;
}

function taskCard(task) {
  return `
    <article class="task-card" data-task="${task.id}">
      <b>${task.title}</b>
      <span>${task.id} / ${task.status}</span>
      <small>${task.owner} · ${task.artifacts} artifacts</small>
    </article>
  `;
}

function taskColumn(status) {
  const rows = tasks.filter((task) => task.status === status);
  return `
    <section class="task-column">
      <header>${status.replace("_", " ")} <small>${rows.length}</small></header>
      ${rows.map(taskCard).join("") || `<p class="empty-column">empty</p>`}
    </section>
  `;
}

function taskRow(task) {
  return `
    <article class="task-row">
      <b>${task.title}</b><span>${task.channel}</span><span>${task.owner}</span><span>${task.status}</span>
    </article>
  `;
}

function memberRow(actor) {
  return `
    <button class="member ${state.selectedMember === actor.id ? "is-active" : ""}" data-member="${actor.id}">
      <span class="avatar avatar--${actor.kind}"></span>
      <span><b>${actor.name}</b><small>${actor.kind} / ${actor.status}</small></span>
    </button>
  `;
}

function machineRow(machine) {
  return `
    <button class="machine-card ${state.selectedMachine === machine.id ? "is-active" : ""}" data-machine="${machine.id}">
      <strong>${machine.name}</strong><p>${machine.status} · ${machine.agents.length} actors</p><span class="status-dot"></span>
    </button>
  `;
}

function bindEvents() {
  document.querySelectorAll("[data-view]").forEach((button) => {
    button.addEventListener("click", () => setView(button.dataset.view));
  });
  document.querySelectorAll("[data-scope]").forEach((button) => {
    button.addEventListener("click", () => setScope(button.dataset.scope));
  });
  document.querySelectorAll("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      state.chatTab = button.dataset.tab;
      render();
    });
  });
  document.querySelectorAll("[data-mode]").forEach((button) => {
    button.addEventListener("click", () => {
      state.taskMode = button.dataset.mode;
      render();
    });
  });
  document.querySelectorAll("[data-member]").forEach((button) => {
    button.addEventListener("click", () => {
      state.selectedMember = button.dataset.member;
      render();
    });
  });
  document.querySelectorAll("[data-machine]").forEach((button) => {
    button.addEventListener("click", () => {
      state.selectedMachine = button.dataset.machine;
      render();
    });
  });
  document.querySelectorAll("[data-action]").forEach((button) => {
    button.addEventListener("click", () => handleAction(button.dataset.action));
  });
  const form = document.querySelector("#prompt-form");
  if (form) {
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      const input = document.querySelector("#prompt-input");
      const text = input.value.trim();
      if (!text) return;
      appendMessage("human", "canfeng", text);
      input.value = "";
      window.setTimeout(() => {
        appendMessage("agent", "router", "已读取当前 scope 历史，并把建议写回为 content.add。需要权限时会发出 action.request。");
      }, 360);
    });
  }
}

function handleAction(action) {
  const actions = {
    search: () => pushToast("Quick switch palette opened"),
    saved: () => pushToast("Saved view opened"),
    stop: () => appendMessage("service", "joi-daemon", "agent process control belongs to joi daemon; stop request acknowledged."),
    rename: () => pushToast("Rename channel modal opened"),
    approve: () => appendMessage("service", "service:ci", "action.result: approved by human; promotion started."),
    reject: () => appendMessage("service", "service:ci", "action.result: rejected by human; no external action executed."),
    handoff: () => appendMessage("human", "canfeng", "handoff -> @ops-agent: read trace artifacts and report next step."),
    "new-task": () => {
      tasks.unshift({ id: `task-${190 + tasks.length}`, channel: parentChannelId(), title: "New task from workspace", status: "todo", owner: "canfeng", artifacts: 0 });
      pushToast("task/create recorded");
    },
    invite: () => pushToast("Invite actor modal opened"),
    dm: () => pushToast("Direct message draft opened"),
    copy: () => pushToast("Actor id copied"),
    "add-machine": () => pushToast("Add computer modal opened"),
    toggle: () => pushToast("Setting toggled"),
  };
  (actions[action] || (() => pushToast(`${action} clicked`)))();
}

function renderDownloads() {
  const release = window.JOI_RELEASE_DOWNLOADS || {};
  const artifacts = Array.isArray(release.artifacts) ? release.artifacts : [];
  const publicArtifacts = artifacts
    .filter((item) => item.kind === "gui" || item.kind === "installer")
    .sort((a, b) => {
      const order = { gui: 0, installer: 1 };
      return (order[a.kind] ?? 9) - (order[b.kind] ?? 9);
    });
  if (!downloadGrid || !downloadMeta) return;
  if (publicArtifacts.length === 0) {
    downloadGrid.innerHTML = `
      <article class="download-card">
        <b>No release uploaded yet</b>
        <p>Run <code>scripts/package-release.sh --upload</code> to build packages, upload them, and refresh this list.</p>
      </article>
    `;
    return;
  }
  downloadMeta.textContent = `Version ${release.version} / ${release.gitSha} / ${release.group}`;
  downloadGrid.innerHTML = publicArtifacts
    .map((item) => {
      const isInstaller = item.kind === "installer";
      return `
      <article class="download-card download-card--${item.kind}">
        <small>${isInstaller ? "install.sh" : "dmg"}</small>
        <b>${isInstaller ? "Install script" : "Joi Desktop for macOS"}</b>
        <p>${formatBytes(item.size)} · sha256 ${String(item.sha256).slice(0, 12)}...</p>
        ${isInstaller ? `<pre>curl -fsSL ${item.downloadUrl} | sh -s -- --module all -y</pre>` : ""}
        <a class="btn btn--primary" href="${item.downloadUrl}" target="_blank" rel="noreferrer">${isInstaller ? "Download install.sh" : "Download DMG"}</a>
      </article>
    `;
    })
    .join("");
}

function formatBytes(size) {
  if (!Number.isFinite(size)) return "-";
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

function render() {
  document.querySelectorAll(".rail-icon").forEach((button) => {
    button.classList.toggle("is-active", button.dataset.view === state.view);
  });
  renderSide();
  renderMain();
  document.querySelector(".demo-frame").classList.toggle("has-toast", Boolean(state.toast));
  let toast = document.querySelector(".toast");
  if (state.toast && !toast) {
    toast = document.createElement("div");
    toast.className = "toast";
    document.querySelector(".desktop-window").appendChild(toast);
  }
  if (toast) {
    toast.textContent = state.toast || "";
    toast.classList.toggle("hidden", !state.toast);
  }
  bindEvents();
}

document.querySelector("#reset-button").addEventListener("click", () => {
  state.view = "chat";
  state.scopeId = "product-lab";
  state.chatTab = "chat";
  pushToast("Context refreshed");
});

render();
renderDownloads();
