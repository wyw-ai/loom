# Provider 扩展设计

本文描述 Loom agent runtime 的下一版 Provider 模型。目标是让新增 Provider
尽量数据驱动，而不是继续把 provider id、启动参数和输出解析逻辑硬编码在
Rust 里。

## 对原有功能的影响

这个设计不是纯内部重构，会有明确的顶层配置边界变化。先列出影响，避免后文只讨论
Provider manifest 时忽略 Loom 原有能力。

| 原有功能/边界 | 影响 | 设计结论 |
| --- | --- | --- |
| agent 启动与 provider discovery | 从 `provider id -> Rust hardcode` 改成 `ProviderManifest -> ProviderRuntimePlan` | 行为应由内置 manifest 复刻，启动入口不再按 provider id 分支。 |
| AgentSpec | 不再保存完整 `transport`，也不由 GUI/server 生成最终文件 | AgentSpec 由目标 daemon 写入，只保存 actor id/displayName、静态 instructions、metadata、trigger/promptTemplate、memory policy、providerRef 和选中的 model/reasoning。 |
| daemon machine 配置 | 只保存 daemon 自己上线和运行所需的宿主信息 | daemon 拥有本机 provider registry、AgentSpec、profile/memory 与运行状态；GUI/server 不直接写 daemon 私有配置。 |
| GUI 本地 machine/agent 配置 | 删除运行时意义 | GUI 只保存人类客户端偏好和 server 连接选择；agent/provider 列表来自 server 上的 daemon inventory，修改必须变成 machine command。 |
| machine-level provider override | 收敛为 daemon-local ProviderManifest variant | 如果某台 host 需要特殊 command/args/env，用该 daemon 的 `{loom.configDir}/providers/<id>.json` + `extends` 表达。 |
| prompt 拼装 | 从单一 envelope 改成结构化 prompt parts | Loom 仍负责生成 actor context、Loom 规则、current state、message/task context；Provider 只决定这些 parts 怎么映射到 system/user/full。 |
| agent workspace `.loom` prompt 文件 | 新增可选输入源 | ProviderManifest 可以声明它支持读取当前 agent workspace 下 `.loom/<path>` 的哪些文件；daemon composer 读取后转成普通 prompt part，仍由 `prompt.outputs` 决定是否注入 system/user/full。 |
| handoff/task 触发语义 | 不改变 | `trigger_prefix`、`promptTemplate`、latest message、assignment context 仍由 Loom composer 产生；manifest 不能自己改写业务语义。 |
| run trace 与 GUI 可见消息 | 不改变 | Provider stdout 仍进 run trace；GUI 可见消息仍来自 agent 显式 `loom message send`，或部署显式开启的 auto-publish。 |
| AdapterEvent 边界 | 不改变 | `Text/ToolUse/Status/Finished/Error/ActionRequest` 仍是 adapter 到 Loom runtime 的公共事件；Provider session 捕获是 Provider runtime 内部事件，不扩展 GUI message 语义。 |
| session/resume | 存储位置会改变 | session id 只进入 runtime state；由 `loom_uuid` 生成或由 decoder 内部捕获，不写 AgentSpec/machine config。 |
| model/reasoning 选择 | 配置归属会改变 | ProviderManifest 提供 provider 级模型菜单和 argv/env 映射；AgentSpec 只记录具体 agent 选中的 model/reasoningEffort。 |
| ServiceSpec / service 插件 | 不受影响 | service 进程仍由 ServiceSpec 管理，不进入 ProviderManifest 体系。 |

因此这个方案会打破“GUI 自己维护 agent/provider 定义”的旧路径，但不改变 Loom 的核心运行模型：
actor/scope/message/run trace、显式 `loom message send`、AdapterEvent 翻译层、
ServiceSpec 都应保持原语义。已有配置如果需要保留，只能通过显式离线导入工具转换到
daemon-owned AgentSpec / ProviderManifest；导入完成后，同一台 host 上 daemon 只保留
一个 agent/provider 事实源，GUI 不再绕过 daemon 读写这些定义。

## 问题

当前一个 Provider 实际承担两类职责，但两者都部分硬编码：

- 启动方式：`discovery.rs` 根据 provider id 决定命令、固定 argv、模型参数、
  session 规则、prompt 传递方式和输出格式。
- 输出解析：`command.rs` 根据 `CommandOutputFormat` 枚举调用 Rust parser，
  例如 Claude stream JSON、Copilot JSON、Codex stream JSON 或纯文本。

这个方式在内置 Provider 很少时可行，但每加一个 Provider 都需要改多处
Rust 代码。即使新 Provider 只是另一种 CLI 包装，仍然要新增 hardcode。

Provider 的契约应该显式表达两个点：

1. Loom 如何启动 Provider，以及如何把 Loom 拼好的 prompt 映射到 Provider
   的命令、参数、stdin 或 env。
2. Loom 如何解析 Provider 的 stdout/stderr，转换成 `ProviderRuntimeEvent`；其中
   文本、工具、状态、错误和完成事件再映射成现有 AdapterEvent。

这里的“prompt”不应该只是一个已经拼死的单一大字符串。Loom 应该先
产出一组结构化 prompt parts，再由 Provider 决定这些 parts 如何组合、哪些进入
system prompt、哪些进入 user prompt、是否插入分隔符、顺序如何排列。

## Loom 顶层边界

Loom 仍然拥有 prompt 内容生成逻辑。ProviderManifest 不负责自己生成 actor metadata、
memory、handoff 或 assignment 内容。daemon 会先生成一组结构化 prompt parts，
例如 actor context、Loom 规则、当前时间、最近消息、
最新 handoff/task message 等。Provider 负责决定这些 parts 的顺序、拼接方式，
以及它们映射到 Provider 的 system/user prompt、argv、stdin 或 env 中哪个位置。

Provider stdout 默认仍然只进入 run trace。Provider 结果是否成为 GUI 可见消息，
应继续由 Loom 策略决定：默认必须由 agent 显式调用 `loom message send`；只有
部署显式开启 auto-publish 时，最终文本才会自动发布成可见消息。auto-publish 是
Loom runtime / actor policy，不属于 ProviderManifest。

更高一层的所有权原则必须固定：

```text
GUI
  人在哪。负责展示、输入和发起意图；不拥有 agent/provider/profile/memory 文件。

server
  沟通数据存在哪。负责 message/task/run trace、daemon 连接状态、daemon inventory
  快照、machine command 队列和结果；不解释 ProviderManifest，不生成 AgentSpec。

daemon
  agent 跑在哪。负责目标 host 上 agent/provider/profile/memory/runtime state 的唯一
  落盘与执行。
```

这意味着 GUI 看到的 hosts/agents/providers 必须来自 server 保存的 daemon inventory
快照；GUI 创建、编辑、删除 agent 或 provider 时，只能向 server 提交面向某个
machine/daemon 的 command。server 只排队、转发和记录结果；真正校验、写盘、刷新
inventory 的是目标 daemon。即使 GUI 和 daemon 在同一台机器，也应走同一条路径，避免
本机特殊分支重新引入第二份事实源。

## 建议 Schema

引入 provider manifest。内置 Provider 可以作为 embedded manifest 随程序发布；
本地自定义 Provider 可以放在配置目录下，例如：

```text
{loom.configDir}/providers/<provider_id>.json
```

建议结构：

```json
{
  "schemaVersion": 1,
  "id": "claude",
  "displayName": "Claude Code",
  "detect": {
    "candidates": ["claude"]
  },
  "modes": {
    "print": {
      "transport": "command",
      "command": "{bin}",
      "prompt": {
        "workspaceFiles": [
          {
            "key": "persona",
            "path": "persona.md",
            "title": "System: Provider persona",
            "roleHint": "system",
            "optional": true,
            "maxBytes": 32768
          }
        ],
        "outputs": {
          "system": {
            "join": "\n\n",
            "include": ["actor_context", "agent_instructions", "workspace_file.persona", "scope_bootstrap"]
          },
          "user": {
            "preset": "loom_turn"
          }
        }
      },
      "args": [
        "--add-dir", "{loom.configDir}",
        "--permission-mode", "bypassPermissions",
        "--output-format", "stream-json",
        "--verbose",
        "--session-id", "{session.id}",
        { "when": "model", "args": ["--model", "{model}"] },
        "--append-system-prompt", "{prompt.system}",
        "-p", "{prompt.user}"
      ],
      "stdout": {
        "format": "builtin",
        "name": "claude_stream_json"
      },
      "session": {
        "idSource": "loom_uuid",
        "scope": "actor_scope"
      }
    }
  },
  "models": {
    "default": "sonnet",
    "choices": [
      { "id": "sonnet", "label": "Sonnet" },
      { "id": "opus", "label": "Opus" }
    ]
  }
}
```

关键点是 `modes.print.args` 表达完整 argv 模板，同时 `modes.print.prompt.outputs`
表达 prompt parts 的组合方式。manifest 可以把 `{prompt.system}`、
`{prompt.user}`、`{prompt.full}` 或任意命名 prompt 输出放在任意位置，因此 Loom
不再需要对 `-p`、末尾追加 prompt、stdin 或 env prompt 做 provider 特殊逻辑。
`prompt.workspaceFiles` 是可选能力：Provider 可以声明自己认识哪些
`{paths.cwd}/.loom/<path>` 文件。daemon composer 在每个 turn 渲染 prompt 前读取这些
文件，把存在的文件转成 `workspace_file.<key>` prompt part，然后仍然交给
`prompt.outputs` 的 `include` / `template` / `preset` 组合。Provider 不能借这个机制
任意读 workspace 文件；它只能读取 manifest 明确声明且通过校验的 `.loom` 相对路径。

`models` 只描述 UI 和默认模型选择；模型是否进入 CLI 参数，也由 mode 的
`args`/`env`/`stdin` 模板显式决定。也就是说，`--model {model}` 不应该藏在
另一个隐式分支里。

daemon-local Provider 可以通过 `extends` 继承内置 Provider，再用同一套 patch 语义覆盖
mode。这样旧 `MachineConfig.providers[]` 这种 command/args/env 局部覆盖不需要继续
作为 GUI/server 可见的独立配置面存在。
没有 `extends` 时，`modes.<name>` 是完整 Provider mode；存在 `extends` 时，
`modes.<name>` 可以是 `ModePatch`，只覆盖被声明的字段。

`detect.candidates` 是最小检测方式。内置 Provider 还应在测试里固定一组 help/version
样本，防止 CLI 升级后参数名变化但 manifest 仍然静默通过。自定义 Provider 可以先只
写 `candidates`；后续再扩展可选的 `versionCommand`、`helpCommand`、
`requiresHelpFlags` 做更严格校验。

如果某个 Provider 不支持 system prompt，可以在 prompt 配置中把所有 parts 合成
`prompt.full`，再传给一个普通 prompt 参数。

## Provider Mode

一个 Provider 可以声明多个 mode。Loom 为每个 agent runtime 选择一个 mode。

- `print`：每个 turn 启动一次命令。这是当前 Claude、Qoder、Copilot、Codex、
  OpenCode 的默认形态。它最简单，也最容易 supervise。
- `interactive`：使用 Loom 拥有的 session id 和 completion sentinel 的交互式命令。
  对应当前 `interactive_command` transport。
- `acp`：长生命周期 ACP stdio 进程。prompt 通过 `session/prompt` 发送，不走 argv。

mode 选择应该由 AgentSpec 或 Provider 默认值显式决定。GUI 请求创建 agent 时默认提交
`print`，除非目标 daemon 的 Provider inventory 声明了更好的默认 mode。

## Prompt Parts 与拼接策略

Loom prompt composer 应输出结构化 parts，而不是只输出一个字符串。建议最小集合：

```text
actor_context       Loom 注入的 actor id/displayName 和当前 actor 上下文
agent_instructions  AgentSpec 的静态 instructions，属于 system-side prompt part
workspace_file.*    ProviderManifest 声明的 agent workspace `.loom` 文件内容
trigger_prefix      AgentSpec triggerPromptPrefix
bootstrap_memory    长期/启动 memory
scope_bootstrap     Loom CLI、协作规则、当前 scope 等首轮规则
turn_memory         本 turn 选中的 memory
runtime_context     本地时间、最近消息等运行时上下文
latest_message      Latest Loom message，含路由、scope、task 元信息
assignment_context  task assignment 的权威 JSON 上下文；非 task 场景为空
turn_input          prompt_template 包装后的 latest_message + assignment_context
user_message        turn 侧输入，等价于 trigger_prefix + turn_input
```

新模型不把历史 `identity.md` / `soul.md` 当作 agent 的标准字段或 Provider 输入。
agent 的静态行为说明属于 `AgentSpec.instructions`，展示名、头像、UI 描述等信息属于
`actor._meta`；长期上下文属于 memory。旧
identity/soul 文件如需保留，只能由显式迁移工具转换成新的 metadata 或 memory，runtime
不再把它们作为默认 prompt part 读取。

但 Provider 可以显式声明自己的 workspace prompt 文件。这个能力用于兼容“某个
Provider/agent 生态希望在 workspace 里维护 persona、policy、soul、identity、
project note 等文件”的场景，同时不把这些名字做成 Loom 标准字段。

建议 schema 放在 mode 的 `prompt.workspaceFiles` 下：

```json
{
  "prompt": {
    "workspaceFiles": [
      {
        "key": "persona",
        "path": "persona.md",
        "title": "System: Persona",
        "roleHint": "system",
        "optional": true,
        "maxBytes": 32768
      },
      {
        "key": "operating_rules",
        "path": "rules/operating.md",
        "title": "System: Operating rules",
        "roleHint": "system",
        "optional": true,
        "maxBytes": 32768
      }
    ],
    "outputs": {
      "system": {
        "join": "\n\n",
        "include": [
          "actor_context",
          "agent_instructions",
          "workspace_file.persona",
          "workspace_file.operating_rules",
          "scope_bootstrap"
        ]
      },
      "user": {
        "preset": "loom_turn"
      }
    }
  }
}
```

运行时规则：

- daemon 在 resolve 出 Provider mode 后、渲染 `prompt.outputs` 前，读取当前 turn 的
  agent workspace 下 `{paths.cwd}/.loom/<path>`。
- 每个声明项生成一个普通 prompt part，key 固定为 `workspace_file.<key>`。Provider
  后续用 `include` 或 `template` 引用它，例如 `{workspace_file.persona}`。
- 文件不存在且 `optional=true` 时该 part 为空并自动跳过；`optional=false` 时本轮启动
  失败并给出配置错误。
- 读取内容作为 raw content；标题来自 manifest 的 `title`，是否渲染标题仍由
  `renderTitle` 控制。
- `roleHint` 只是默认建议。文件是否进入 system/user/full 仍由当前 Provider 的
  `prompt.outputs` 决定。

第一版只支持显式声明的相对文件路径，不支持随意扫描所有文件进入 prompt。原因是
prompt 文件会影响模型行为和 token 成本，必须可校验、可解释、顺序稳定。后续如确实
需要目录批量导入，可以扩展受限 `glob`，但仍必须有 `keyPrefix`、`maxFiles`、
`maxBytesPerFile`、确定性排序和路径校验。

每个 part 至少包含：

```json
{
  "key": "scope_bootstrap",
  "title": "System: Loom multi-actor context",
  "content": "...",
  "roleHint": "system"
}
```

`part.content` 应该是未包标题的正文，标题由 `part.title` 和输出的 `renderTitle`
决定，避免 Provider 模板里再手写一次标题导致重复。composer 输出 raw content +
metadata。

Provider manifest 可以把这些 parts 组合成任意命名 prompt 输出。输出支持三种写法：

- `preset`：使用 Loom 内置默认组合，减少每个 Provider 重复维护 part 列表。
- `include` / `join`：结构化拼接，适合按 part 顺序组合。
- `template`：自由模板，适合精确控制文案、分隔符和顺序。

```json
{
  "prompt": {
    "outputs": {
      "system": {
        "preset": "loom_system"
      },
      "user": {
        "preset": "loom_turn"
      },
      "full": {
        "preset": "loom_full"
      }
    }
  }
}
```

建议内置 preset：

```text
loom_system  actor_context + agent_instructions + bootstrap_memory + scope_bootstrap
loom_turn    turn_memory + runtime_context + user_message，承载每轮动态上下文、
             trigger_prefix 和 prompt_template 行为
loom_full    loom_system + loom_turn，等价于当前单字符串 envelope 的语义
```

preset 不是黑盒。Provider 可以改用 `include` 或 `template` 完全控制顺序；未来 Loom
新增 part 时，只需要更新 preset，旧 Provider 不会被迫修改 manifest。
`workspace_file.*` 不应被自动塞进内置 preset；Provider 既然声明了文件槽位，就必须在
对应 output 里显式 `include` 或用 `template` 引用，避免 workspace 文件因为默认组合
意外改变模型行为。

如果 manifest 没有声明 `prompt.outputs`，Loom 应自动提供默认输出：
`full = loom_full`。这样最小自定义 Provider 可以直接引用 `{prompt.full}`。只要
manifest 显式引用了不存在的 `{prompt.<name>}`，校验就应该失败。

`template` 中可以直接引用原子 part：

```json
{
  "prompt": {
    "outputs": {
      "claude_system": {
        "template": "{actor_context}\n\n{scope_bootstrap}"
      },
      "claude_user": {
        "join": "\n\n",
        "include": ["latest_message", "assignment_context"],
        "renderTitle": "always"
      }
    }
  }
}
```

组合规则：

- `include` 决定顺序，空 part 自动跳过。
- `join` 决定 part 之间的拼接字符串。
- `prefix` / `suffix` 可用于包装某个组合结果。
- `template` 精确控制输出文本。未出现或为空的 part 默认展开为空字符串；如果需要
  缺失时报错，应在输出或变量上声明 `required`。
- `renderTitle` 可以决定是否保留 part 标题，例如 `always`、`never`、`auto`。
- `required` 可声明某个 part 缺失时启动失败，默认缺失就跳过。
- `roleHint` 只是 Loom 默认建议，最终 system/user/assistant 映射由 Provider
  manifest 决定。
- `user_message` 是 turn 输入。配置新 Provider 时优先使用 `loom_turn` preset；
  只有在 Provider 明确要接管 turn 文本包装时，才直接引用 `latest_message`、
  `assignment_context`、`turn_input` 或 `trigger_prefix`。

这样 Claude 这种支持 system prompt 的 Provider 可以把较稳定的 actor context 和 Loom
规则放到 system，把每轮 current state、message list、最新 handoff/task message
放到 user；而只支持一个 prompt 字符串的 Provider 可以直接使用 `{prompt.full}`。

## 模板变量

Provider 的 argv/env/stdin 模板应支持这些运行时变量：

```text
{bin}                  检测到的可执行文件路径
{prompt.<name>}        prompt.outputs 生成的命名 prompt，例如 system/user/full
{loom.configDir}       目标 daemon 的配置目录；按 host/machine 隔离，不随 GUI 当前 server 切换
{loom.server}          server websocket URL
{loom.actor}           actor id
{loom.scope.id}        当前 channel/thread scope id
{loom.scope.kind}      channel 或 thread
{loom.run.id}          当前 run id
{loom.trigger.id}      触发当前 turn 的 message/source id
{loom.trigger.actor}   触发当前 turn 的 actor id
{model}                当前选中的模型 id
{reasoningEffort}      当前选中的 reasoning effort，如果有
{session.id}           当前 Provider session id。可能是 Loom 生成的稳定 UUID，
                       也可能是从 Provider 输出捕获的 id
{paths.cwd}            本 turn 工作目录
```

模板展开必须 argv-safe：JSON 数组里的每个元素展开后仍然是一个 argv token。
Loom 不应该把 argv 拼成 shell 字符串再执行。

为了支持可选模型、可选 reasoning effort 或不同权限配置，`args` 除了字符串，也可以
支持条件片段：

```json
{
  "args": [
    "exec",
    "--skip-git-repo-check",
    { "when": "model", "args": ["--model", "{model}"] },
    { "when": "reasoningEffort", "args": ["--effort", "{reasoningEffort}"] },
    "{prompt.full}"
  ]
}
```

`when` 只判断 Loom 运行时变量是否存在且非空。第一版不需要表达复杂逻辑；复杂逻辑
应该拆成不同 mode 或本地 Provider variant。

对于较大的 prompt，Provider 可以要求通过 stdin 或 env 传入任意命名 prompt：

```json
{
  "args": ["run", "--prompt-file", "-"],
  "stdin": "{prompt.full}"
}
```

或者：

```json
{
  "args": ["run"],
  "env": {
    "PROVIDER_SYSTEM_PROMPT": "{prompt.system}",
    "PROVIDER_USER_PROMPT": "{prompt.user}"
  }
}
```

这会替代当前 `PromptVia::{Args,Stdin,Env}` 的特殊分支，变成统一模板模型。

## Session 策略

Provider session 不应该默认依赖 stdout 解析。更稳的优先级是：

1. `loom_uuid`：Loom 为 `(provider, actor, scope)` 生成并持久化稳定 UUID，Provider
   每轮都通过 argv/env/stdin 收到同一个 `{session.id}`。如果 CLI 支持“指定 session
   id，不存在则创建、存在则继续”，优先使用这种方式。
2. `provider_capture`：Provider 自己生成 session id，stdout/stderr decoder 发出
   `ProviderRuntimeEvent::Session`，Loom 保存后在后续 turn 通过 resume 参数传回。
3. `none`：每轮都是无状态新进程。

建议 schema：

```json
{
  "session": {
    "idSource": "loom_uuid",
    "scope": "actor_scope"
  }
}
```

`scope` 建议先支持：

- `actor_scope`：同一个 actor 在同一个 channel/thread 内复用一个 Provider session。
  这是当前 command transport session map 的语义。
- `actor`：同一个 actor 全局复用一个 Provider session。
- `turn`：每个 turn 新 session，通常只用于调试。

如果 Provider 只能自己吐出 session id，则使用捕获模式：

```json
{
  "session": {
    "idSource": "provider_capture",
    "scope": "actor_scope",
    "resumeArgs": ["--resume", "{session.id}", "-p", "{prompt.user}"]
  }
}
```

`provider_capture` 的含义是“session id 来自 Provider 输出”，但具体怎么从输出里取值
仍属于 stdout/stderr parser。decoder 可以通过 JSONL reducer 或 builtin decoder 发出
`ProviderRuntimeEvent::Session`，session manager 只消费这个事件并写 runtime state。
Claude Code 和 Copilot 这类支持指定 session id 的 Provider，不应该为了拿
session id 再解析 stdout；stdout parser 只负责 run trace、最终文本、工具调用、
usage、错误，以及必要时的 `ProviderRuntimeEvent::Session`。

## 输出解析模型

Provider 必须声明自己的 `stdout` / `stderr` parser spec。runtime 不再通过 provider id
推断解析方式；即使 parser 的实现是内置 Rust 代码，也必须由 manifest 显式引用。
parser 的输出先进入 `ProviderRuntimeEvent`；其中可见执行事件再映射到现有
`AdapterEvent`，session 捕获等控制事件留在 Provider runtime 内部。

推荐 parser 类型：

- `text`：收集完整 stdout，进程结束后发一个最终 text event。
- `json`：把完整 stdout 解析成一个 JSON 文档，再提取字段。
- `jsonl`：逐行解析 stdout JSON，边读边发事件。
- `regex`：兜底文本提取方式。
- `builtin`：命名内置 decoder，用于过于复杂或性能敏感、暂时无法 DSL 化的稳定协议。

`builtin` 也必须写在 manifest 里，例如 `{ "format": "builtin", "name":
"codex_stream_json" }`。这保持了“解析选择 provider 化”，同时允许复杂 decoder 先由
Rust 实现。

每个 parser 输出 `ProviderRuntimeEvent`：

- `text`：候选文本，映射为 `AdapterEvent::Text` 并进入 run trace；是否发布成可见消息仍由 Loom 策略决定。
- `tool_use`：Provider 报告的工具调用 trace，映射为 `AdapterEvent::ToolUse`。
- `status`：Provider 进度或状态 trace，映射为 `AdapterEvent::StatusChange`。
- `error`：Provider 错误 trace，映射为 `AdapterEvent::Error`。
- `usage`：token usage metadata。
- `session`：Provider runtime 内部事件，仅用于 `provider_capture` 模式下报告捕获到的 session id，不进入 GUI message。
- `finish`：显式完成信号，映射为 `AdapterEvent::Finished`；没有显式完成时，进程退出仍会产生 Finished。

输出解析不能只靠“每条 JSON 映射一个事件”。Copilot/Codex 这类 CLI 会同时输出
中间解释、工具轨迹、子 agent 轨迹和最终回答，Provider 需要能声明 stateful reducer：

- `emit`：看到一条输入就发 `ProviderRuntimeEvent`，适合 Claude assistant text/tool_use。
- `reduce.finalText`：在整个 stdout 结束后，从多条 JSONL 中选最终文本。
- `reduce.deltaText`：累积流式 delta，作为 finalText 的 fallback。
- `capture.session`：仅在 Provider 不能由 Loom 指定 session id 时，从 JSONL 或完整
  JSON 中捕获最后一个非空 session id。

`capture.session` 属于 decoder，不属于 `session` 本身。`session.idSource =
provider_capture` 只说明 session id 的来源是 Provider 输出；具体从哪种 stdout/stderr
协议里取值必须由 parser 声明。例如：

```json
{
  "session": {
    "idSource": "provider_capture",
    "scope": "actor_scope",
    "resumeArgs": ["--resume", "{session.id}", "-p", "{prompt.user}"]
  },
  "stdout": {
    "format": "jsonl",
    "capture": {
      "session": {
        "mode": "lastNonEmpty",
        "path": "$.session_id"
      }
    }
  }
}
```

因此 `session.capture` 不应继续存在于 ProviderManifest；否则会把 stdout 协议细节混回
session 策略里。

Copilot 当前 `--output-format json --stream off` 实际是 JSONL，每行一个事件。现有
`CommandOutputFormat::CopilotJson` 的关键逻辑是：忽略 sub-agent 和 parent tool call
事件，忽略 thinking/reasoning phase，取最后一个 root `assistant.message`，如果没有
完整 message 再拼接 `assistant.message_delta`。这应被表达成 Provider parser，而不是
继续由 provider id 硬编码：

```json
{
  "stdout": {
    "format": "jsonl",
    "reduce": {
      "finalText": {
        "mode": "lastNonEmpty",
        "path": "$.data.content",
        "when": {
          "all": [
            { "path": "$.type", "equals": "assistant.message" },
            { "path": "$.agentId", "absentOrNull": true },
            { "path": "$.data.parentToolCallId", "absentOrNull": true },
            { "path": "$.data.phase", "notIn": ["thinking", "reasoning"] }
          ]
        },
        "fallback": {
          "mode": "concat",
          "path": "$.data.deltaContent",
          "when": {
            "all": [
              { "path": "$.type", "equals": "assistant.message_delta" },
              { "path": "$.agentId", "absentOrNull": true },
              { "path": "$.data.parentToolCallId", "absentOrNull": true }
            ]
          }
        }
      }
    }
  }
}
```

通用 JSONL 可以这样表达：

```json
{
  "stdout": {
    "format": "jsonl",
    "events": [
      {
        "when": { "path": "$.type", "equals": "text" },
        "emit": {
          "type": "text",
          "text": "$.text",
          "partial": true
        }
      },
      {
        "when": { "path": "$.type", "equals": "done" },
        "emit": { "type": "finish" }
      }
    ]
  }
}
```

第一版实现可以保持 selector 很小：

- JSONPath 子集：`$.field`、`$.a.b`、`$.arr[*]`、数字下标。
- 条件操作：`equals`、`notEquals`、`exists`、`absentOrNull`、`notEmpty`、`in`、`notIn`、
  `all`、`any`、`not`。
- 提取器：直接 path、`firstNonEmpty`、`lastNonEmpty`、`joinText`、`concat`。

这已经足够表达当前 Claude/Qoder stream JSON、Copilot JSONL final text 提取，以及
大多数 JSONL Provider 输出。不需要一开始就引入完整脚本引擎。Codex JSONL 的历史
形态较多，第一版可以继续使用 `builtin: codex_stream_json`，但这个别名也应该出现在
manifest 中，而不是通过 provider id 推断。

## 现有 Provider 接入结论

本节只用于验证 ProviderManifest 能覆盖当前内置 Provider，不作为架构约束。新增
Provider 不应该被迫模拟 Claude、Copilot 或 Codex 的启动/输出形态；只要能声明启动
模板、prompt 映射、session 策略和输出解析，就应能接入。

以下结论基于当前代码硬编码和本机 CLI help 抽查：

```text
Claude Code          2.1.92
Qoder CLI            1.0.6
GitHub Copilot CLI   1.0.54
Codex CLI            0.134.0
OpenCode             1.15.11
```

### Claude Code

Claude Code 支持非交互 `-p/--print`、`--output-format stream-json`、
`--append-system-prompt`、`--system-prompt`、`--resume`、`--add-dir`、`--plugin-dir`、
`--session-id`、`--model` 和 `--permission-mode bypassPermissions`。Loom 应优先使用
`--append-system-prompt`，因为 `--system-prompt` 可能替换 Claude Code 默认系统行为，
影响内置工具、默认规则或 CLI 自身约束。Session 建议用 Loom 生成的稳定 UUID，通过
`--session-id {session.id}` 传给 Claude Code，而不是从 stream-json 里捕获。

建议 print mode：

```json
{
  "args": [
    "--add-dir", "{loom.configDir}",
    "--permission-mode", "bypassPermissions",
    "--output-format", "stream-json",
    "--verbose",
    "--session-id", "{session.id}",
    { "when": "model", "args": ["--model", "{model}"] },
    "--append-system-prompt", "{prompt.system}",
    "-p", "{prompt.user}"
  ],
  "stdout": { "format": "builtin", "name": "claude_stream_json" },
  "session": {
    "idSource": "loom_uuid",
    "scope": "actor_scope"
  }
}
```

### Qoder CLI

Qoder CLI 的 print 接口与 Claude Code 接近，支持 `-p/--print`、`--output-format`、
`--append-system-prompt`、`--system-prompt`、`--resume`、`--add-dir`、`--plugin-dir`、
`--model`、`--reasoning-effort` 和 `--permission-mode bypass_permissions`。
现有 Loom 代码把 Qoder 按 Claude stream JSON 解析，并使用 `--yolo`。新 manifest
应优先写文档化参数，并直接声明 session 策略：

```json
{
  "args": [
    "--add-dir", "{loom.configDir}",
    "--permission-mode", "bypass_permissions",
    "--output-format", "stream-json",
    { "when": "model", "args": ["--model", "{model}"] },
    { "when": "reasoningEffort", "args": ["--reasoning-effort", "{reasoningEffort}"] },
    "--append-system-prompt", "{prompt.system}",
    "-p", "{prompt.user}"
  ],
  "stdout": { "format": "builtin", "name": "claude_stream_json" },
  "session": {
    "idSource": "provider_capture",
    "scope": "actor_scope",
    "resumeArgs": [
      "--add-dir", "{loom.configDir}",
      "--permission-mode", "bypass_permissions",
      "--output-format", "stream-json",
      { "when": "model", "args": ["--model", "{model}"] },
      { "when": "reasoningEffort", "args": ["--reasoning-effort", "{reasoningEffort}"] },
      "--append-system-prompt", "{prompt.system}",
      "--resume", "{session.id}",
      "-p", "{prompt.user}"
    ]
  }
}
```

Qoder session 直接按 `provider_capture + --resume` 接入：首轮从 stream-json 输出捕获
Provider 生成的 session id，后续 turn 用 `--resume {session.id}`。这里
`claude_stream_json` decoder 需要在看到 `session_id` 时发出
`ProviderRuntimeEvent::Session`。

### GitHub Copilot CLI

Copilot CLI 支持 `-p/--prompt`、`--output-format json`、`--stream off`、`--yolo`、
`--add-dir`、`--model`、`--effort/--reasoning-effort` 和 `--resume`。help 中没有
system prompt 参数，因此应把 Loom system + turn 合成 `{prompt.full}` 传给 `-p`。
Session 建议用 Loom 生成的稳定 UUID，通过 `--resume {session.id}` 传给 Copilot；
如果该 id 不存在则创建 session，存在则 resume。

建议 print mode：

```json
{
  "prompt": {
    "outputs": {
      "full": { "preset": "loom_full" }
    }
  },
  "args": [
    "--add-dir", "{loom.configDir}",
    "--yolo",
    "--output-format", "json",
    "--stream", "off",
    "--resume", "{session.id}",
    { "when": "model", "args": ["--model", "{model}"] },
    { "when": "reasoningEffort", "args": ["--effort", "{reasoningEffort}"] },
    "-p", "{prompt.full}"
  ],
  "stdout": { "format": "builtin", "name": "copilot_jsonl_final_text" },
  "session": {
    "idSource": "loom_uuid",
    "scope": "actor_scope"
  }
}
```

这里的 parser 不能简单把所有 `assistant.message` 发成 text，否则会把中间状态和
最终回答一起暴露。`copilot_jsonl_final_text` 可以先作为 builtin parser 落地，
但语义必须等价于上一节的 finalText reducer；后续可直接展开成 manifest reducer。

### Codex CLI

Codex CLI 的非交互入口是 `codex exec [PROMPT]`。它支持 `--json` JSONL 输出、
`--sandbox danger-full-access`、`--add-dir`、`--model`、`-C/--cd`，也支持 prompt
参数为空时从 stdin 读取。Codex 没有专用 system prompt 参数，因此第一版应使用
`{prompt.full}`。现有 Loom 还为 Codex 设置 `LOOM_NO_DAEMON=1`，让 agent 内部调用
`loom` CLI 时走 websocket server，而不是访问 macOS sandbox 可能拦截的 daemon
Unix socket；这个行为必须进入 manifest/env。

建议 print mode：

```json
{
  "prompt": {
    "outputs": {
      "full": { "preset": "loom_full" }
    }
  },
  "env": {
    "LOOM_NO_DAEMON": "1"
  },
  "args": [
    "exec",
    "--skip-git-repo-check",
    "--json",
    "--sandbox", "danger-full-access",
    "-c", "sandbox_workspace_write.network_access=true",
    "--add-dir", "{loom.configDir}",
    { "when": "model", "args": ["--model", "{model}"] },
    "{prompt.full}"
  ],
  "stdout": { "format": "builtin", "name": "codex_stream_json" },
  "session": {
    "idSource": "provider_capture",
    "scope": "actor_scope",
    "resumeArgs": [
      "exec",
      "resume",
      "{session.id}",
      "--skip-git-repo-check",
      "--json",
      "--sandbox", "danger-full-access",
      "-c", "sandbox_workspace_write.network_access=true",
      "--add-dir", "{loom.configDir}",
      { "when": "model", "args": ["--model", "{model}"] },
      "{prompt.full}"
    ]
  }
}
```

Codex 输出历史形态较多，短期用 builtin parser alias 更稳；中期可以把已知
`task_complete`、`agent_message`、`item.completed`、`agent_message_content_delta`
规则逐步迁移成 manifest reducer。Codex session 直接按 `provider_capture + resume`
接入：首轮从 JSONL 输出捕获 Provider 生成的 session id，后续 turn 用
`codex exec resume {session.id}`。这里 `codex_stream_json` decoder 需要在看到 session
id 时发出 `ProviderRuntimeEvent::Session`。

### OpenCode

OpenCode 的非交互入口是 `opencode run [message..]`。当前 CLI help 显示它支持
`--format default|json`、`--session`、`--continue`、`--model`、`--variant`、
`--agent` 和 `--dangerously-skip-permissions`。没有看到等价于
`--system-prompt` / `--append-system-prompt` 的 run 参数，因此第一版应使用
`{prompt.full}`，让 Loom system、agent instructions、runtime context 和 turn message
按 prompt parts 组合后作为 message 传入。

OpenCode session 直接按 `provider_capture + --session` 接入：首轮从 JSON 输出捕获
Provider 生成的 `sessionID`，后续 turn 用 `--session {session.id}`。为了让每个 Loom
agent 的运行期状态彼此隔离，manifest 应设置 `XDG_DATA_HOME`、`XDG_STATE_HOME` 和
`XDG_CACHE_HOME` 到 `{agent.profile}/opencode/...`。`opencode debug paths` 显示
config 仍默认来自用户级 `~/.config/opencode`；这部分暂时保留给 OpenCode 凭据和全局
配置，不写入 AgentSpec。本机 `opencode models` 当前返回
`opencode/big-pickle`、`opencode/deepseek-v4-flash-free`、`opencode/mimo-v2.5-free`
和 `opencode/nemotron-3-super-free`；内置 manifest 不应默认写不存在的 OpenAI
模型 id。

建议 print mode：

```json
{
  "prompt": {
    "outputs": {
      "full": { "preset": "loom_full" }
    }
  },
  "env": {
    "XDG_DATA_HOME": "{agent.profile}/opencode/data",
    "XDG_STATE_HOME": "{agent.profile}/opencode/state",
    "XDG_CACHE_HOME": "{agent.profile}/opencode/cache"
  },
  "args": [
    "run",
    "--dangerously-skip-permissions",
    "--format", "json",
    { "when": "model", "args": ["--model", "{model}"] },
    { "when": "reasoningEffort", "args": ["--variant", "{reasoningEffort}"] },
    "{prompt.full}"
  ],
  "stdout": {
    "format": "jsonl",
    "reduce": {
      "finalText": {
        "mode": "lastNonEmpty",
        "path": "$.part.text",
        "when": { "path": "$.type", "equals": "text" }
      }
    },
    "capture": {
      "session": { "mode": "lastNonEmpty", "path": "$.sessionID" }
    }
  },
  "session": {
    "idSource": "provider_capture",
    "scope": "actor_scope",
    "resumeArgs": [
      "run",
      "--dangerously-skip-permissions",
      "--format", "json",
      "--session", "{session.id}",
      { "when": "model", "args": ["--model", "{model}"] },
      { "when": "reasoningEffort", "args": ["--variant", "{reasoningEffort}"] },
      "{prompt.full}"
    ]
  }
}
```

## Runtime 架构

建议分层：

```text
ProviderManifest
  -> ProviderRuntimePlan
    -> CommandInvocation
    -> OutputDecoder
      -> ProviderRuntimeEvent
        -> AdapterEvent / session manager
```

`ProviderManifest` 是配置，应保持可序列化、稳定。

`ProviderRuntimePlan` 是检测、模型选择、scope 解析、模板展开之后的 resolved form。
它包含具体 argv/env/stdin 和编译后的 decoder，不包含 `PromptVia`、`modelArgs`、
`CommandOutputFormat` 这类旧 command transport 控制字段；如果短期还需要生成
`AgentTransport`，这些字段只能在最后一跳由 manifest 信息派生。

`CommandInvocation` 只负责启动和 supervise 进程，不再知道 Claude、Copilot、
Codex 或 Qoder。

`OutputDecoder` 消费 stdout/stderr，产生 `ProviderRuntimeEvent`。其中可见执行事件
映射成现有 `AdapterEvent`，继续复用 run trace 和可选 auto-publish pipeline；session
捕获事件只更新 runtime session store。Provider decoder 一旦存在，行级事件解析就只
看 decoder 的 `events` 或 `builtin name`，不能再隐式回退到旧
`CommandOutputFormat` parser；旧 parser 只服务没有 Provider decoder 的 command
transport。

## AgentSpec 方向

AgentSpec 不复制 transport。GUI 请求创建 agent 时只提交 actor metadata、instructions、
providerRef、model 等声明；真正写入由目标 daemon 完成：

```json
{
  "actor": {
    "id": "actor_joi",
    "displayName": "Joi"
  },
  "instructions": "Reply concisely and report completed work.",
  "providerRef": {
    "id": "claude",
    "mode": "print",
    "model": "sonnet"
  },
  "autostart": true
}
```

daemon 在启动时把 `providerRef` resolve 成具体 runtime plan。手写高级配置也不在
AgentSpec 里复制 command/args/parser；如果需要改 Provider 行为，应在目标 daemon
的 provider registry 里创建一个本地 Provider variant，然后让 AgentSpec 引用新的
provider id。

## 与 AgentSpec / `spec.json` 的边界

Agent 自己的 `spec.json` 位于目标 daemon 管辖的
`{loom.configDir}/agents/<actor_id>/spec.json`。这个文件描述的是一个具体 agent
actor；Provider manifest 描述的是一类 CLI/runtime 如何接入 Loom。两者不能继续混在
一起。server 只能保存 daemon inventory 快照，GUI 只能展示快照或发 command；任何
GUI/server 侧副本都不能参与运行时 resolve。

职责边界：

```text
ProviderManifest
  描述“怎么启动和解析某类 Provider”
  例如 Claude Code / Codex / Qoder / Copilot 的 detect、command、args/env/stdin、
  prompt parts 映射、stdout/stderr parser、session 策略、provider 级模型菜单。

AgentSpec / spec.json
  描述“这个 agent actor 是谁、如何被触发、选哪个 provider”
  例如 actor id/displayName、metadata、autostart、trigger、promptTemplate、
  actor 选中的 model/reasoningEffort，以及 providerRef。
```

容易重复的字段和归属建议：

| 字段 | 归属 | 说明 |
| --- | --- | --- |
| `command` / `args` / `env` / `stdin` | ProviderManifest | 默认不应该出现在每个 agent 的 `spec.json` 里。 |
| stdout/stderr parser | ProviderManifest | 解析方式是 Provider 能力，不是 actor 个性。 |
| session 策略 | ProviderManifest | `loom_uuid`、`provider_capture`、resumeArgs 都是 Provider 接入规则。 |
| model choices | ProviderManifest | Provider 给 UI 默认菜单；agent 只记录选中的 model。 |
| selected model / reasoning effort | AgentSpec 或运行时选择 | 这是具体 agent 的偏好，Provider 只定义如何映射到 argv/env。 |
| prompt parts 如何进 system/user/full | ProviderManifest | 这是 Provider 接入形态。 |
| `prompt.workspaceFiles` 声明 | ProviderManifest | Provider 声明它支持读取当前 agent workspace `.loom` 下哪些文件，并把它们命名为 `workspace_file.*` parts。 |
| `.loom/<path>` prompt 文件内容 | agent workspace | 文件内容属于当前 agent workspace，由 daemon 在运行时读取；GUI/server 不保存副本，AgentSpec 不复制内容。 |
| instructions | AgentSpec | 这是具体 agent 的静态行为说明，进入 `agent_instructions` prompt part，并由 ProviderManifest 决定是否落到 system prompt。 |
| promptTemplate / trigger_prefix | AgentSpec | 这是具体 agent 的触发语义和任务包装。 |
| metadata / description | AgentSpec | 这是 actor 的 UI 与调度元信息，不参与 Provider 启动规则。 |
| memory policy | AgentSpec | 这是具体 agent 是否注入 memory 的策略；memory 内容和索引属于 daemon profile/dataRoot。 |
| autostart / avatar / displayName | AgentSpec | 这是 actor 生命周期和 UI 信息。 |

因此新设计下，`spec.json` 的推荐形态是引用 provider，而不是复制 transport：

```json
{
  "actor": {
    "id": "actor_agent_reviewer",
    "kind": "agent",
    "displayName": "Reviewer",
    "_meta": {
      "description": "Reviews code changes"
    }
  },
  "instructions": "Reviews code changes and replies with concise findings.",
  "providerRef": {
    "id": "claude",
    "mode": "print",
    "model": "sonnet",
    "reasoningEffort": "high"
  },
  "autostart": true,
  "memory": {
    "delivery": {
      "prompt": true
    }
  }
}
```

最终 AgentSpec 不包含 raw `transport`。如果某个 agent 需要特殊启动参数、不同 parser
或不同 session 策略，应创建一个本地 Provider variant，再让 `providerRef.id` 指向该
variant。这样“运行适配规则”始终只存在于 ProviderManifest。

## 统一最终配置模型

最终应收敛成清晰的 ownership，而不是让 GUI、server、daemon 各自维护一份 agent 定义。

三层职责：

```text
GUI
  人在哪。保存人类客户端偏好、当前 server 选择和窗口/UI 状态；只读 server 上的
  daemon inventory。创建/更新/删除 agent/provider 时发 machine command，不直接读写
  daemon 的 agent/provider/profile/memory 文件。

server
  沟通数据存在哪。保存 message/task/run trace、daemon 连接状态、daemon 发布的
  inventory 快照，以及 machine command 队列/结果；不解释 provider，不生成 AgentSpec，
  也不把 inventory 快照反写成配置。

daemon
  agent 跑在哪。拥有目标 host 的 provider registry、AgentSpec、profile、session/runtime
  state，并负责校验、写盘、启动、停止、解析 Provider。
```

daemon 内部应收敛成三类事实源：

```text
{loom.configDir}/providers/<provider_id>.json
  ProviderManifest。描述 Provider 接入规则，可被多个 agent 复用。

{loom.configDir}/agents/<actor_id>/spec.json
  AgentSpec。描述具体 agent actor，引用 providerRef。

{loom.dataRoot}/runtime/...
  Runtime state。保存进程状态、session id、最近 run、临时缓存等，不进入配置。

{paths.cwd}/.loom/<provider-declared-path>
  当前 agent workspace 内的可选 prompt 文件。ProviderManifest 只声明可读取的文件槽位；
  内容本身随 workspace 走，不进入 AgentSpec、ProviderManifest、server inventory 或 GUI
  local state。
```

resolve 顺序应固定，避免多处配置互相覆盖：

```text
built-in ProviderManifest
  -> local ProviderManifest / extends patch
  -> AgentSpec.providerRef 选择 provider + mode + model
  -> runtime variables 展开成 ProviderRuntimePlan
  -> runtime state 记录 session/run/process 状态
```

GUI local state 与 daemon machine config 必须是不同文件和不同事实源：

```text
GUI local state
  GUI 的 desktop.toml。
  当前选中的 server、最近连接列表、窗口状态、人类 actor 选择等 UI/客户端信息。
  不包含 agents/providers/profile/memory。

daemon machine config
  daemon 的 daemon.toml。
  daemon 上线所需的 server URL、machine id、dataRoot/configDir、认证信息和本机能力。
  不包含 AgentSpec 列表，也不包含 ProviderManifest override 列表。
```

GUI 创建或编辑 agent 时不写本地 agent 文件；它向 server 发 machine command，server
转发给目标 daemon，由 daemon 写入自己的 `agents/<actor_id>/spec.json` 并刷新
inventory。即使目标是本机 daemon，也应走同一条 server machine command 路径；
daemon offline 时 GUI 只能显示 offline/不可编辑或排队 command，不能自己落盘一个
“临时 agent”。

标准数据流应是单向闭环：

```text
GUI intent
  -> server machine command queue
  -> target daemon validates and writes ProviderManifest/AgentSpec/runtime state
  -> daemon publishes inventory
  -> server stores latest inventory snapshot
  -> GUI refreshes from server snapshot
```

这个闭环里没有 GUI 本地 merge，也没有 server 根据 providerRef 生成 spec 的步骤。

统一后的职责：

| 当前位置 | 问题 | 最终归宿 |
| --- | --- | --- |
| `AgentProviderSpec.provider + transport + actors[]` | provider 接入规则和多个 actor 混在一起 | 拆成 ProviderManifest + 多个 AgentSpec |
| `AgentSpec.transport` | 每个 agent 复制 command/args/parser/session | 删除；改为 `providerRef` 指向 ProviderManifest |
| `MachineConfig.providers[]` / `AgentProviderOverride` | GUI/server 可能把 provider override 当成跨端事实源 | 改为 daemon-local ProviderManifest，必要时用 `extends` 或 custom provider id |
| `MachineConfig.agents[]` / `MachineAgentConfig` | GUI agent 信息和 daemon AgentSpec 重复 | 不再作为运行时输入；历史配置如需保留，应显式导入成 daemon-owned AgentSpec |
| `AgentDefinition` | MachineAgentConfig 到 AgentSpec 的中间展开结构 | 删除；daemon command 直接生成/更新 AgentSpec |
| runtime session id | 容易被误写入 spec | 写入 runtime state，由 session 策略生成或从输出捕获 |

### 高级手写配置

高级手写配置仍然写 ProviderManifest，不写在 AgentSpec 里。做法是定义一个本地
Provider variant，用 `extends` 继承已有 Provider，再覆盖 mode：

```json
{
  "schemaVersion": 1,
  "id": "claude_review_budgeted",
  "displayName": "Claude Code Review Budgeted",
  "extends": "claude",
  "modes": {
    "print": {
      "timeoutMs": 900000,
      "env": {
        "merge": {
          "EXTRA_FLAG": "1"
        }
      },
      "args": {
        "append": ["--max-budget-usd", "5"]
      }
    }
  }
}
```

`extends` 下的 `modes.<name>` 使用 Provider mode 的字段名，但语义是 patch，不是完整
定义：

| patch 字段 | 合并规则 |
| --- | --- |
| `timeoutMs` / `idleTimeoutMs` | 标量覆盖 |
| `env.merge` | 合并到 provider env；同名 key 覆盖 |
| `env.unset` | 从 provider env 删除指定 key |
| `args.append` / `args.prepend` | 在 provider args 前后追加；元素使用同一套 argv item schema，适合安全小改动 |
| `args.replace` | 整体替换 args；元素使用同一套 argv item schema |
| `prompt.outputs` | 按 output name 替换或新增；不影响未提到的输出 |
| `prompt.workspaceFiles` | 整体替换；避免 base provider 和 variant 对同一个 `workspace_file.<key>` 产生冲突 |
| `stdin` | 覆盖 stdin 模板 |
| `stdout` / `stderr` parser | 整体替换 |
| `session` | 整体替换 |

对应的 AgentSpec 仍然只是引用 provider：

```json
{
  "actor": {
    "id": "actor_agent_reviewer",
    "kind": "agent",
    "displayName": "Reviewer"
  },
  "providerRef": {
    "id": "claude_review_budgeted",
    "mode": "print",
    "model": "sonnet",
    "reasoningEffort": "high"
  }
}
```

规则是：**运行适配差异和 workspace prompt 文件槽位声明进 ProviderManifest；单个
actor 的 instructions、metadata、记忆策略、触发和模型偏好进 AgentSpec；workspace
`.loom` prompt 文件内容跟随 agent workspace；短期进程状态进 runtime state。**

## 安全与校验

Provider manifest 使用前必须校验：

- `id` 必须稳定、小写、唯一。
- schema 必须严格；未知字段必须失败。这个规则同样适用于 `extends` patch 的
  顶层、mode、args/env/prompt patch 结构，避免 typo 被 merge 阶段静默吞掉。
- `command` 必须来自 `detect.candidates` 的解析结果，或者是显式路径。
- 模板只能引用已知变量，除非开启显式 allow unknown。
- `prompt.workspaceFiles` 只能声明 `.loom` 下的相对路径；禁止绝对路径、`..`、
  空路径、控制字符和平台相关路径逃逸。`key` 必须稳定、小写、唯一，并且只能生成
  `workspace_file.<key>` 这一类 prompt part。
- workspace prompt 文件必须限制大小，第一版建议每个文件默认上限 32 KiB；超限应失败
  或按 manifest 明确声明的策略截断，不能静默把大文件塞进 prompt。
- command mode 下至少一个 `{prompt.<name>}` 必须出现在
  args/stdin/env 中；如果同一个组合输出被多处引用，必须显式声明允许重复发送。
  ACP mode 例外。
- 对需要访问 Loom config 的 sandbox CLI，应建议或要求
  `--add-dir {loom.configDir}`。
- parser 每行/每个文档的提取工作必须有边界。
- regex parser 要限制输入大小，避免灾难性回溯。

对于本地自定义 Provider，GUI 应展示它是 custom provider，以及最终会执行哪个
命令。

## `loom provider` 管理命令

`loom provider` 应成为 Provider lifecycle 的 CLI 入口，而不只是一个校验工具。它
负责本地自定义 Provider 的校验、录入、查看、删除和诊断；GUI 可以复用同一套
backend API。

建议第一版命令：

```bash
loom provider validate <file>
```

校验一个 manifest 是否能被 Loom 正确加载。校验应覆盖 schema、模板变量、
prompt 输出引用、workspace prompt 文件声明、session 配置、parser 配置、命令检测和
路径安全。

```bash
loom provider add <file>
```

把一个自定义 Provider 录入当前 `{loom.configDir}/providers/`。`add` 必须先跑
`validate`；如果 `id` 已存在，默认失败，除非显式传 `--replace`。内置 Provider
不能被删除。本地 Provider 如果要基于内置 Provider 改参数，应优先使用新 id +
`extends`；不允许用本地 manifest 覆盖内置 Provider id。

```bash
loom provider list
```

列出当前可用 Provider，包括 built-in 和 local custom。输出至少包含 id、
displayName、source、detect 状态、默认 mode 和默认模型。

```bash
loom provider show <provider_id>
```

展示 Provider 的最终 resolved 配置：来源、检测到的二进制、可用 modes、默认 mode、
models、prompt outputs、argv/env/stdin 模板、stdout/stderr parser、session 策略。
这里展示的是 resolve 后的结果，方便配置者确认 Loom 实际会怎么启动 Provider。

```bash
loom provider remove <provider_id>
```

删除本地自定义 Provider。内置 Provider 不允许删除；后续可以支持 disable/enable，
但第一版可以先不做。

```bash
loom provider doctor <provider_id>
```

对已安装 Provider 做诊断：检查 CLI 是否存在、版本/help 是否符合 manifest 预期、
参数模板能否展开、`{session.id}` 策略是否完整、workspace prompt 文件声明是否可读且
不会逃逸 `.loom`、stdout parser 是否能用样本解析。如果 manifest 提供 sample output，
doctor 应直接跑 parser；如果没有 sample，可以只做静态检查和可执行文件检测，避免
误触发真实模型调用。

命令写入范围应遵守目标 daemon 的 `{loom.configDir}`。GUI 对接多个 server/machine 时，
应通过 server machine command 触达目标 daemon 的 provider 管理能力；不同 daemon 的
local Provider 互不污染。`loom provider list` 在 daemon/CLI 本机执行时只看当前进程
解析出的 `{loom.configDir}`。

## Loom 顶层影响

这个设计会改变 Loom 顶层配置边界，属于有意的架构收敛：

- daemon 是 agent 运行与定义归属。GUI 不直接读写 daemon 私有文件；创建、更新、删除
  agent 都通过 server machine command 到达目标 daemon，由 daemon 修改自己的
  `agents/<actor_id>/spec.json` 并刷新 inventory。面向本机 daemon 的 CLI 管理命令可以
  操作当前 daemon config，但这不是 GUI 的数据路径。
- server 保存 daemon 发布的 inventory 快照和 machine command 队列/结果；不解释
  ProviderManifest，也不自行生成 AgentSpec。
- GUI local state 与 daemon machine config 不保存 agents/providers。GUI 的
  `desktop.toml` 只承载连接选择和人类客户端偏好；daemon 的 `daemon.toml` 只承载
  server URL、machine id、dataRoot/configDir 等宿主信息。两者不能复用同一个文件或
  让 GUI 把 `machines[].agents[]`、provider override 当成可编辑事实源。
- Provider command/args/env/parser/session 不写在 AgentSpec 里；这些进入目标 daemon 的
  ProviderManifest。host-specific 差异用 daemon-local provider variant 表达。
- Provider 支持的 workspace prompt 文件只在 ProviderManifest 里声明槽位；具体
  `.loom/<path>` 文件内容属于当前 agent workspace，server/GUI 不保存、不合并、不反写。
- 运行期 session id、进程状态和 run 缓存只写 runtime state，不写任何 spec。
- run trace 和 GUI 可见 message 的边界不变：Provider stdout 进入 run trace；GUI
  可见消息仍来自 agent 显式 `loom message send` 或 Loom 的 auto-publish 策略。
- ServiceSpec 不参与 ProviderManifest 体系。service 插件仍由 service spec 管理，
  避免 agent provider 设计污染服务进程模型。

## 落地顺序

1. 先固定 ownership 边界：daemon inventory 是 GUI/server 看到 agent/provider 的唯一来源；
   GUI 创建/更新/删除 agent/provider 只发 server machine command；server 只转发 command
   和保存 inventory 快照；daemon 是唯一写盘方。
2. 删除正常运行路径里对 `MachineConfig.providers[]`、`MachineConfig.agents[]`、
   `AgentProviderSpec.provider + transport + actors[]`、`AgentSpec.transport` 的依赖。
   如必须承接历史安装，只提供显式离线导入命令，不做 runtime 双读或 merge。
3. 增加最终版 AgentSpec `providerRef`，daemon 创建/编辑 agent 时写入自己的
   `{loom.configDir}/agents/<actor_id>/spec.json`；GUI 只发 machine command 并等待
   daemon inventory 刷新。
4. 增加 `ProviderManifest`、`ProviderMode`、`ProviderModePatch`、
   `ProviderRuntimePlan` 类型，以及校验测试。
5. 把内置 Claude/Qoder/Copilot/Codex Provider 编码成 manifest resource，并让 discovery
   从 manifest resolve provider，而不是 match provider id。
6. 实现 prompt parts composer 和 `prompt.outputs` 渲染，输出 `{prompt.system}`、
   `{prompt.user}`、`{prompt.full}` 等命名 prompt。
7. 实现 `prompt.workspaceFiles`：按 ProviderManifest 声明读取当前 agent workspace
   `.loom` 下的文件，生成 `workspace_file.*` prompt parts，再进入同一套
   `prompt.outputs` 渲染链路。
8. 实现 argv/env/stdin 模板展开、条件 args、`loom_uuid` / `provider_capture` session
   策略，并让 decoder 产出的 `ProviderRuntimeEvent::Session` 写入 runtime session
   store。
9. 实现 manifest-driven stdout/stderr decoder；复杂协议先通过 manifest 引用
   `builtin` decoder，后续可逐步改写成 JSONL reducer。
10. 增加 `loom provider validate/add/list/show/remove/doctor`。这些命令在 daemon/CLI
   本机操作当前 `{loom.configDir}`；GUI 如需管理远端 Provider，必须通过目标 daemon
   的 server machine command。

新 runtime 不做新旧配置双读，也不维护旧配置到新配置的运行时覆盖优先级。对单台
daemon 而言，ProviderManifest、AgentSpec 和 runtime state 才是唯一生效边界；
GUI/server 只消费 daemon inventory。

## 待定问题

- OpenCode 已按当前 CLI help 接入 JSON 输出和 `provider_capture` session；仍需要在
  后续真实运行样本中确认 JSON 事件字段是否稳定，尤其是 `sessionID` 和最终文本路径。
- 复杂 Provider 是否需要 JavaScript/WASM parser hook，还是小型 JSONPath/regex DSL
  足够？
- models 应该静态写在 manifest、通过 provider command 动态发现，还是两者都支持并
  使用 cache fallback？
- workspace prompt 文件是否需要第一版就支持受限 glob，还是先只支持显式文件路径？
- `interactive_command` 和 command `print` mode 是否现在就合并到同一 manifest
  schema，还是等 print mode 稳定后再处理 interactive？
