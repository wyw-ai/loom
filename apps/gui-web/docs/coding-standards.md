# Loom GUI Web — 前端代码规范

> 基于 P1–P7 React 工程化拆分实践提炼。本文档是 OPS 知识库归档的源文件。

---

## 1. 组件拆分原则

### 1.1 何时拆分

| 条件 | 说明 |
|------|------|
| 单文件超过 **300 行** | 强制拆分。目标是 200 行以内，最大不超过 500 行。 |
| 组件内嵌套 ≥3 层函数组件 | 拆出独立子组件。 |
| 函数体超过 **50 行** | 拆出子函数或子组件。 |
| 同一层级有 ≥3 个相关子组件 | 拆入独立文件（如 `MachineComponents.tsx` 含 5 个机器相关组件）。 |
| 跨视图复用的 UI 片段 | 提升到 `shared/` 或 `ui/`。 |

### 1.2 拆分粒度

```
粗粒度（视图层）         细粒度（UI 层）
  SettingsView.tsx         Badge, Input, Button
       │                       ▲
       ▼                       │
  SettingsSection.tsx      复用的通用组件
       │
       ▼
  AgentComponents.tsx  ← 按领域聚合的子树组件
```

- **视图文件**（`views/`）：仅含顶层页面编排逻辑，组合子组件。
- **领域组件文件**（`components/<domain>/`）：按业务域聚合 5–10 个紧密相关的子组件。
- **UI 组件**（`components/ui/`）：纯展示、零业务逻辑，可跨项目复用。
- **共享组件**（`components/shared/`）：跨领域复用的布局或小部件。

### 1.3 命名规范

| 对象 | 规范 | 示例 |
|------|------|------|
| 组件文件 | PascalCase，领域名取单数 | `MessageRow.tsx`, `ThreadPanel.tsx`, `ChannelPanels.tsx` |
| 组件函数 | `export function ComponentName`（非 default export） | `export function MessageFeed({...})` |
| 子组件文件 | 复数形式，聚合文件用 `Components` 后缀 | `AgentComponents.tsx`, `MachineComponents.tsx` |
| 目录名 | 单数 kebab-case 或 domain 名 | `chat/`, `settings/`, `layout/`, `shared/` |
| Props 接口 | 内联在函数签名中，不单独导出 | 见 5.2 节 |

**禁止：**
- ❌ `export default function` — 统一使用命名导出，便于 IDE 自动导入和重构。
- ❌ `index.ts` 重导出桶 — 维护负担大，直接按路径导入。

---

## 2. 状态管理规范

### 2.1 决策树

```
这个状态是否只在一个组件内使用？
  ├─ YES → useState / useRef
  └─ NO  → 是否跨两个组件共享？
             ├─ YES → 通过 props 下传（优先）
             └─ NO  → 跨 ≥3 个组件或跨路由？
                        ├─ YES → Zustand store
                        └─ NO  → 提升到共同祖先，props 下传
```

### 2.2 Zustand Store 使用规则

1. **一个 store 文件一种领域**：6 个 store 分别管理 `actor` / `channel` / `connection` / `message` / `task` / `ui`。
2. **单向更新**：所有 mutation 通过 store 的 setter 函数，禁止在组件内直接修改 store 状态。
3. **setter 支持函数式更新**：`setMessages((prev) => sortMessages(upsertMessage(prev, msg)))`。
4. **store 文件内可以包含纯工具函数**：如 `capMessages()`、`lruRecord()` 放在 store 文件内（仅该 store 使用）或 `lib/`（跨 store 复用）。
5. **禁止在 store 中发起副作用**：所有 IPC 调用、fetch 留在 App.tsx 或 hook 中。

### 2.3 当前 Store 清单

| Store | 文件 | 管理状态 |
|-------|------|----------|
| `actorStore` | `store/actorStore.ts` | 当前用户 actor、所有 actor 缓存 |
| `channelStore` | `store/channelStore.ts` | 频道列表、当前频道、DM 列表 |
| `connectionStore` | `store/connectionStore.ts` | 连接状态、空间信息 |
| `messageStore` | `store/messageStore.ts` | 消息数组、threadStats、草稿 |
| `taskStore` | `store/taskStore.ts` | 任务列表、当前任务 |
| `uiStore` | `store/uiStore.ts` | 侧边栏、面板、弹窗开关 |

---

## 3. 目录结构约定

```
apps/gui-web/src/
├── App.tsx                  # 顶层路由编排 + 流订阅（唯一有副作用的根组件）
├── components/
│   ├── ui/                  # 通用 UI 原子组件（Badge, Input, Button...）
│   ├── shared/              # 跨领域复用组件（EmptyState, MutedLine...）
│   ├── layout/              # 布局层（Rail, Sidebar, ChatHeader...）
│   ├── chat/                # 聊天 & 消息（MessageFeed, ThreadPanel...）
│   ├── agent/               # Agent 展示（ActorAvatar, AgentProviderIcon...）
│   ├── channel/             # 频道操作（ChannelDeleteConfirm）
│   ├── panels/              # 右侧面板（ChannelPanels）
│   ├── settings/            # 设置页子组件
│   └── views/               # 视图层页面组件
├── store/                   # Zustand stores（每个 store 一种领域）
├── lib/                     # 纯函数工具库（零依赖或仅依赖 types）
├── hooks/                   # 自定义 React hooks
├── ipc/                     # IPC 桥接层 & 类型定义
│   ├── bridge.ts
│   └── types.ts
└── styles/                  # 全局样式（Tailwind 优先，此目录最小化）
```

### 各目录职责边界

| 目录 | 允许 | 禁止 |
|------|------|------|
| `ui/` | DOM 渲染、样式、无障碍属性 | 业务逻辑、IPC 调用、store 引用 |
| `shared/` | 跨多视图复用的纯展示组件 | 依赖特定 store 或 view |
| `layout/` | 布局骨架（Rail/Sidebar/Header） | 消息内容渲染 |
| `chat/` | 消息列表、输入框、Markdown、reaction | 频道管理、设置 |
| `views/` | 页面编排，组合子组件 | 复杂子组件逻辑（应下沉到领域文件） |
| `store/` | 状态定义 + setter | 副作用（IPC/fetch） |
| `lib/` | 纯函数、常量、类型、工具 | React hooks、JSX、store 引用 |
| `hooks/` | 自定义 React hook | 业务逻辑不应堆积在单个巨型 hook 中 |

---

## 4. 导入顺序规范

**必须按以下顺序分组**，组间空一行：

```typescript
// 1. React 核心
import { useState, useMemo, Fragment } from "react";

// 2. 第三方库
import { Virtuoso } from "react-virtuoso";
import { X, Split } from "lucide-react";

// 3. IPC 层（底层基础类型）
import type { Message, Actor, Channel } from "@/ipc/types";

// 4. 内部类型
import type { ThreadActivityStats } from "@/lib/types";

// 5. lib 工具函数
import { cn } from "@/lib/utils";
import { groupMessagesByDate } from "@/lib/message-utils";

// 6. store
import { useMessageStore } from "@/store/messageStore";

// 7. hooks
import { useStickToBottomScroll } from "@/hooks/useStickToBottomScroll";

// 8. 兄弟/子组件
import { MessageRow } from "@/components/chat/MessageRow";
import { EmptyState } from "@/components/shared/EmptyState";
```

**类型导入**：使用 `import type` 关键字（TypeScript `verbatimModuleSyntax` 兼容）。

---

## 5. TypeScript 使用规范

### 5.1 类型定义位置

| 类型来源 | 定义位置 | 示例 |
|----------|----------|------|
| IPC 协议类型 | `src/ipc/types.ts` | `Message`, `Actor`, `Channel` |
| 前端专用类型 | `src/lib/types.ts` | `ThreadActivityStats`, `ActionChoice` |
| 组件 Props | 函数签名内联 | 见 5.2 |
| Store 接口 | Store 文件中 `export interface` | `MessageStore` |

### 5.2 Props 类型 — 内联定义

```typescript
// ✅ 正确：Props 内联在函数参数中
export function MessageRow({
  actor,
  message,
  canReply = false,
  onReply,
}: {
  actor: Actor | undefined;
  message: Message;
  canReply?: boolean;
  onReply: (message: Message) => void;
}) { ... }

// ❌ 错误：单独导出 Props 类型（除非被多文件复用）
export interface MessageRowProps { ... }
export function MessageRow(props: MessageRowProps) { ... }
```

### 5.3 interface vs type

- **`type`**：联合类型、交叉类型、工具类型、简单对象。
- **`interface`**：仅在需要扩展/合并声明时使用（如 Store interface）。
- **非 Store 的类型一律用 `type`**。

### 5.4 禁止规则

- ❌ `any` — 用 `unknown` 替代，或在充分校验后显式 `as` 断言。
- ❌ `as unknown as Target` 链式断言 — 表明类型设计有缺陷，应修复上游类型。
- ❌ 可选链后不加判空 — `actor?.name.length` 在 `name` 为 `undefined` 时仍可能报错。
- ❌ 关闭 strict 模式 — `tsconfig.json` 保持 `"strict": true`。

---

## 6. 内存管理规范

### 6.1 核心原则

**前端永远假设用户会长时间保持页面打开。** 所有会随时间增长的数据结构必须有明确的上限。

### 6.2 消息数组

```typescript
// ✅ 每次 set 时自动 cap
const MAX = 150;
setMessages: (messages) =>
  set((state) => ({
    messages: capMessages(
      typeof messages === "function" ? messages(state.messages) : messages,
      MAX,
    ),
  })),

// capMessages: 保留数组末尾（最新）至多 max 条
function capMessages<T>(items: T[], max: number): T[] {
  return items.length > max ? items.slice(-max) : items;
}
```

### 6.3 Record/Map 类缓存 — LRU 逐出

```typescript
// threadStatsById: max 50 entries, LRU eviction
const MAX_THREAD_STATS = 50;

setThreadStatsById: (stats) =>
  set((state) => {
    const resolved = typeof stats === "function" ? stats(state.threadStatsById) : stats;
    let order = [...state._threadStatsOrder];
    for (const key of Object.keys(resolved)) {
      // touch accessed keys
      order = order.filter((k) => k !== key);
      order.push(key);
    }
    // evict oldest if over limit
    if (order.length > MAX_THREAD_STATS) {
      const toEvict = new Set(order.slice(0, order.length - MAX_THREAD_STATS));
      const record: Record<string, ThreadActivityStats> = {};
      const newOrder: string[] = [];
      for (const k of order) {
        if (!toEvict.has(k)) { record[k] = resolved[k]; newOrder.push(k); }
      }
      return { threadStatsById: record, _threadStatsOrder: newOrder };
    }
    return { threadStatsById: resolved, _threadStatsOrder: order };
  }),
```

### 6.4 虚拟滚动

- 消息列表 (`MessageFeed`)、线程面板 (`ThreadPanel`) 使用 `react-virtuoso`。
- **何时需要虚拟滚动**：列表可能超过 **50 项** 的任意场景。
- **Virtuoso 模式**：
  - `followOutput="smooth"` — 自动跟随底部（替代手动 scroll 逻辑）。
  - Header 插槽 — 放置非虚拟化内容（如 ThreadPanel 的 root message）。
  - `components={{ EmptyPlaceholder }}` — 空状态占位。

### 6.5 检查清单

- [ ] 数组类状态是否有最大长度 cap？
- [ ] Record/Map 类缓存是否有 LRU 逐出？
- [ ] 超过 50 项的列表是否使用虚拟滚动？
- [ ] 组件卸载时是否清理了定时器/订阅？
- [ ] 流订阅是否在 scope 切换时正确销毁？

---

## 7. Git 提交规范

### 7.1 格式

```
<phase-tag>: <English summary>

<Chinese detail body>

Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

### 7.2 Phase Tag

| Tag | 含义 |
|-----|------|
| `P1:` | 常量/类型/工具提取 |
| `P2:` | 状态管理迁移 |
| `P3:` | 布局层拆分 |
| `P4:` | 聊天/线程组件 |
| `P5:` | 视图层拆分 |
| `P6:` | Settings 子组件 |
| `P7:` | 内存泄露修复 |
| `P8:` | 代码规范 & 归档 |
| `feat:` | 新功能 |
| `fix:` | Bug 修复 |
| `refactor:` | 非功能性的重构 |

### 7.3 示例

```
P7: Memory leak fixes — LRU caps + virtual scrolling

1. Message array size caps (messages≤150, threadMessages≤100, directMessages≤150)
2. threadStatsById LRU eviction (max 50 entries)
3. react-virtuoso virtual scrolling in MessageFeed & ThreadPanel

✅ tsc --noEmit: zero errors
✅ vite build: success

Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

### 7.4 规则

1. **一次 commit 只做一件事**：一个 phase 一个 commit。
2. **commit 前必须** `tsc --noEmit` 零错误 + `vite build` 通过。
3. **不提交 WIP**：所有 commit 都是可工作的完整状态。
4. **包含 `Co-authored-by` trailer**：Copilot 辅助的提交必须注明。

---

## 8. 其他约定

### 8.1 React 组件

- **优先函数组件**，不使用 class 组件。
- **Props 解构在签名中**：`function Comp({ a, b }: { a: A; b: B })`。
- **默认值在解构中**：`function Comp({ size = "md" })`。
- **条件渲染优先 `&&`**：`{show && <Content />}`。
- **列表 key 优先业务 ID**，不要用 `index`。

### 8.2 样式

- **Tailwind 作为主样式方案**。
- 自定义 CSS 仅用于 Tailwind 无法覆盖的场景（如 Virtuoso 内部 DOM）。
- 颜色使用设计系统 token：`text-[#111827]` `bg-[#fbfbfd]` `border-[#dfe3ec]`。
- `className` 使用 `cn()`（clsx + tailwind-merge）合并条件样式。

### 8.3 错误处理

- IPC 调用结果必须检查 `result.error`。
- 用户可见的错误通过 `setError` 展示在 ErrorBanner 中。
- 不吞异常：catch 块至少 `console.error`。

---

> **最后更新**：2026-06-14 | **基于**：P1–P7 拆分实践 | **维护者**：FE
