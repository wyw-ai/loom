# ContextLayer — 配置参考

> `agentcontext.json` 和 `spec.json` 中 `context_layer` 字段的完整 schema。
> 概念说明参见 [架构 Wiki](./architecture.md)。

---

## 文件位置

ContextLayer 配置可放在三个位置，按优先级顺序合并（上层对相同 `scheme` 覆盖下层）：

| 层级 | 文件路径 | 用途 |
|---|---|---|
| Agent 级 | `<profile_dir>/spec.json` → `context_layer` 字段 | 基础配置，通过 `loom agent edit` 设置 |
| Profile 级 | `<profile_dir>/agentcontext.json` | 按 Agent 覆盖 |
| 工作区级 | `<profile_dir>/workspace/agentcontext.json` | 按作用域覆盖 |

> **注意**：运行时使用 `agentcontext.json`（不是 `.yml`）。Loom 不依赖
> YAML 解析器。ARCH 设计中概念性地引用了 `agentcontext.yml`；运行时
> 使用 JSON 实现相同目的。

---

## 顶层 Schema

### AgentContextSpec

`agentcontext.json` 中的顶层对象，也是 `spec.json` 中 `context_layer` 字段的类型：

```json
{
  "version": 1,
  "effective_scope": ["thread", "channel"],
  "resources": [ ... ]
}
```

| 字段 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `version` | `u32` | `1` | Schema 版本。合并时取较大值。 |
| `effective_scope` | `Vec<ContextScopeKind>` | `[]`（所有作用域） | 此配置适用的作用域类型。空 = 全部。 |
| `resources` | `Vec<ContextResourceSpec>` | `[]` | 资源声明。 |

### ContextScopeKind

```json
"thread" | "channel"
```

序列化为小写字符串。控制作用域级过滤：`effective_scope: ["thread"]` 的资源
在 Channel 作用域中会被跳过。

---

## 资源声明

### ContextResourceSpec

`resources` 数组中的每个条目：

```json
{
  "scheme": "file",
  "mount": "workspace-docs",
  "priority": 20,
  "config": {
    "path": "${workspace.dir}/docs",
    "max_files": 10
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `scheme` | `String` | 是 | 标识 Provider 的 URI scheme。可选值：`"memory"`、`"message-list"`、`"file"`、`"warm-summary"`（`"skill"` 设计中但未实现）。 |
| `mount` | `Option<String>` | 否 | 挂载标签（信息性；标识此资源声明）。省略时：覆盖已有资源则继承下层值；新资源则缺省为 `scheme` 本身。 |
| `priority` | `Option<i32>` | 否 | 组装顺序。值越小 = 越先组装。`0` = 永不跳过（保留值）。省略时：覆盖已有资源则继承下层值；新资源则缺省为 `100`。显式声明的 `priority` 会覆盖资源内置优先级并真实作用于装配顺序（迭代 2 起；此前显式值若与内置值不同会被静默忽略）。 |
| `config` | `Option<serde_json::Value>` | 否 | Provider 专属配置。参见下文各 scheme 说明。 |

> **行为变更（迭代 1 / R2）**：此前 `priority` 省略时按 `0` 处理（即"永不跳过"的
> 保留值，资源被静默豁免于预算瀑布）。现在省略 `priority` 表示"继承"：覆盖已有
> 资源时继承下层值，新资源时取统一缺省值 `100`。显式声明 `"priority": 0` 仍表示
> 保留语义（永不跳过），不受影响。

---

## 各 Scheme 配置

### `memory` scheme

官方记忆插件（独立 `plugin-memory` crate; 迭代 2 起以官方插件形态接入，迭代 3 R1
整改起完全规范化——inventory 注册 + `plugin.json` v2 清单，`plugin list` 显示
`official` / v1.0.0）。插件从 config envelope 的 `memory` key 读取配置: actor 的
`MemorySpec`（来自 `spec.json`）由宿主在链构造前注入 envelope（per-field 合并、
actor 侧胜出），在链装配阶段按当前回合输入（`AssemblyContext.turn_input`）检索并
渲染记忆段落。

```json
{
  "scheme": "memory",
  "mount": "memory",
  "priority": 5
}
```

**清单声明优先级**：5

**行为**：生成 `bootstrap_memory` 和 `turn_memory` 段落。如果 Agent 未配置
`MemorySpec` 或 `delivery.prompt=false`，两个段落都为空，插件不贡献任何内容；
检索失败时降级为空输出（跳过而非中断）。

**`config` 字段**：经 config envelope 的 `memory` key 读取 `MemorySpec` 子集
（`config_schema` 声明）。常规配置走 `spec.json` 的 actor 级 `memory` 字段（宿主
注入、actor 侧胜出）; envelope 缺失或 malformed 时 warn 并回退默认。

**禁用 / 覆盖 / 定制**（迭代 2 语义）：

- **禁用**：从有效 `resources` 列表中移除 `memory` 条目即可（逐字段合并无删除
  语义，需在最高层用不含 memory 的完整 `resources` 数组覆盖）：

  ```json
  { "resources": [
      { "scheme": "warm-summary", "priority": 7 },
      { "scheme": "message-list", "priority": 10 }
  ] }
  ```

- **覆盖**：在更高层级声明同 `scheme` 条目并显式指定 `priority`（或 `mount`）。
  显式 `priority` 会真实作用于装配顺序（见下文行为变更说明）：

  ```json
  { "resources": [ { "scheme": "memory", "priority": 25 } ] }
  ```

- **定制**：`config` 字段会传入资源工厂；记忆插件当前忽略它（业务配置以
  `spec.json` 的 `MemorySpec` 为准），第三方替身插件可用自身 `config`。

### `message-list` scheme

将投递上下文（Thread 消息、收件箱条目）包装为提示词段落。

```json
{
  "scheme": "message-list",
  "mount": "messages",
  "priority": 10
}
```

**内置默认优先级**：10

**行为**：生成单个 `delivery_context` 段落，包含本轮的 Thread/Channel 消息历史。

### `file` scheme

将工作区文件挂载为上下文资源。

```json
{
  "scheme": "file",
  "mount": "workspace-docs",
  "priority": 20,
  "config": {
    "path": "${workspace.dir}/docs",
    "max_files": 10
  }
}
```

**内置默认优先级**：20

**配置字段**：

| 键 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `path` | `String` | `"${workspace.dir}"` | 挂载目录。支持 `${workspace.dir}` 模板。绝对路径直接使用；相对路径基于工作区目录解析。 |
| `max_files` | `u64` | `10` | 枚举和读取的最大文件数。 |

**限制**：
- 每文件读取限制：128KB（`MAX_READ_BYTES`）
- 路径遍历保护：canonicalize 检查防止 `../` 逃逸
- 支持的媒体类型：`.md`、`.txt`、`.json`、`.yaml`/`.yml`、`.toml`、`.rs`、`.py`、`.js`/`.ts`（其他类型以 `application/octet-stream` 读取）

### `warm-summary` scheme

通过 D2 ContextResource 链注入 Warm 摘要。读取持久化的摘要文件
（`{profile_dir}/summaries/{scope_id}.md`），预算限制为剩余 Token 的 20%，
超预算时跳过（不截断，C-3）。

```json
{
  "scheme": "warm-summary",
  "mount": "warm",
  "priority": 7
}
```

**行为**：`WarmSummaryContextResource`（优先级 7）在 D2 chain 中自动组装。
摘要内容由 Provider 生成并持久化（`persist()`），此资源只负责读取和注入。
`default_agent_context_spec()` 默认包含 warm-summary，无需手动配置。

### `skill` scheme（未实现）

> ⚠️ `scheme="skill"` 的 `SkillContextResource` 在设计文档中规划过，但**当前
> 代码中未实现**。以下配置仅供参考，实际不会产生效果。

设计中的配置：

```json
{
  "scheme": "skill",
  "mount": "workspace-skills",
  "priority": 8,
  "config": {
    "read_instruction": true,
    "max_skills": 5,
    "instruction_file": "SKILL.md"
  }
}
```

**当前实际路径**：Skill 通过 `bundle.skills`（在 `spec.json` 中配置）安装并
投影到 workspace 目录，由 Provider 原生 skill 系统读取。不经过 ContextResource
chain，不消耗 Loom token budget。详见
[Skill 挂载指南](./skill-mounting-guide.md)。

---

## 合并语义

合并按 **逐字段（per-field）** 进行，而不是整资源覆盖。合并顺序为
spec.json → profile → 工作区（后者覆盖前者）。

当相同 `scheme` 出现在多个层级时，`mount` / `priority` / `config` 三个字段
各自独立合并：**上层显式声明的字段（`Some`）覆盖下层；上层省略的字段（`None`）
继承下层**。

```
层级 1（spec.json）：     scheme="memory", mount="memory",  priority=5
层级 2（profile）：       scheme="memory", priority=3        （mount 省略）

结果：scheme="memory", mount="memory"（继承层级 1）, priority=3（层级 2 覆盖）
```

当新 `scheme` 出现在更高层级时，该资源作为新条目追加，省略的字段取统一缺省值：

```
层级 1（spec.json）：     scheme="memory"
层级 2（profile）：       scheme="memory" + scheme="file"（mount/priority 省略）

结果：memory（逐字段合并）+ file（mount 缺省为 "file"，priority 缺省为 100）
```

> **行为变更（迭代 1 / R2）**：此前相同 `scheme` 的合并是整资源覆盖，且省略
> `priority` 会被反序列化为 `0`（永不跳过的保留值）。现在改为逐字段合并：
> 省略即继承，新资源缺省 `priority=100`、`mount=scheme`。显式 `"priority": 0`
> 仍按保留值处理。

**`effective_scope` 合并**：指定了非空 `effective_scope` 的最高层级优先。
如果所有层级的 `effective_scope` 都为空，则对所有作用域生效。

**`version` 合并**：`max(base.version, overlay.version)`。

---

## 完整示例

包含所有内置 Provider 的完整 `agentcontext.json`：

```json
{
  "version": 1,
  "effective_scope": ["thread", "channel"],
  "resources": [
    {
      "scheme": "memory",
      "mount": "memory",
      "priority": 5
    },
    {
      "scheme": "message-list",
      "mount": "messages",
      "priority": 10
    },
    {
      "scheme": "file",
      "mount": "workspace-docs",
      "priority": 20,
      "config": {
        "path": "${workspace.dir}/docs",
        "max_files": 15
      }
    },
    {
      "scheme": "warm-summary",
      "mount": "warm",
      "priority": 7
    }
  ]
}
```

---

## 在 spec.json 中配置 ContextLayer

D2 ContextResource chain 始终活跃。当 `context_layer` 为 `null` 或不存在时，
自动使用 `default_agent_context_spec()` 默认 chain（memory + warm-summary + message-list）。

如需自定义 chain（如添加 file、skill 资源），在 `spec.json` 中设置 `context_layer`：

```json
{
  "id": "my-agent",
  "provider": { ... },
  "context_layer": {
    "version": 1,
    "effective_scope": ["thread", "channel"],
    "resources": [
      {
        "scheme": "memory",
        "mount": "memory",
        "priority": 5
      },
      {
        "scheme": "warm-summary",
        "mount": "warm",
        "priority": 7
      },
      {
        "scheme": "message-list",
        "mount": "messages",
        "priority": 10
      },
      {
        "scheme": "file",
        "mount": "workspace-docs",
        "priority": 20,
        "config": { "path": "${workspace.dir}/docs" }
      }
    ]
  }
}
```

当 `context_layer` 不存在或为 `null` 时，Agent 使用默认 D2 chain
（`default_agent_context_spec()`：memory(5) + warm-summary(7) + message-list(10)）。
无需任何配置即可获得完整的 ContextResource chain 体验。

---

## 模板变量

`file` scheme 的 `config.path` 支持 `${workspace.dir}` 模板：

| 模板 | 解析为 |
|---|---|
| `${workspace.dir}` | 替换为 `.`（相对于工作区目录，在组装时通过 `AssemblyContext.profile_dir` 解析） |

未来版本可能添加更多模板变量。