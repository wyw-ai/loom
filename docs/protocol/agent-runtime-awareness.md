# Agent 运行规则感知协议

状态：草案

日期：2026-07-06

本文定义 Loom 如何让 agent 感知 Loom 的运行规则，同时不再向 provider
预置或追加 Loom 自有的 system prompt。

## 目标

Loom 中的 agent 需要知道如何查看协作上下文、发送可见动作、使用 wake/private
消息，以及在需要时查阅更详细的操作指南。这些能力不应该依赖一段很长的
system prompt，也不应该依赖一段很长的 user message。

标准传递路径是：

1. `AGENTS.md` 在 agent 工作目录中提供最小运行契约。
2. 官方 Loom skill 负责识别场景，并指向对应的 guide topic。
3. `loom guide` 提供详细、官方的操作指南。
4. 当前 turn 的动态信息由后续 prompt 设计单独定义；本文只规定这些信息不进入
   `AGENTS.md`。

## 非目标

本文不定义：

- agent 的人格、角色或产品侧专属 instruction；
- provider session 的持久化规则；
- 给不支持读取工作区文件的 provider 准备一份长 prompt fallback。

## 设计原则

### 不再注入 Loom System Prompt

Loom 不应该把一份预制的 Loom 使用手册注入 provider system prompt。

Loom 的运行规则应该外置到工作区文件、skill 和 guide 中。这样可以保持 provider
prompt 干净，也避免和 agent 自己的 instruction 互相竞争。

### `AGENTS.md` 是启动契约

`AGENTS.md` 是 agent 在工作目录中最先应该看到的本地文档。Loom 只用它放稳定的
启动规则和索引指引。

它应该足够短，agent 每个 turn 都可以快速读取；也应该足够稳定，Loom 不需要频繁
改写它。

### `loom-guide` 是官方手册

详细用法放在独立的 guide 仓库中：

```text
https://github.com/wyw-ai/loom-guide.git
```

Loom 打包时内置该仓库的一份快照。用户或 agent 可以运行 `loom guide update`
从 GitHub 拉取更新版本。运行时查询 guide 时，应该优先使用本地更新缓存；如果
没有缓存或更新失败，则回退到内置快照。

`loom-guide` 的内容不应复制维护到 Loom 主仓库。Loom 构建和发布时只读取外部
仓库内容生成编译期快照。构建机可以通过 `LOOM_GUIDE_DIR` 指向该仓库；未设置时，
默认查找 Loom 仓库平级的 `../loom-guide`；如果本地路径不存在，则从官方 GitHub
仓库临时 clone 到构建输出目录，生成内置快照后删除临时 clone。需要覆盖远程地址时，
可以设置 `LOOM_GUIDE_REPO`。

### `loom-skills` 是官方 Skill 集合

Loom 默认给每个 agent 投影一个官方 `loom` skill。该 skill 的源仓库是：

```text
https://github.com/wyw-ai/loom-skills.git
```

该仓库可以维护多个 skill。Loom 当前默认内置和投影的是其中的 `skills/loom`
目录：

```text
skills/loom/SKILL.md
skills/loom/references/*.md
```

`SKILL.md` 负责描述何时使用 Loom skill，并按场景指向 `references` 下的文档。
`references` 保存 Loom 常识、消息路由、任务协作、状态和 artifact 等场景说明。

Loom 打包时内置 `skills/loom` 的一份快照，并在 agent turn 启动前把它写入本机
`data_root` 的内置 skill 区，再投影到当前 workspace 的 provider-native skill
目录。这样 agent 不需要依赖 system prompt，也能知道遇到 Loom 路由、任务、私信、
artifact、reminder 等场景时应该先读哪个 reference 或 guide topic。

### Host-owned hidden awareness

`AgentSpec.runtimeAwareness` 默认为 `native`。宿主应用如果已经提供自己的 agent-facing
通信协议和工具，可以设置为 `hidden`。该模式下 Loom 仍负责 channel/thread、投递、
队列、run 和 provider session，但不向 provider 暴露 Loom 操作面：

- 不写入或保留 Loom 生成的 workspace `AGENTS.md` 区块；
- 不投影默认 `loom` skill；
- turn prompt 只传递原始触发消息正文，不加入 Loom turn header、reply contract、
  history inspection 或 CLI command guidance；
- 不注入 `LOOM_*` / `AGENTX_*` provider 环境变量；仅使用中性的
  `RUNTIME_TRIGGER_MESSAGE_ID` 让宿主 wrapper 关联当前投递。

`hidden` 表示宿主接管 provider-facing instructions、工具和结果投递，不改变 Loom
内部的 durable message、inbox acknowledgement、run lifecycle 或 session 管理语义。

`loom-skills` 的内容同样不应复制维护到 Loom 主仓库。Loom 构建和发布时只读取外部
仓库内容生成编译期快照。构建机可以通过 `LOOM_SKILLS_DIR` 指向该仓库；未设置时，
默认查找 Loom 仓库平级的 `../loom-skills`；如果本地路径不存在，则从官方 GitHub
仓库临时 clone 到构建输出目录，生成内置快照后删除临时 clone。需要覆盖远程地址时，
可以设置 `LOOM_SKILLS_REPO`。

### Skill 负责指路，Guide 负责解释

官方 Loom skill 不应该复制完整手册。它的职责是识别常见场景，并告诉 agent 应该
打开哪个 `loom guide` topic 或使用哪个命令继续。

### Prompt 动态信息另行设计

当前 turn 的动态信息应该随用户请求一起进入 provider，但具体结构由后续 prompt
设计单独定义。本文不规定 user prompt 的最终形态。

本文只规定：provider prompt 不应该包含 Loom 的通用操作手册。

## 工作区解析

对 channel 中的 actor，Loom 在启动或恢复 provider 进程之前，先解析 provider
工作目录。

默认工作区是：

```text
<data_root>/channels/<channel_id>/agents/<actor_id>/workspace
```

如果 channel member 配置了自定义工作目录，Loom 使用该自定义目录。

workspace 里的 `AGENTS.md` 是 Loom 启动契约的权威位置：

```text
<workspace>/AGENTS.md
```

`data_root` 下的 provider 专用 home 目录属于实现细节。除非某个 provider 明确要求
home-level instruction path，否则它不应该作为 agent 感知 Loom 运行规则的主要
来源。

## `AGENTS.md` 写入规则

Loom 预置内容使用已有 marker 包裹：

```text
<!-- BEGIN loom -->
...
<!-- END loom -->
```

Loom 必须按以下规则处理：

1. 如果 `AGENTS.md` 不存在，创建文件并写入 Loom block。
2. 如果 `AGENTS.md` 已存在，并且同时包含 Loom begin/end marker，重新生成 Loom
   block；如果内容不同，只替换 marker 内部的 Loom block，并保留 marker 外的用户
   内容。
3. 如果 `AGENTS.md` 已存在，但不包含 Loom marker，把 Loom block 插入到文件最
   前面，并保留原有内容。

Loom 不应该改写 marker 外的用户内容。marker 内部属于 Loom 管理区域，用于同步
actor/channel 级基础上下文和稳定运行规则；当这些生成内容发生变化时，Loom 应把
marker 内部同步到最新状态。如果生成内容没有变化，Loom 不应产生文件写入。

## Loom Block 内容边界

Loom block 只应该包含最小、稳定的启动契约：

- agent 正在 Loom 中运行；
- 当前 actor 的基础身份信息；
- 当前 channel 的基础信息；
- 可见协作动作应该通过 Loom CLI 发出；
- wake/private 消息的基础规则；
- 如何查看当前 channel、thread、assignment 状态；
- 如何通过 `loom guide` 打开官方指南；
- 如果官方 Loom skill 可用，应该如何使用它。

Loom block 不应该包含完整命令手册、长示例、provider 实现说明，或者 guide 内容的
重复副本。

## 基础上下文与动态上下文边界

`AGENTS.md` 是 workspace/channel/actor 级启动契约，因此应该包含不随新 thread
变化的基础上下文。Loom block 应包含：

- 当前 `actor_id` 的具体值；
- actor display name；
- 当前 actor 的稳定角色摘要，前提是该信息来自 actor/profile 配置；
- 当前 `channel_id` 的具体值；
- channel name/title/description 等基础 channel 信息；
- 当前 channel 成员列表和成员 display name，前提是这些信息可以从 Loom 服务端
  权威状态生成；
- 当前 workspace 路径；
- 当前 Loom guide 和 skill 的读取方式。

这些内容虽然可能变化，但变化频率低于 thread/turn 状态，并且对 agent 理解“我是谁、
我在哪个 channel、我周围有哪些 actor”很重要。因此它们属于 Loom marker 内的生成
内容；只要最新生成结果和现有 block 不同，Loom 就应该同步 marker 内部。

Loom block 不应该包含随 thread 或 turn 变化的动态上下文，包括：

- 当前 `scope_id` 或 thread id；
- 当前 `LOOM_REPLY_TARGET` 的具体值；
- 当前 trigger message id；
- 当前 trigger actor；
- 当前最新消息正文；
- 当前 task、assignment 或 inbox 状态；
- 本次 turn 的临时决策、进度或输出。

这些动态信息应该随当前用户请求进入 provider，具体结构由后续 user prompt 设计
单独定义。本文不约束该结构。

如果产品侧或用户需要给某个 actor 写稳定身份设定，那应该作为独立的 actor/profile
instruction 管理。是否把它投影进 Loom marker 内部，由 actor/profile 配置决定；
用户手写的项目规则和本地约定应放在 Loom marker 之外。

## 官方 Loom Skill

官方 Loom skill 应安装或投影到 provider 能通过原生机制发现的位置。

内容来源是：

```text
https://github.com/wyw-ai/loom-skills.git
```

默认 skill id 是 `loom`。每个 agent turn 构造 workspace 前，Loom 应确保该 skill
存在并投影到当前 workspace；channel/member scope skills 和 actor bundle skills
可以和它并存，但不应替代默认 `loom` skill。

Loom 只内置并同步 `loom-skills` 仓库中的 `skills/loom` 目录。运行时写入
`data_root/builtin/skills/loom` 时，应同步完整目录树，包括 `SKILL.md` 和
`references` 下的文档；如果内置快照删除了旧文件，运行时也应清理
`data_root/builtin/skills/loom` 中对应的旧文件，避免 agent 读到过期 reference。

不经过原生 `agent serve` workspace 构造路径的外部 runtime adapter，应从当前安装的
Loom CLI 导出同一份编译期快照：

```text
loom skill materialize loom --output <dedicated-skill-directory>
```

该命令离线执行，`output` 本身就是 `loom` skill 目录。adapter 应把它作为内置 runtime
skill 绑定到每个 agent，且不能让 actor bundle、项目 skill 或手工 skill 操作覆盖它。
这样原生 provider 与外部 runtime 使用同一 Loom 版本的官方 skill，不依赖另一个 daemon
的私有 `data_root`。

为避免误删调用方数据，导出目录名必须为 loom；首次导出只接受不存在或空目录，
后续 reconcile 只接管带有可验证 Loom managed marker 的目录。

skill 应专注于场景路由：

- 判断 agent 当前是否需要 channel state、thread state、wake message、
  private message、artifact handling、assignment state 或 guide update；
- 告诉 agent 应该打开哪个 `loom guide` topic；
- 提供继续执行所需的最小命令提示。

skill 的内容应该从 guide index 生成，或至少和 guide index 保持一致，确保 topic
名称和路由规则不会漂移。

## `loom guide`

`loom guide` 是 Loom 运行帮助的权威接口。

必须支持的命令：

```text
loom guide list
loom guide show <topic>
loom guide search <query>
loom guide update
```

guide 接口应该同时支持面向人的可读输出，以及面向 agent 的结构化输出，例如
JSON。

打包和更新规则：

1. Loom 从 `https://github.com/wyw-ai/loom-guide.git` 打包一份内置快照。
2. `loom guide update` 把最新 guide 拉取到本地缓存。
3. 运行时读取 guide 时，优先使用本地缓存。
4. 本地缓存不存在或不可用时，回退到内置快照。

## Prompt Assembly 契约

Loom runtime guidance 必须从 prompt assembly 中移除。

默认 prompt assembly 不应该包含：

- `scope_bootstrap`；
- `seed_manifest`；
- 生成式 Loom 使用手册；
- 为注入 Loom 运行规则而存在的 provider 参数，例如
  `--append-system-prompt {prompt.system}`。

turn prompt 的具体内容由后续 user prompt 设计定义。该设计可以包含当前 user
message、turn metadata、assignment context、近期 runtime state 等，但不在本文中
展开。

如果某个 provider 需要通过原生机制读取 instruction，Loom 应配置该 provider
读取 workspace `AGENTS.md` 或投影出来的 skill 文件，而不是重新引入 Loom system
prompt fallback。

## 启动流程

每个 agent turn：

1. 解析 actor 和 channel scope。
2. 解析 workspace 目录；如果配置了自定义工作目录，使用自定义目录。
3. 按本文规则确保 `<workspace>/AGENTS.md` 存在。
4. 确保默认 `loom` skill 可以被 provider 的 skill discovery 机制发现。
5. 构造不包含 Loom runtime guide 内容的 turn prompt；具体 user prompt 结构由后续
   设计定义。
6. 使用 workspace 作为 `cwd` 启动或恢复 provider。
7. agent 读取 `AGENTS.md`，使用 skill 做场景路由，并在需要时通过 `loom guide`
   打开详细指南。

## 兼容性说明

不同 provider 对 workspace instruction 的发现机制不同。兼容逻辑应该放在
provider adapter 中，而不是放回共享的 Loom system prompt。

如果 provider 不能从 `cwd` 读取 `AGENTS.md`，Loom 可以通过 provider 原生
instruction path 指向同一份 workspace bootstrap 内容。这是 provider adapter
层的职责，不应该改变 prompt assembly 契约。

已经包含 Loom marker 的既有 `AGENTS.md` 会在 actor/channel 级基础上下文或稳定规则
变化时同步 marker 内部。marker 外的用户内容不应被改写。可变的详细手册由
`loom-guide` 承担。

## 迁移计划

1. 将本文作为 agent 感知 Loom 运行规则的协议来源。
2. 引入外部 `loom-guide` 的打包和更新机制。
3. 引入外部 `loom-skills` 的 `skills/loom` 打包快照，并默认投影到每个 agent
   workspace。
4. 把生成的 Loom `AGENTS.md` block 缩短为最小启动契约。
5. 修改 `AGENTS.md` 生成逻辑：已有 marker block 时，只有生成内容不同才替换
   marker 内部。
6. 从默认 prompt assembly 中移除 Loom runtime guidance。
7. 移除仅用于注入 Loom 运行规则的 provider system prompt append 行为。
8. 更新 provider adapter，让它们通过 provider 原生机制发现 workspace
   `AGENTS.md` 和官方 Loom skill。
9. 更新仍在描述 `scope_bootstrap`、prompt 注入式 Loom 手册、或 AGENTS/prompt
   双写规则的旧文档。

## 验收标准

- 使用默认工作区的新 channel actor 会得到
  `<data_root>/channels/<channel_id>/agents/<actor_id>/workspace/AGENTS.md`。
- 使用自定义工作区的 channel actor 会得到 `<custom_workspace>/AGENTS.md`。
- `AGENTS.md` 的 Loom block 包含当前 actor/channel 级基础上下文。
- 已有 marker 的 `AGENTS.md` 在生成内容不变时不会被改写。
- 已有 marker 的 `AGENTS.md` 在 actor/channel 基础上下文变化时，只替换 marker
  内部内容。
- 已存在但没有 marker 的 `AGENTS.md` 会在最前面插入 Loom block。
- `AGENTS.md` 不包含 thread/turn/task 级动态状态，例如当前 trigger message、
  reply target、task 状态或 assignment 状态。
- provider prompt 不包含 Loom 通用使用手册。
- `loom guide` 可以在离线状态下使用内置快照。
- `loom guide update` 可以从 `https://github.com/wyw-ai/loom-guide.git` 刷新
  本地缓存；运行时可用 `LOOM_GUIDE_REPO` 环境变量覆盖仓库地址（例如私有镜像
  或带凭证的 URL）。
- 每个 agent workspace 都能发现默认 `loom` skill。
- 默认 `loom` skill 的内容来自 `https://github.com/wyw-ai/loom-skills.git` 的
  `skills/loom` 打包快照。
- 官方 Loom skill 负责把 agent 指向 guide topic，而不是复制 guide 全文。
