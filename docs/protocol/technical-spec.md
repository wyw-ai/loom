# Joi (AgentHub) 技术说明书

> 版本：2.0 · 日期：2026-03-28
>
> 注意：这份文档保留的是 `joi` 原型阶段语义，其中 `@mention` 仍承担机器路由职责。
> `joi-apps` 当前权威协议语义以
> [open-multi-actor-collaboration-protocol-v0.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol/open-multi-actor-collaboration-protocol-v0.md)
> 和
> [open-multi-actor-collaboration-schema-v0.md](/Users/bojun.cbj/Workspace/gitlab.alibaba-inc.com/bojun.cbj/joi-apps/docs/protocol/open-multi-actor-collaboration-schema-v0.md)
> 为准：
> `@handle` 属于 binding 层文本语法，显式执行交接使用 `handoff`，持续上下文归属使用 `Membership`。

---

## 目录

1. [系统概述](#1-系统概述)
2. [技术栈与架构](#2-技术栈与架构)
3. [数据库设计](#3-数据库设计)
4. [Agent 规格系统（Specs）](#4-agent-规格系统specs)
5. [Agent 注册中心与 Marketplace](#5-agent-注册中心与-marketplace)
6. [Agent 通信机制（ACP 协议）](#6-agent-通信机制acp-协议)
7. [Agent 记忆系统（Memory）](#7-agent-记忆系统memory)
8. [内置 MCP 服务器](#8-内置-mcp-服务器)
9. [线程上下文服务（ThreadContext）](#9-线程上下文服务threadcontext)
10. [信息泳道：消息的接收策略](#10-信息泳道消息的接收策略)
11. [信息泳道：消息的发送策略](#11-信息泳道消息的发送策略)
12. [@Mention 路由机制](#12-mention-路由机制)
13. [消息队列与并发控制](#13-消息队列与并发控制)
14. [权限请求（Action）机制](#14-权限请求action机制)
15. [前端事件驱动架构](#15-前端事件驱动架构)
16. [完整数据流图](#16-完整数据流图)
17. [数据目录结构](#17-数据目录结构)
18. [构建与部署](#18-构建与部署)

---

## 1. 系统概述

Joi（内部代号 AgentHub / agentx）是一个**本地优先的多 Agent 协作平台**，形态类似 Discord，但频道的参与者除了人类用户，还可以是 AI Agent（任何支持 ACP 协议的 CLI 工具）。平台核心目标：

- 支持人类与 Agent 的自然对话（@mention 路由）
- 支持 Agent 与 Agent 之间的协作（Agent A @mention Agent B，或通过 Joi handoff 协议的 MCP / CLI 入口）
- 对 Agent 的工具调用、权限请求提供可视化追踪
- 保持完全本地运行，数据存储于 `~/.agentx/data.db`
- 提供 Agent Marketplace，支持一键从 ACP 官方注册中心安装 Agent
- 为每个 Agent 提供独立的记忆系统（JSONL 存储）

---

## 2. 技术栈与架构

### 2.1 整体架构

```
┌──────────────────────────────────────────────────────────┐
│                    Tauri Desktop App ("Joi")              │
│                                                          │
│  ┌──────────────────────┐    ┌─────────────────────────┐ │
│  │   React Frontend     │    │    Rust Backend          │ │
│  │   (WebView)          │◄──►│    (Native Process)     │ │
│  │                      │    │                         │ │
│  │  Zustand Store       │    │  AgentManager           │ │
│  │  Tauri invoke()      │    │  AgentRegistry          │ │
│  │  Tauri listen()      │    │  AcpAdapter             │ │
│  │                      │    │  MemoryStore            │ │
│  │                      │    │  ThreadContextService   │ │
│  │                      │    │  SQLite DB              │ │
│  └──────────────────────┘    └───────────┬─────────────┘ │
│                                          │               │
└──────────────────────────────────────────┼───────────────┘
                                           │ stdin/stdout (ACP JSON-RPC)
                              ┌────────────▼────────────┐
                              │     Agent Process       │
                              │  (Claude Code / Codex / │
                              │   任何 ACP agent)       │
                              │                         │
                              │  ┌───────────────────┐  │
                              │  │  MCP Server 1:    │  │
                              │  │  agentx-conversa… │  │
                              │  │  (对话历史)        │  │
                              │  └───────────────────┘  │
                              │  ┌───────────────────┐  │
                              │  │  MCP Server 2:    │  │
                              │  │  agentx-memory    │  │
                              │  │  (Agent 记忆)     │  │
                              │  └───────────────────┘  │
                              └─────────────────────────┘
```

### 2.2 技术栈

| 层次 | 技术 | 版本 |
|------|------|------|
| 桌面框架 | Tauri v2 | 2.x |
| 前端框架 | React | 19.1 |
| 前端语言 | TypeScript | 5.8 |
| 前端样式 | Tailwind CSS | v4.2 |
| 前端状态 | Zustand | 5.x |
| 前端虚拟列表 | @tanstack/react-virtual | 3.x |
| Markdown 渲染 | react-markdown + remark-gfm | 10.x |
| 代码高亮 | react-syntax-highlighter | 16.x |
| 后端语言 | Rust | stable (edition 2021) |
| 数据库 | SQLite（rusqlite 0.31, bundled） | WAL 模式 |
| 异步运行时 | Tokio | 1.x (full) |
| Agent 通信 | ACP over stdio（JSON-RPC 2.0） | - |
| Agent 配置 | YAML（serde_yaml） | - |
| HTTP 客户端 | reqwest（blocking, rustls-tls） | 0.12 |
| 文件类型推断 | infer | 0.19 |

### 2.3 模块结构

```
discord/
├── src-tauri/src/
│   ├── lib.rs                  # Tauri 入口、CLI 模式检测、注册 commands
│   ├── main.rs                 # main() → maybe_run_cli_mode() || run()
│   ├── agents/
│   │   ├── adapter.rs          # AgentEvent 枚举、AgentStatus 定义
│   │   ├── acp.rs              # ACP 协议适配器（核心通信层）
│   │   ├── context.rs          # PromptEnvelope：消息上下文格式化
│   │   ├── manager.rs          # AgentManager：生命周期 & 消息调度
│   │   └── router.rs           # @mention 解析 & 目标路由
│   ├── commands/
│   │   ├── agent_commands.rs   # Agent CRUD + 记忆 + 启停 + Marketplace
│   │   ├── channel_commands.rs # 频道 CRUD
│   │   ├── message_commands.rs # 消息发送/查询 + 附件 + 线程
│   │   └── settings_commands.rs # 代理设置
│   ├── db/
│   │   ├── mod.rs              # Database 结构体、agentx_home_dir()
│   │   ├── schema.rs           # SQL 建表 & 迁移逻辑
│   │   ├── messages.rs         # 消息 CRUD、搜索、读窗口
│   │   ├── channels.rs         # 频道操作
│   │   ├── agents.rs           # Agent 会话记录（agent_sessions 表）
│   │   ├── threads.rs          # 线程管理
│   │   ├── action_requests.rs  # 权限请求
│   │   ├── agent_handoffs.rs   # Agent 间任务交接记录
│   │   └── app_settings.rs     # 全局设置（代理等）
│   ├── handoffs.rs             # Handoff 核心服务：校验、创建、查询、状态更新
│   ├── memory/
│   │   ├── store.rs            # MemoryRecord / MemoryQuery / MemoryStore trait
│   │   ├── jsonl_store.rs      # JSONL 文件实现，按月分片
│   │   ├── selector.rs         # MemorySelector：Bootstrap & Turn 记忆选取
│   │   └── renderer.rs         # MemoryRenderer：渲染记忆为文本
│   ├── mcp/
│   │   ├── mod.rs              # CLI 分发：内置 MCP / collaboration CLI
│   │   ├── conversation.rs     # 对话历史 / handoff / artifact MCP 服务器（11 个工具）
│   │   └── memory.rs           # Agent 记忆 MCP 服务器（3 个工具）
│   ├── cli/
│   │   ├── mod.rs              # collaboration CLI 分发
│   │   ├── artifact.rs         # 共享工件 CLI：publish/list/get/read
│   │   └── handoff.rs          # 交接 CLI：create/list/get/claim/update-status
│   ├── registry/
│   │   └── agent_registry.rs   # AgentRegistry：发现、CRUD、Marketplace 安装
│   ├── specs/
│   │   ├── agent_spec.rs       # AgentSpec YAML 结构体（完整 Agent 配置）
│   │   ├── defaults.rs         # PlatformDefaults：全局默认配置
│   │   └── tool_policy.rs      # ToolPolicy：MCP 服务器 & 允许的工具列表
│   └── thread_context/
│       └── service.rs          # ThreadContextService：线程摘要、决策、交接
└── src/
    ├── App.tsx                 # 根布局、模态管理
    ├── main.tsx                # React 入口
    ├── types/index.ts          # TypeScript 接口定义
    ├── hooks/
    │   ├── useAgentEvents.ts   # Tauri 事件订阅
    │   └── useMention.ts       # @mention 弹窗逻辑
    ├── stores/
    │   ├── messageStore.ts     # 消息 & 线程状态
    │   ├── channelStore.ts     # 频道列表 & 未读计数
    │   ├── agentStore.ts       # Agent 列表 & 状态 & Marketplace
    │   ├── settingsStore.ts    # 代理设置
    │   ├── uiStore.ts          # UI 状态（模态开关等）
    │   └── notificationStore.ts # 通知队列
    ├── components/
    │   ├── layout/
    │   │   ├── Sidebar.tsx     # 频道列表 + Agent 列表 + 用户面板
    │   │   └── MainPanel.tsx   # 频道头 + 消息列表 + 输入框
    │   ├── message/
    │   │   ├── MessageList.tsx  # 虚拟滚动消息列表
    │   │   ├── MessageInput.tsx # 富文本输入 + @mention + 附件上传
    │   │   ├── MessageItem.tsx  # 单条消息渲染 + 执行时间线
    │   │   ├── MessageMarkdown.tsx  # Markdown 渲染 + 代码高亮
    │   │   ├── ActionCard.tsx   # 权限/输入请求可视化卡片
    │   │   ├── MentionPopup.tsx # @mention 候选弹窗
    │   │   └── AttachmentPreviewModal.tsx  # 附件预览
    │   ├── agent/
    │   │   ├── AgentList.tsx    # Agent 列表 + 状态指示器
    │   │   ├── AgentConfigPanel.tsx  # Agent 设置面板
    │   │   └── AgentAvatar.tsx  # Agent 头像组件
    │   ├── channel/
    │   │   ├── ChannelList.tsx  # 频道列表 + 未读角标
    │   │   └── CreateChannelModal.tsx  # 新建频道弹窗
    │   └── app/
    │       ├── AppErrorBoundary.tsx    # 错误边界
    │       ├── NotificationCenter.tsx  # Toast 通知
    │       └── ProxySettingsModal.tsx  # HTTP 代理设置
    └── styles/
        └── index.css           # Tailwind CSS 入口
```

---

## 3. 数据库设计

数据库路径：`~/.agentx/data.db`，使用 SQLite（WAL 模式）。

> **注意**：Agent 配置不再存储在数据库中，而是以 YAML 文件形式存储在 `~/.agentx/agents/<id>/spec.yaml`。数据库中保留 `agent_sessions` 表用于追踪运行时会话。

### 3.1 核心表

```sql
-- 频道
CREATE TABLE channels (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- 消息（泳道的核心存储单元）
CREATE TABLE messages (
    id            TEXT PRIMARY KEY,
    channel_id    TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    thread_id     TEXT,                -- NULL 表示主频道
    sender_type   TEXT NOT NULL CHECK(sender_type IN ('user', 'agent', 'system')),
    sender_name   TEXT NOT NULL,
    content       TEXT NOT NULL,
    content_type  TEXT NOT NULL DEFAULT 'text'
                  CHECK(content_type IN ('text', 'action_card', 'tool_output', 'system', 'image')),
    metadata      TEXT,                -- JSON: {turn_id, tool_trace, action_trace, attachments}
    reply_to      TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE INDEX idx_messages_channel ON messages(channel_id, created_at);
CREATE INDEX idx_messages_thread ON messages(thread_id, created_at);

-- 线程（对某条消息的回复线）
CREATE TABLE threads (
    id              TEXT PRIMARY KEY,
    channel_id      TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    root_message_id TEXT NOT NULL,
    title           TEXT NOT NULL DEFAULT '',
    created_at      INTEGER NOT NULL
);

-- Agent 运行时会话
CREATE TABLE agent_sessions (
    id            TEXT PRIMARY KEY,
    agent_name    TEXT NOT NULL,
    channel_id    TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    thread_id     TEXT,
    tmux_session  TEXT,              -- 预留字段
    pid           INTEGER,
    status        TEXT NOT NULL DEFAULT 'starting'
                  CHECK(status IN ('starting', 'active', 'idle', 'stopping', 'stopped', 'error')),
    session_data  TEXT,              -- 预留 JSON
    started_at    INTEGER NOT NULL,
    last_active   INTEGER NOT NULL
);

-- 权限/输入请求
CREATE TABLE action_requests (
    id              TEXT PRIMARY KEY,
    message_id      TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    agent_name      TEXT NOT NULL,
    request_type    TEXT NOT NULL CHECK(request_type IN ('permission', 'input', 'choice')),
    payload         TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK(status IN ('pending', 'approved', 'denied', 'expired')),
    response        TEXT,
    created_at      INTEGER NOT NULL,
    responded_at    INTEGER
);

-- Agent 间任务交接
CREATE TABLE agent_handoffs (
    id                  TEXT PRIMARY KEY,
    channel_id          TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    source_agent_name   TEXT NOT NULL,
    target_agent_name   TEXT NOT NULL,
    thread_id           TEXT,
    message             TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'pending'
                        CHECK(status IN ('pending', 'dispatching', 'queued', 'failed')),
    error               TEXT,
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL
);

CREATE INDEX idx_agent_handoffs_pending
ON agent_handoffs(channel_id, source_agent_name, status, created_at);

-- 全局设置（如代理配置）
CREATE TABLE app_settings (
    key             TEXT PRIMARY KEY,
    value           TEXT NOT NULL
);
```

### 3.2 数据库迁移

`schema.rs` 中的 `migrate()` 函数负责增量迁移：

- 检测旧版 `agents` 表（含 `cli_stream`/`cli_tmux` 类型），自动迁移为 `acp_stdio`
- 检测 `agent_sessions` 表的外键变更（移除对 agents 表的引用）
- 检测 `agent_handoffs` 表的外键变更
- 所有迁移在事务中执行，使用 `PRAGMA foreign_keys=OFF/ON` 包围

### 3.3 消息元数据 JSON 结构

`messages.metadata` 字段是追踪 Agent 执行过程的核心载体：

```json
{
  "turn_id": "uuid-本轮对话标识",
  "attachments": [
    {
      "id": "attachment-uuid",
      "name": "schema.sql",
      "path": "/home/user/.agentx/channels/<channel-id>/attachments/<message-id>/schema.sql",
      "mime": "text/plain",
      "size": 1024,
      "kind": "code",
      "previewable": true
    }
  ],
  "tool_trace": {
    "items": [
      {
        "id": "trace-item-id",
        "tool_name": "bash",
        "input": { "command": "ls -la" },
        "update_count": 3,
        "created_at": 1711234567890
      }
    ]
  },
  "action_trace": {
    "items": [
      {
        "action_id": "action-id",
        "request_type": "permission",
        "title": "Permission required: run bash command",
        "description": "Agent wants to execute: rm -rf /tmp/cache",
        "choices": [
          { "id": "opt_approve", "label": "Approve" },
          { "id": "opt_deny", "label": "Deny" }
        ],
        "status": "approved",
        "selected_option_id": "opt_approve",
        "selected_label": "Approve",
        "responded_at": 1711234567890
      }
    ]
  }
}
```

---

## 4. Agent 规格系统（Specs）

Agent 不再使用数据库行存储配置，而是采用 **YAML 规格文件**体系。每个 Agent 是一个目录：

```
~/.agentx/agents/<agent-id>/
├── spec.yaml           # 主配置文件（AgentSpec）
├── identity.md         # Agent 身份描述
├── soul.md             # Agent 行为风格
├── tools.yaml          # 工具策略（ToolPolicy）
└── memory/
    ├── meta.json       # 记忆存储元信息
    └── records/
        └── 2026-03.jsonl  # 按月分片的记忆文件
```

### 4.1 AgentSpec 完整结构

```yaml
# spec.yaml
apiVersion: agentx/v1
kind: AgentSpec

metadata:
  id: claude-code                    # 唯一标识符（字母/数字/-/_）
  handle: claude-code                # @mention 使用的句柄
  displayName: Claude Code           # 显示名称
  enabled: true                      # 是否启用

source:                              # 可选：来源信息
  type: acp_registry                 # "local" | "acp_registry"
  registry: official-acp
  registryAgentId: claude-code
  installedVersion: "0.1.0"

identity:
  files:
    identity: ./identity.md          # Agent 身份描述文件路径
    soul: ./soul.md                  # Agent 行为风格文件路径

runtime:
  protocol: acp                      # 协议类型（目前仅支持 "acp"）
  transport:                         # 传输配置（二选一）
    type: stdio                      # "stdio" 或 "remote"
    command: claude                   # 可执行程序路径
    args: ["--acp"]                  # 命令行参数
    env:                             # 额外环境变量
      ANTHROPIC_API_KEY: sk-xxx
    authMethod: ""                   # 认证方式（如 "cursor_login"）
    workdir: "{agent.workspace}"     # 工作目录模板（{channel.workspace} 兼容映射到同一路径）

tools:
  policyFile: ./tools.yaml           # 工具策略文件路径

memory:
  store:
    storeType: jsonl                 # 存储类型
    root: ./memory/records           # 记忆文件目录
    shardBy: month                   # 分片策略
  query:
    mode: heuristic                  # 查询模式
    bootstrapTopK: 8                 # Bootstrap 阶段返回的记忆条数
    turnTopK: 4                      # 每轮对话返回的相关记忆条数
  delivery:
    mode: hybrid                     # 交付模式
    prompt: true                     # 是否在 prompt 中注入记忆
    mcp: true                        # 是否通过 MCP 提供记忆查询
    native: opaque                   # 原生交付模式
  extraction:
    mode: disabled                   # 记忆提取模式
  compaction:
    mode: disabled                   # 记忆压缩模式

context:
  window: 50                         # 历史消息上下文数量上限
  pushMode: mention_only             # "mention_only" | "all"
  handoffMode: manual                # "manual" | "auto"
  systemPromptAppend: ""             # Bootstrap 追加的系统提示

session:
  idleTimeoutSecs: 300               # 空闲超时（秒）
```

### 4.2 TransportSpec：传输层抽象

系统支持两种传输方式：

| 类型 | 使用场景 | 配置 |
|------|---------|------|
| `stdio` | 本地子进程，通过 stdin/stdout 通信 | command, args, env, workdir, authMethod |
| `remote` | 远程 ACP 服务端（预留） | endpoint, headers, workdir |

### 4.3 ToolPolicy：工具策略

```yaml
# tools.yaml
apiVersion: agentx/v1
kind: ToolPolicy

platform:
  mcpServers:                        # 允许连接的平台 MCP 服务器
    - agentx-conversation
    - agentx-memory
  allowTools:                        # 允许 Agent 调用的工具白名单
    - conversation_list_recent
    - conversation_search
    - conversation_read_window
    - conversation_list_threads
    - agent_handoff
    - handoff_list
    - handoff_get
    - artifact_publish
    - artifact_list
    - artifact_get
    - artifact_read
    - memory_search
    - memory_get
    - memory_list_recent

runtimeNative:
  policy: observe                    # 对 Agent 原生工具的策略："observe" | "block"
```

### 4.4 PlatformDefaults：全局默认

存储在 `~/.agentx/config/defaults.yaml`，为新安装的 Agent 提供默认配置：

```yaml
apiVersion: agentx/v1
kind: PlatformDefaults

runtime:
  protocol: acp

context:
  window: 50
  pushMode: mention_only
  handoffMode: manual

session:
  idleTimeoutSecs: 300

memory:
  query:
    mode: heuristic
    bootstrapTopK: 8
    turnTopK: 4
  delivery:
    mode: hybrid
    prompt: true
    mcp: true
    native: opaque
  extraction:
    mode: disabled
  compaction:
    mode: disabled
```

### 4.5 Identity 与 Soul

每个 Agent 有两个 Markdown 身份文件：

- **identity.md**：描述 Agent 的角色、职责、非目标
- **soul.md**：描述 Agent 的沟通风格和行为偏好

示例 identity.md：
```markdown
# Claude Code

- Role: collaborative ACP agent
- Primary responsibility: respond clearly, act within Joi, and keep work grounded in provided context.
- Non-goals: do not invent capabilities you cannot access.
```

创建 Agent 时如果未提供这些文件，系统会自动生成默认内容。

---

## 5. Agent 注册中心与 Marketplace

### 5.1 AgentRegistry

`AgentRegistry` 是 Agent 发现和管理的核心组件：

```rust
pub struct AgentRegistry {
    home_dir: PathBuf,  // 默认 ~/.agentx
}
```

**核心功能：**

| 方法 | 说明 |
|------|------|
| `ensure_layout()` | 初始化目录结构（agents/, config/, registries/, runtimes/） |
| `list_agents()` | 扫描 `agents/` 目录，返回所有 AgentSummary |
| `list_enabled_agents()` | 仅返回 enabled=true 的 Agent |
| `get_agent_document(id)` | 加载完整 AgentDocument（spec + identity + soul + policy） |
| `load_agent(id)` | 加载 LoadedAgent（含 root_dir） |
| `save_agent_document(doc)` | 写入 Agent 到文件系统 |
| `delete_agent(id)` | 删除 Agent 目录 |
| `find_by_handle(handle)` | 按 handle 或 id 查找 Agent |

### 5.2 Marketplace

Marketplace 从 ACP 官方注册中心获取可安装的 Agent 列表：

```
注册中心 URL: https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json
缓存路径:     ~/.agentx/registries/official-acp/registry.json
```

**安装流程：**

```
用户点击 "Install" → invoke('install_marketplace_agent', {
    marketplaceId: "claude-code",
    localAgentId: "claude-code",     // 可选自定义 ID
    displayName: "Claude"            // 可选自定义名称
})
    │
    ▼
AgentRegistry.install_marketplace_agent_with_proxy()
    │
    ├─ 1. 下载 registry.json（支持代理，失败时读缓存）
    ├─ 2. 在注册表中查找 marketplace_id
    ├─ 3. 加载 PlatformDefaults 作为基础配置
    ├─ 4. 根据分发方式生成 TransportSpec：
    │      ├─ npx → StdioTransportSpec { command: "npx", args: ["-y", package, ...] }
    │      ├─ uvx → StdioTransportSpec { command: "uvx", args: [package, ...] }
    │      └─ binary → 下载归档 → 解压 → 符号链接到 runtimes/bin/
    ├─ 5. 生成 identity.md / soul.md / tools.yaml
    ├─ 6. 写入 spec.yaml 并保存到 agents/<id>/
    └─ 7. 返回 AgentDocument
```

**分发类型支持：**

| 类型 | 平台 | 安装方式 |
|------|------|---------|
| `npx` | 全平台 | 通过 `npx -y <package>` 运行 |
| `uvx` | 全平台 | 通过 `uvx <package>` 运行 |
| `binary` | 按平台 | 下载 tar.gz/zip → 解压 → 符号链接到 `runtimes/bin/` |

平台标识格式：`darwin-aarch64`, `darwin-x86_64`, `linux-x86_64`, `windows-x86_64` 等。

---

## 6. Agent 通信机制（ACP 协议）

### 6.1 协议概述

Joi 使用 **ACP（Agent Communication Protocol）**，基于以下设计原则：

- **传输层**：进程 stdio（stdin 写入命令，stdout 读取响应）
- **编码格式**：JSON-RPC 2.0，每个消息占一行（newline-delimited）
- **生命周期**：每个 Agent 实例对应一个长运行子进程
- **协议版本**：通过 initialize 握手协商

### 6.2 连接建立（Handshake）

```
Rust (AgentManager)           Agent Process
       │                            │
       │── spawn child process ────►│
       │                            │ (process starts)
       │──── initialize ───────────►│
       │◄─── initialize response ───│
       │                            │
       │──── authenticate ─────────►│  (如 authMethod 已配置)
       │◄─── authenticate response ─│
       │                            │
       │──── session/new ──────────►│
       │◄─── session/new response ──│
       │                            │
       │     [连接建立完成]          │
```

**initialize 请求：**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": 1,
    "clientInfo": { "name": "agentx", "title": "Joi", "version": "0.1.0" },
    "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false }
  }
}
```

**session/new 请求（注册 MCP 服务器）：**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "session/new",
  "params": {
    "cwd": "/home/user/.agentx/channels/<channel-id>/agents/<agent-id>/workspace",
    "mcpServers": [
      {
        "name": "agentx-conversation",
        "command": "/path/to/joi",
        "args": ["--agentx-conversation-mcp", "--channel-id", "<channel-id>", "--source-agent", "<agent-name>"]
      },
      {
        "name": "agentx-memory",
        "command": "/path/to/joi",
        "args": ["--agentx-memory-mcp", "--agent-id", "<agent-name>"]
      }
    ]
  }
}
```

> Joi 的二进制文件兼做 MCP 服务器。启动时检测 `--agentx-conversation-mcp` 或 `--agentx-memory-mcp` 参数，进入 CLI 模式运行对应的 MCP stdio 服务器，而不是启动 Tauri GUI。

### 6.3 消息交互（Session 层）

建立 Session 后，通信进入双向异步模式：

```
Rust                               Agent
 │                                   │
 │──── session/prompt ──────────────►│
 │     (包含 bootstrap 上下文)         │
 │                                   │ (Agent 开始执行)
 │◄─── session/update (Text) ────────│  持续推送文本块
 │◄─── session/update (Text) ────────│
 │◄─── session/request_permission ───│  (需要权限时)
 │──── session/permission_response ─►│  (用户批准/拒绝后)
 │◄─── session/update (ToolUse) ─────│  工具调用通知
 │◄─── session/update (Finished) ────│  执行完成
```

**session/prompt 请求结构：**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "session/prompt",
  "params": {
    "sessionId": "session-uuid",
    "prompt": [{
      "type": "text",
      "text": "[Bootstrap 上下文 + 消息历史 + 最新消息]"
    }]
  }
}
```

### 6.4 事件类型（AgentEvent）

Rust 定义的 `AgentEvent` 枚举是所有 Agent 输出的统一抽象：

```rust
pub enum AgentEvent {
    // Agent 产生文本输出（流式，is_partial=true 表示还在继续）
    Text {
        content: String,
        is_partial: bool,
    },
    // Agent 请求用户权限/输入/选择
    ActionRequest {
        id: String,
        request_type: String,   // "permission" | "input" | "choice"
        title: String,
        description: String,
        choices: Vec<ActionChoice>,
    },
    // Agent 调用了某个工具
    ToolUse {
        tool_name: String,
        input: serde_json::Value,
    },
    // Agent 运行状态变化
    StatusChange {
        status: String,         // "active" | "idle" | "error"
    },
    // 本轮任务执行完毕
    Finished {
        success: bool,
        summary: Option<String>,
    },
    // 发生错误
    Error {
        message: String,
    },
}
```

### 6.5 Bootstrap 上下文

每次 Agent 收到新消息时，系统构造完整的 Bootstrap 上下文：

```
┌─────────────────────────────────────────────────────┐
│  [系统介绍]                                          │
│  You are in Joi, a collaborative environment.       │
│  Channel: #channel-name                             │
│                                                     │
│  [可用 Agent 列表]                                   │
│  Other available agents: @agent-a, @agent-b         │
│                                                     │
│  [协作指导]（如有 MCP 可用）                           │
│  Use conversation and memory tools to collaborate.  │
│                                                     │
│  [自定义系统提示]（如有配置）                          │
│  [system_prompt_append 内容]                        │
│                                                     │
│  [消息历史上下文]（PromptEnvelope）                   │
│  包括 channel_name, scope, recent_messages,         │
│  latest_message, thread_context                     │
│                                                     │
│  [当前用户消息]                                      │
│  Current turn:                                      │
│  @claude 请帮我重构这个函数                          │
└─────────────────────────────────────────────────────┘
```

**PromptEnvelope 结构（context.rs）：**

```rust
pub struct PromptEnvelope {
    pub channel_name: String,
    pub available_agents: Vec<String>,
    pub latest_message: Message,
    pub scope: PromptScope,
}

pub enum PromptScope {
    Main {
        recent_messages: Vec<Message>,
    },
    Thread {
        thread_id: String,
        thread_title: String,
        root_message: Message,
        recent_replies: Vec<Message>,
        recent_channel_messages: Vec<Message>,
    },
}
```

---

## 7. Agent 记忆系统（Memory）

每个 Agent 有独立的持久化记忆系统，存储为 JSONL 文件。

### 7.1 MemoryRecord 结构

```rust
pub struct MemoryRecord {
    pub schema_version: u32,     // 固定为 1
    pub id: String,              // "mem_<uuid>"
    pub agent_id: String,        // Agent 标识
    pub ts: String,              // RFC3339 时间戳
    pub record_type: String,     // "fact" | "experience" | "strategy" | "preference" | ...
    pub status: String,          // "accepted" | "pending" | "rejected"
    pub summary: String,         // 记忆摘要
    pub detail: String,          // 详细内容
    pub confidence: String,      // "high" | "medium" | "low"
    pub source: MemorySource,    // 来源（channel_id, thread_id, message_ids）
    pub tags: Vec<String>,       // 标签
}
```

### 7.2 JSONL 存储

```
~/.agentx/agents/<agent-id>/memory/records/
├── 2026-01.jsonl
├── 2026-02.jsonl
└── 2026-03.jsonl
```

- **按月分片**（`shard_path()`）：根据 `ts` 字段的年月生成文件名 `YYYY-MM.jsonl`
- 每行一条 JSON 记录，追加写入
- 查询时加载所有分片，按时间倒序排列
- 仅 `status == "accepted"` 的记录在默认查询中返回

### 7.3 MemoryStore Trait

```rust
pub trait MemoryStore: Send + Sync {
    fn list_recent(&self, limit: usize) -> Result<Vec<MemoryRecord>, String>;
    fn get(&self, id: &str) -> Result<Option<MemoryRecord>, String>;
    fn query(&self, query: &MemoryQuery) -> Result<Vec<MemoryRecord>, String>;
    fn append(&self, record: &MemoryRecord) -> Result<(), String>;
}
```

查询支持：
- **文本搜索**：在 summary + detail + tags 中做大小写不敏感的子串匹配
- **标签过滤**：必须同时匹配所有指定标签
- **类型过滤**：按 record_type 过滤
- **数量限制**：truncate 到指定条数

### 7.4 MemorySelector：记忆选取策略

```rust
pub struct MemorySelector {
    config: AgentMemorySpec,
}
```

两个选取阶段：

1. **Bootstrap 阶段**（`select_bootstrap`）：
   - 取最近 `bootstrap_top_k`（默认 8）条已接受记忆
   - 按 confidence 排序（high > medium > low）

2. **Turn 阶段**（`select_for_turn`）：
   - 从最新消息和线程上下文中提取关键词（4+ 字符，最多 12 个）
   - 用关键词查询记忆
   - 如果无匹配结果，降级为 list_recent
   - 返回最多 `turn_top_k`（默认 4）条记忆

### 7.5 MemoryRenderer：渲染为文本

```rust
impl MemoryRenderer {
    pub fn render_bootstrap(records: &[MemoryRecord]) -> String;
    pub fn render_turn(records: &[MemoryRecord]) -> String;
}
```

输出格式：
```
Bootstrap memory:
- [preference / high] User prefers architecture-first discussion
- [experience / medium] Previous migration caused downtime

Relevant memory:
- [fact / high] Schema uses UUID primary keys
```

---

## 8. 内置 MCP 服务器

Joi 内置两个 **stdio-based MCP 服务器**（协议版本 2024-11-05），在 Agent 启动时作为子进程注册。Agent 可通过这些服务器访问平台能力。

### 8.1 Conversation MCP（agentx-conversation）

启动方式：
```bash
joi --agentx-conversation-mcp --channel-id <channel-id> --source-agent <agent-name>
```

说明：
- 对支持 `session/new.mcpServers` 的 ACP runtime，Joi 会直接注入 `agentx-conversation`
- 对 `cursor_login` 这类 MCP 注入不稳定的 runtime，Joi 仍会写 workspace 级 `.cursor/mcp.json`，但共享工件能力不再依赖它；agent 可退回到本地 artifact CLI

提供 11 个工具：

| 工具名 | 说明 | 必需参数 |
|--------|------|---------|
| `conversation_list_recent` | 列出频道或线程的最近消息 | scope ("main"/"thread") |
| `conversation_search` | 按关键词搜索消息 | query |
| `conversation_read_window` | 读取锚点消息周围的消息 | message_id |
| `agent_handoff` | 将任务委派给另一个 Agent | target_agent, message |
| `handoff_list` | 列出频道中的 handoff 记录 | (无) |
| `handoff_get` | 按 handoff id 读取交接记录 | handoff_id |
| `conversation_list_threads` | 列出频道中的线程 | (无) |
| `artifact_publish` | 显式发布文件、目录或文本到频道共享工件区 | source_path 或 text |
| `artifact_list` | 列出最近发布的共享工件 | (无) |
| `artifact_get` | 按 artifact id 或 URI 读取工件 manifest | artifact_id 或 artifact_uri |
| `artifact_read` | 读取共享工件中的文本内容 | artifact_id 或 artifact_uri |

**agent_handoff 详细流程：**

```
Agent-A 调用 agent_handoff(target: "codex", message: "请检查迁移")
    │
    ▼
MCP Server 验证
    ├─ source_agent != target_agent（不能交接给自己）
    ├─ target_agent 存在且 enabled
    └─ thread_id 属于当前 channel（如果提供）
    │
    ▼
db.create_agent_handoff() → 状态 = "pending"
    │
    ▼
如果在线程中：写入 handoff.md（ThreadContextService）
    │
    ▼
返回 { handoff_id, status: "pending" }
```

**handoff CLI 后备通路（尤其用于 MCP 工具不可见的 runtime）：**

```bash
"$AGENTX_RUNTIME_BIN" --agentx-handoff-create \
  --target-agent codex \
  --message "Please inspect the migration"
```

- 如果当前提示是线程上下文，且 prompt 里给出了 `Thread ID: ...`，可继续传 `--thread-id <thread-id>`
- `handoff_list` / `handoff_get` 在 MCP 中可用；CLI 也提供 `--agentx-handoff-list` / `--agentx-handoff-get`
- CLI 创建的 handoff 仍然写入同一张 `agent_handoffs` 表，并复用同一套线程上下文写入逻辑

**artifact_publish 详细流程：**

```
Agent-A 调用 artifact_publish(source_path: "patch.diff")
    │
    ▼
MCP Server 验证 source_path 位于 Agent 私有 workspace
    │
    ▼
复制到 ~/.agentx/channels/<channel-id>/shared/artifacts/<artifact-id>/
    │
    ▼
写入 manifest.json，生成 artifact://<channel-id>/<artifact-id>
    │
    ▼
返回 artifact manifest + 可直接贴到回复里的 markdown link
```

**artifact CLI 后备通路（尤其用于 Cursor ACP）：**

```bash
"$AGENTX_RUNTIME_BIN" --agentx-artifact-publish \
  --source-path report.md \
  --title "Shared report" \
  --output markdown
```

- `AGENTX_CHANNEL_ID` / `AGENTX_AGENT_NAME` / `AGENTX_AGENT_WORKSPACE` / `AGENTX_CHANNEL_SHARED` / `AGENTX_CHANNEL_SHARED_ARTIFACTS` 会在 agent runtime 中自动注入
- CLI 输出可直接贴到回复里；`--output markdown` 会直接打印 `[title](artifact://...)`

### 8.2 Memory MCP（agentx-memory）

启动方式：
```bash
joi --agentx-memory-mcp --agent-id <agent-id>
```

提供 3 个工具：

| 工具名 | 说明 | 必需参数 |
|--------|------|---------|
| `memory_search` | 搜索 Agent 的记忆记录 | (query 可选) |
| `memory_get` | 按 ID 获取单条记忆 | id |
| `memory_list_recent` | 列出最近的已接受记忆 | (limit 可选, 默认 8) |

### 8.3 CLI 模式分发

Joi 的 `main.rs` 在启动时先检查 CLI 参数：

```rust
fn main() {
    match maybe_run_cli_mode() {
        Ok(true) => return,          // 以 MCP 服务器模式运行
        Ok(false) => {},             // 继续启动 Tauri GUI
        Err(err) => { eprintln!("{}", err); std::process::exit(1); }
    }
    run();  // 启动 Tauri 桌面应用
}
```

`maybe_run_cli_mode()` 依次尝试：
1. `mcp::conversation::maybe_run_from_args()` → 检测 `--agentx-conversation-mcp`
2. `mcp::memory::maybe_run_from_args()` → 检测 `--agentx-memory-mcp`

---

## 9. 线程上下文服务（ThreadContext）

`ThreadContextService` 为线程中的对话维护结构化上下文快照。

### 9.1 ThreadContextSnapshot

```rust
pub struct ThreadContextSnapshot {
    pub summary: String,                   // 线程摘要
    pub decisions: Vec<ThreadDecision>,    // 已做出的决策
    pub open_loops: Vec<ThreadOpenLoop>,   // 待解决的开放问题
    pub handoff: Option<String>,           // 最近的交接说明
}
```

### 9.2 文件存储结构

```
~/.agentx/channels/<channel-id>/threads/<thread-id>/context/
├── summary.md          # 自动生成的线程摘要
├── decisions.yaml      # 决策列表（ThreadDecisionList）
├── open_loops.yaml     # 开放问题列表（ThreadOpenLoopList）
└── handoff.md          # 最近的 Agent 交接说明
```

### 9.3 核心方法

| 方法 | 说明 |
|------|------|
| `refresh_summary(db, channel_id, thread_id)` | 从数据库读取根消息和最近回复，生成 summary.md |
| `write_handoff_note(...)` | 写入 Agent 交接说明 |
| `load(db, channel_id, thread_id)` | 加载完整 ThreadContextSnapshot |
| `render(snapshot)` | 将快照渲染为纯文本，用于注入 Agent 提示 |

**Summary 自动生成格式：**
```
Thread: Schema drift
Root: alice: Investigate schema drift
Recent replies:
- architect: Looking into the migration history
- reviewer: Found three inconsistencies
```

---

## 10. 信息泳道：消息的接收策略

### 10.1 信息获取的两种触发方式

```
触发方式一：显式 @mention
用户在消息中写 "@agent-name ..."
         │
         ▼
router.parse_mentions(content)  →  ["agent-name"]
         │
         ▼
agent_mgr.enqueue_message(agent-name, ...)

触发方式二：线程自动回复
用户在某 Agent 已参与的线程中发消息（不含 @mention）
         │
         ▼
router.resolve_agent_targets(content, reply_target=Some("agent-name"), ...)
         │
         ▼
reply_target 自动作为路由目标
```

### 10.2 上下文窗口控制

Agent 规格中的 `context.window` 参数（默认 50）控制历史消息数量上限。数据库查询时 `LIMIT window ORDER BY created_at DESC`，取最近 N 条消息后反序排列。

### 10.3 消息排队：Agent 忙时的接收策略

```
新消息到达
    │
    ▼
agent_mgr.enqueue_message()
    │
    ├─ Agent 不在 running 中 → 创建 StartingAgent，异步启动
    │                          排入 StartingAgent.pending_messages
    │
    ├─ RunningAgent::Starting → 排入 pending_messages 队列
    │
    └─ RunningAgent::Acp(ActiveAgent)
            ├─ adapter.status == Idle → dispatch_next_message()（立即处理）
            └─ adapter.status == Active → 排入 pending_messages 队列
```

### 10.4 push_mode：Agent 的信息推送策略

| push_mode | 行为 |
|-----------|------|
| `mention_only`（默认） | 只有被 @mention 时才收到消息 |
| `all` | 频道中所有消息都会推送给该 Agent |

`all` 模式适用于需要监控整个频道对话的 Agent（如日志分析、自动回复机器人）。

---

## 11. 信息泳道：消息的发送策略

### 11.1 Agent 输出的实时流式写入

```
ACP stdout reader（后台线程）
    │  读取每一行 JSON-RPC 输出
    │
    ▼
parse_agent_event(line) → AgentEvent
    │
    ▼  通过 mpsc channel 发送
event_sender.send(event)
    │
    ▼  Event Forwarder 异步任务接收
spawn_event_forwarder(agent_name, channel_id, ...)
    │
    ├─ AgentEvent::Text { content, is_partial }
    │      │
    │      ▼
    │  sync_turn_message()
    │      ├─ 若本轮无消息：db.create_message(sender_type='agent')
    │      │                 emit('message:created')
    │      └─ 若本轮有消息：append content to existing
    │                        db.update_message(content)
    │                        emit('message:updated')
    │
    ├─ AgentEvent::ToolUse { tool_name, input }
    │      │
    │      ▼
    │  upsert_tool_trace_item()
    │      └─ 更新 metadata.tool_trace.items
    │         emit('message:updated')
    │
    ├─ AgentEvent::ActionRequest { id, type, title, ... }
    │      │
    │      ▼
    │  upsert_action_trace_item()
    │      ├─ 更新 metadata.action_trace.items (status='pending')
    │      ├─ db.save_action_request()
    │      └─ emit('message:updated')
    │
    ├─ AgentEvent::StatusChange { status }
    │      │
    │      ▼
    │  db.update_session_status(status)
    │  emit('agent:status', {agent_name, status})
    │
    └─ AgentEvent::Finished { success, summary }
           │
           ▼
       handle_detected_mentions()
       emit('agent:finished', {agent_name, success})
       finish_current_prompt()
       dispatch_next_message()     [处理队列中的下一条消息]
```

### 11.2 Agent-to-Agent 协作的两种方式

**方式一：文本中 @mention（handoff_mode = "manual"）**
```
Agent-A 的输出文本包含 "@agent-b 请帮我处理这部分"
    │
    ▼
AgentEvent::Finished 处理时
    ▼
handle_detected_mentions(content)
    ▼
router.parse_mentions(agent_output_content) → ["agent-b"]
    ▼
emit('agent:mentions_detected', {mentions: ["agent-b"]})
    ▼
前端通知用户，用户可触发 enqueue_message("agent-b", ...)
```

**方式二：MCP agent_handoff 工具（handoff_mode = "auto"）**
```
Agent-A 调用 MCP 工具:
    agent_handoff(target_agent: "agent-b", message: "请检查这部分")
    │
    ▼
db.create_agent_handoff() → 记录交接请求
    ▼
ThreadContextService.write_handoff_note() → 写入上下文
    ▼
Agent-B 下次被触发时，可通过线程上下文看到交接说明
```

**方式三：handoff CLI 后备通路（MCP 不可见时）**
```
Agent-A 在 shell / skill 中执行:
    "$AGENTX_RUNTIME_BIN" --agentx-handoff-create --target-agent agent-b --message "请检查这部分"
    │
    ▼
db.create_agent_handoff() → 记录交接请求
    ▼
ThreadContextService.write_handoff_note() → 写入上下文
    ▼
Joi app runtime 后续处理 pending handoff 并将消息排队给 Agent-B
```

**方式四：MCP artifact_publish 工具（显式共享工作产物）**
```
Agent-A 在私有 workspace 中产出 patch / report / 目录
    │
    ▼
artifact_publish(...)
    │
    ▼
Joi 复制到 channel shared/artifacts/
    │
    ▼
Agent-A 在回复里贴出 artifact://... 链接
    │
    ▼
人类或其他 Agent 通过 artifact_get / artifact_read 或桌面端点击链接访问
```

**方式五：artifact CLI 后备通路（无 MCP 注入时）**
```
Agent-A 在私有 workspace 中产出 patch / report / 目录
    │
    ▼
shell 中执行 "$AGENTX_RUNTIME_BIN" --agentx-artifact-publish --source-path <path> --output markdown
    │
    ▼
Joi 复制到 channel shared/artifacts/
    │
    ▼
Agent-A 在回复里贴出 artifact://... 链接
```

### 11.3 消息的线程归属策略

Agent 发送的消息严格遵循当前提示的线程上下文：用户在哪个上下文问的，Agent 就在哪个上下文回答。

### 11.4 turn_id 关联机制

同一轮 Agent 任务（从收到提示到 Finished）产生的所有消息共享同一个 `turn_id`，使前端可以将文本 + 工具追踪 + 权限追踪聚合展示在同一个消息气泡中。

### 11.5 Agent 状态机

```
         start_agent()
Stopped ──────────────► Starting
                            │
                            │ complete_async_start()
                            ▼
                         Active/Idle ◄────────────────────┐
                            │                             │
                            │ dispatch_next_message()      │
                            ▼                             │
                         Active/Busy                      │
                            │                             │
                            │ AgentEvent::Finished         │
                            └─────────────────────────────┘
                            │
                            │ stop_agent()
                            ▼
                         Stopped
```

**关键约束**：一个 Agent 实例在同一时刻只处理一条消息（串行处理），保证上下文的连续性。

---

## 12. @Mention 路由机制

### 12.1 消息路由全流程

```
用户发送: "@claude 帮我看看这段代码，然后 @codex 补充一下测试"

     │
     ▼
send_message Tauri command
     │
     ├─ 1. 写入数据库（sender_type='user'）
     ├─ 2. emit('message:created') → 前端立即显示
     ├─ 3. registry.list_enabled_agents()
     ├─ 4. resolve_agent_targets(content, reply_target=None, enabled_agents)
     │       │
     │       ▼
     │    parse_mentions("@claude 帮我... @codex ...") → ["claude", "codex"]
     │       │
     │       ▼
     │    过滤出在 enabled_agents 中的 → ["claude", "codex"]
     │
     ├─ 5. agent_mgr.enqueue_message("claude", msg)
     └─ 6. agent_mgr.enqueue_message("codex", msg)
              │
              ▼
         两个 Agent 并行处理同一条消息
```

### 12.2 路由优先级

```rust
pub fn resolve_agent_targets(
    content: &str,
    reply_target_agent: Option<&str>,
    enabled_agents: &[String],
) -> Vec<String> {
    let mentions = parse_mentions(content);

    if !mentions.is_empty() {
        // 1. 优先使用显式 @mention
        mentions.into_iter()
            .filter(|m| enabled_agents.contains(m))
            .collect()
    } else if let Some(target) = reply_target_agent {
        // 2. 线程回复：自动路由到线程所属 Agent
        if enabled_agents.contains(&target.to_string()) {
            vec![target.to_string()]
        } else {
            vec![]
        }
    } else {
        // 3. 无 Agent 被触发
        vec![]
    }
}
```

### 12.3 前端 @mention 输入体验

```
用户输入 "@cl"
    │
    ▼
useMention.handleInputChange(value, cursorPos)
    │
    ▼  正则匹配 /@([\w-]*)$/ 在光标前的文本
    │  match[1] = "cl"
    │
    ▼
setMentionQuery("cl") → showMention = true
    │
    ▼
filteredAgents = agents.filter(a =>
    a.enabled && a.name.includes("cl")
)  → ["claude"]
    │
    ▼
MentionPopup 显示候选列表
    │
用户按 Enter/Tab 选择
    │
    ▼
selectMention(value, cursorPos, "claude")
    │  将 "@cl" 替换为 "@claude "
    ▼
MessageInput 更新文本内容
```

---

## 13. 消息队列与并发控制

### 13.1 全局并发模型

```rust
pub struct AgentManager {
    db: Arc<Database>,
    registry: Arc<AgentRegistry>,
    thread_context: ThreadContextService,
    running: Mutex<HashMap<String, RunningAgent>>,  // key: "agent_name:channel_id"
    app_handle: Mutex<Option<AppHandle>>,
}

enum RunningAgent {
    Starting(StartingAgent {
        pending_messages: VecDeque<QueuedPrompt>,
    }),
    Acp(ActiveAgent {
        adapter: AcpAdapter,
        pending_messages: VecDeque<QueuedPrompt>,
        current_prompt: Option<QueuedPrompt>,
    }),
}
```

多个 Agent 实例之间完全**并行运行**（不同 HashMap key）。
同一 Agent 实例内部**串行处理**（current_prompt 完成后才取下一条）。

### 13.2 complete_async_start 的竞态处理

启动 Agent 是异步操作，期间可能有更多消息入队：

```rust
async fn complete_async_start(...) {
    // 1. 启动子进程（耗时操作）
    let adapter = AcpAdapter::start(&config).await?;

    // 2. 重新获取锁，将 Starting → Acp
    let mut running = manager.running.lock();

    // 3. 取出 Starting 时积累的所有消息
    let pending = match running.remove(&key) {
        Some(RunningAgent::Starting(s)) => s.pending_messages,
        _ => VecDeque::new(),
    };

    // 4. 迁移到 ActiveAgent，继承积压消息
    running.insert(key, RunningAgent::Acp(ActiveAgent {
        adapter,
        pending_messages: pending,
        current_prompt: None,
    }));

    // 5. 开始处理队列
    manager.dispatch_next_message(&key);
}
```

---

## 14. 权限请求（Action）机制

### 14.1 权限请求流程

```
Agent 执行工具时需要用户授权
    │
    ▼
ACP: session/request_permission
    {
      "method": "session/request_permission",
      "params": {
        "toolCall": { "title": "bash", "toolCallId": "...", ... },
        "options": [
          {"optionId": "opt_approve", "name": "Approve"},
          {"optionId": "opt_deny", "name": "Deny"}
        ]
      }
    }
    │
    ▼  Event Forwarder 处理
AgentEvent::ActionRequest
    │
    ▼
1. db.save_action_request(status='pending')
2. upsert_action_trace_item(status='pending')
3. db.update_message_metadata()
4. emit('message:updated')
    │
    ▼  [前端]
ActionCard 显示（警告样式 + 选项按钮）
    │
用户点击 "Approve"
    │
    ▼
invoke('respond_action', {
    agentName, channelId,
    actionId: "action-uuid",
    optionId: "opt_approve",
    optionLabel: "Approve"
})
    │
    ▼  [Rust]
respond_action command
    │
    ├─ adapter.respond_permission("action-uuid", "opt_approve")
    │       └─ write JSON-RPC response to stdin:
    │          {
    │            "jsonrpc": "2.0",
    │            "id": N,
    │            "result": {
    │              "outcome": { "outcome": "selected", "optionId": "opt_approve" }
    │            }
    │          }
    └─ record_action_response()
            ├─ db.update_action_request(status='approved')
            ├─ patch action_trace item status → 'approved'
            └─ emit('message:updated')
    │
    ▼  Agent 收到响应，继续执行
```

### 14.2 前端乐观更新

ActionCard 采用乐观更新策略：

```typescript
const handleRespond = async (optionId: string, label: string) => {
    setOptimisticSelection({ optionId, label }); // 立即更新 UI
    try {
        await invoke('respond_action', { agentName, channelId, actionId, optionId, optionLabel: label });
    } catch (err) {
        setOptimisticSelection(null); // 失败时回滚
    }
};
```

### 14.3 权限状态流转

```
pending → approved (用户批准)
        → denied   (用户拒绝)
        → expired  (超时或 Agent 取消)
```

### 14.4 Agent Handoff 状态流转

```
pending → dispatching (已被 claim)
        → queued      (已入队给目标 Agent)
        → failed      (失败)
```

---

## 15. 前端事件驱动架构

### 15.1 Tauri 事件总线

```
Rust emit()                      前端 listen()
─────────────────────────────────────────────────────
message:created   →  messageStore.addMessage()
                     channelStore.incrementUnread()

message:updated   →  messageStore.updateMessage()
                     (内容、metadata、工具追踪、权限追踪)

agent:status      →  agentStore.setAgentStatus()

agent:finished    →  agentStore.setAgentStatus('idle')

agent:error       →  notificationStore.pushNotification()
                     agentStore.setAgentStatus('error')

agent:mentions_detected  →  notificationStore.pushNotification()
                            (Agent 提及了其他 Agent)

marketplace:install_status → 安装进度通知
```

### 15.2 Tauri IPC Commands 完整列表

**消息相关：**
```typescript
invoke('send_message', { channelId, threadId?, content, attachments?, senderName }) → Message
invoke('get_messages', { channelId, threadId?, limit?, beforeId? }) → Message[]
invoke('read_attachment_preview', { path, maxBytes? }) → string
invoke('create_thread', { channelId, rootMessageId, title }) → Thread
invoke('get_threads', { channelId }) → Thread[]
```

**频道相关：**
```typescript
invoke('create_channel', { name, description }) → Channel
invoke('list_channels') → Channel[]
invoke('delete_channel', { id }) → void
invoke('update_channel', { id, name?, description? }) → Channel
```

**Agent 相关：**
```typescript
invoke('list_agents') → AgentSummary[]
invoke('get_agent', { id }) → AgentDocument
invoke('save_agent', { agent: AgentDocument }) → AgentDocument
invoke('delete_agent', { id }) → void
invoke('list_marketplace_agents') → MarketplaceAgentSummary[]
invoke('install_marketplace_agent', { request }) → void
invoke('list_agent_memory', { agentId, query? }) → MemoryRecord[]
invoke('append_agent_memory', { input }) → MemoryRecord
invoke('start_agent', { name, channelId, threadId? }) → AgentSession
invoke('stop_agent', { name, channelId, threadId? }) → void
invoke('respond_action', { agentName, channelId, actionId, optionId, optionLabel }) → void
invoke('get_agent_status', { name, channelId }) → string
```

**设置相关：**
```typescript
invoke('get_proxy_settings') → ProxySettings
invoke('save_proxy_settings', { settings }) → void
```

### 15.3 Zustand Store 结构

```typescript
// messageStore
interface MessageStore {
    messages: Message[];
    threads: Thread[];
    threadMessages: Record<string, Message[]>;
    activeThreadId: string | null;

    fetchMessages(channelId, threadId?): Promise<void>;
    sendMessage(channelId, content, threadId?, attachments?): Promise<Message>;
    addMessage(msg): void;
    updateMessage(id, patch): void;
    createThread(channelId, rootMessageId, title): Promise<Thread>;
    ensureThread(channelId, messageId, title): Promise<Thread>;
    setActiveThread(id): void;
}

// agentStore
interface AgentStore {
    agents: AgentSummary[];
    marketplaceAgents: MarketplaceAgentSummary[];
    agentStatuses: Record<string, string>;

    fetchAgents(): Promise<void>;
    fetchAgent(id): Promise<AgentDocument>;
    saveAgent(agent): Promise<AgentDocument>;
    deleteAgent(id): Promise<void>;
    fetchMarketplaceAgents(): Promise<MarketplaceAgentSummary[]>;
    installMarketplaceAgent(request): Promise<void>;
    setAgentStatus(id, status): void;
}

// channelStore
interface ChannelStore {
    channels: Channel[];
    activeChannelId: string | null;
    unreadCounts: Record<string, number>;

    fetchChannels(): Promise<void>;
    createChannel(name, description): Promise<Channel>;
    deleteChannel(id): Promise<void>;
    setActiveChannel(id): void;
    markChannelUnread(id, count?): void;
    incrementChannelUnread(id): void;
    clearChannelUnread(id): void;
}

// settingsStore
interface SettingsStore {
    proxySettings: ProxySettings;
    showProxySettings: boolean;

    fetchProxySettings(): Promise<void>;
    saveProxySettings(settings): Promise<void>;
}
```

---

## 16. 完整数据流图

### 16.1 用户消息 → Agent 处理 → 前端更新

```
┌──────────────────────────────────────────────────────────────────┐
│                          用户操作                                 │
│  MessageInput.handleSend()                                       │
│  invoke('send_message', {channelId, content, threadId, ...})     │
└────────────────────────────────┬─────────────────────────────────┘
                                 │
                ┌────────────────▼────────────────┐
                │        Rust: send_message        │
                │  1. db.create_message(user)      │
                │  2. emit('message:created') ──────────► 前端显示消息
                │  3. registry.list_enabled_agents()│
                │  4. resolve_agent_targets()       │
                │  5. enqueue_message(agent_a)      │
                │  6. enqueue_message(agent_b)      │
                └────────┬──────────────┬──────────┘
                         │              │
          ┌──────────────▼──┐      ┌────▼─────────────┐
          │  Agent A 处理   │      │  Agent B 处理    │
          │  AcpAdapter     │      │  AcpAdapter      │
          │  .send_prompt() │      │  .send_prompt()  │
          └──────┬──────────┘      └────────┬─────────┘
                 │                          │
          ┌──────▼──────────────────────────▼─────────┐
          │          Event Forwarder (async)           │
          │                                            │
          │  Text    → sync_turn_message()             │
          │  ToolUse → upsert_tool_trace()             │
          │  Action  → upsert_action_trace()           │
          │  Status  → emit('agent:status')            │
          │  Finish  → handle_mentions()               │
          │           → dispatch_next_message()        │
          │                                            │
          │  所有变更 → emit('message:created/updated') │
          └────────────────────────────────────────────┘
                            │
                            ▼
                    前端实时更新 UI
```

### 16.2 Agent 记忆系统数据流

```
┌──────────────────────────────────────────────────────────────┐
│  Agent 启动时                                                 │
│                                                              │
│  MemorySelector.select_bootstrap(store)                      │
│      ↓                                                       │
│  [最近 8 条高置信度记忆]                                       │
│      ↓                                                       │
│  MemoryRenderer.render_bootstrap()                           │
│      ↓                                                       │
│  注入 Bootstrap 上下文                                        │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│  每轮对话时                                                   │
│                                                              │
│  MemorySelector.select_for_turn(store, message, context)     │
│      ↓                                                       │
│  提取关键词 → 查询匹配记忆 → 返回最多 4 条                      │
│      ↓                                                       │
│  MemoryRenderer.render_turn()                                │
│      ↓                                                       │
│  注入 Turn 上下文                                             │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│  Agent 运行中（通过 MCP）                                      │
│                                                              │
│  Agent → memory_search(query) → JsonlMemoryStore.query()     │
│  Agent → memory_get(id) → JsonlMemoryStore.get()             │
│  Agent → memory_list_recent(limit) → JsonlMemoryStore.list() │
└──────────────────────────────────────────────────────────────┘
```

### 16.3 Marketplace 安装流程

```
用户浏览 Marketplace
    │
    ▼
invoke('list_marketplace_agents')
    │
    ▼
AgentRegistry.list_marketplace_agents_with_proxy()
    ├─ GET https://cdn.agentclientprotocol.com/.../registry.json
    │  (失败时读本地缓存)
    └─ 返回 MarketplaceAgentSummary[]
    │
    ▼
用户选择 Agent，点击安装
    │
    ▼
invoke('install_marketplace_agent', { marketplaceId, localAgentId?, displayName? })
    │
    ▼
AgentRegistry.install_marketplace_agent_with_proxy()
    ├─ 查找 manifest
    ├─ 解析 distribution → TransportSpec
    │      ├─ npx → StdioTransportSpec { command: "npx" }
    │      ├─ uvx → StdioTransportSpec { command: "uvx" }
    │      └─ binary → 下载 → 解压 → 符号链接
    ├─ 加载 PlatformDefaults
    ├─ 生成 identity.md, soul.md, tools.yaml
    └─ save_agent_document() → 写入 agents/<id>/
    │
    ▼
Agent 可用，出现在 AgentList 中
```

---

## 17. 数据目录结构

```
~/.agentx/                          # AGENTHUB_HOME 环境变量可覆盖
├── data.db                         # SQLite 数据库（WAL 模式）
├── config/
│   └── defaults.yaml               # PlatformDefaults（全局默认配置）
├── agents/
│   └── <agent-id>/
│       ├── spec.yaml               # Agent 规格配置
│       ├── identity.md             # Agent 身份描述
│       ├── soul.md                 # Agent 行为风格
│       ├── tools.yaml              # 工具策略
│       └── memory/
│           ├── meta.json           # 记忆存储元信息
│           └── records/
│               ├── 2026-01.jsonl   # 按月分片的记忆文件
│               └── 2026-03.jsonl
├── channels/
│   └── <channel-id>/
│       ├── attachments/
│       │   └── <message-id>/
│       │       └── <filename>      # 用户上传的附件
│       ├── shared/
│       │   └── artifacts/          # 显式共享的频道级工件
│       ├── agents/
│       │   └── <agent-id>/
│       │       ├── workspace/      # Agent 私有工作区（默认 cwd）
│       │       ├── cache/
│       │       └── logs/
│       ├── threads/
│       │   └── <thread-id>/
│       │       └── context/
│       │           ├── summary.md
│       │           ├── decisions.yaml
│       │           ├── open_loops.yaml
│       │           └── handoff.md
│       └── workspace/              # 兼容目录；不再作为所有 Agent 的默认共享 cwd
├── registries/
│   └── official-acp/
│       └── registry.json           # Marketplace 注册表缓存
└── runtimes/
    ├── acp/
    │   └── <runtime-id>/
    │       └── <version>/
    │           ├── runtime-archive.tar.gz
    │           └── payload/        # 解压后的二进制
    └── bin/
        └── <runtime-id>            # 符号链接到已安装的二进制
```

---

## 18. 构建与部署

### 18.1 开发

```bash
pnpm install          # 安装前端依赖
pnpm tauri dev        # 启动开发模式（前端热重载 + Rust 编译）
```

### 18.2 构建 macOS Bundle

```bash
pnpm build:macos
# 等效于: pnpm tauri build --bundles app,dmg --ci
```

产物：
- `src-tauri/target/release/bundle/macos/Joi.app`
- `src-tauri/target/release/bundle/dmg/Joi_0.1.0_*.dmg`

### 18.3 Tauri 配置

```json
{
  "productName": "Joi",
  "version": "0.1.0",
  "identifier": "com.agentx.joi",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "beforeBuildCommand": "pnpm build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [{
      "title": "Joi",
      "width": 1280,
      "height": 800,
      "minWidth": 960,
      "minHeight": 600
    }],
    "security": { "csp": null }
  }
}
```

### 18.4 Rust 依赖清单

| 依赖 | 版本 | 用途 |
|------|------|------|
| tauri | 2.x | 桌面应用框架（含 tray-icon） |
| tauri-plugin-opener | 2.x | 系统默认应用打开文件 |
| tauri-plugin-shell | 2.x | 子进程管理 |
| rusqlite | 0.31 | SQLite（bundled 模式） |
| tokio | 1.x | 异步运行时（full features） |
| serde / serde_json | 1.x | JSON 序列化 |
| serde_yaml | 0.9 | YAML 序列化（Agent 配置） |
| uuid | 1.x | UUID v4 生成 |
| regex | 1.x | @mention 解析 |
| chrono | 0.4 | 时间处理 |
| reqwest | 0.12 | HTTP 客户端（Marketplace 下载） |
| base64 | 0.22 | 附件 Base64 编码 |
| infer | 0.19 | 文件 MIME 类型推断 |
| strip-ansi-escapes | 0.2 | 清理 ANSI 转义序列 |
| async-trait | 0.1 | 异步 trait 支持 |
| log / env_logger | 0.4 / 0.11 | 日志 |

### 18.5 前端依赖清单

| 依赖 | 版本 | 用途 |
|------|------|------|
| react / react-dom | 19.1 | UI 框架 |
| zustand | 5.x | 状态管理 |
| @tauri-apps/api | 2.x | Tauri IPC 调用 |
| @tauri-apps/plugin-opener | 2.x | 打开文件/URL |
| @tanstack/react-virtual | 3.x | 虚拟滚动列表 |
| react-markdown | 10.x | Markdown 渲染 |
| remark-gfm | 4.x | GitHub Flavored Markdown |
| react-syntax-highlighter | 16.x | 代码语法高亮 |
| @tailwindcss/typography | 0.5 | Markdown 排版样式 |
| tailwindcss | 4.2 | CSS 工具类 |
| vite | 7.x | 构建工具 |
| typescript | 5.8 | 类型检查 |
