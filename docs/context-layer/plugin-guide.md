# plugin.json 清单指南（资源插件声明格式 · 规范唯一锚点）

> 一个资源插件（plugin）= context layer 的一个供给单元。本文定义它的声明清单
> `plugin.json`（v2 schema）：字段表、v1 归一化规则、`executable` 预留字段，以及
> 官方源 `official-plugins.json` 的数据化说明。
>
> **本文档是「符合 context layer plugin 规范」的唯一规范锚点**——五面一致判据与
> 豁免回收条款（见下文）只在此维护; docs 全族其他文档不得出现第二套插件规范表述。
>
> 相关文档: [第三方资源扩展指南](./third-party-resource-guide.md)（Rust 侧
> `ContextResource` 实现与 inventory 注册）| [plugin list 自省](./plugin-list.md)
> | [配置参考](./configuration.md)（agentcontext.json）

---

## 概念定位

**context layer 是唯一顶层概念**——Agent 提示词组装的 AOP 中间层（scope resource
接入 / actor 侧解析 / agent context 产出 / 横切关注点处理）。**plugin 是角色词**
（层的供给单元），不是独立系统。`plugin.json` 是供给单元的声明清单，也是注册的
事实源。

> 仓名消歧: 官方插件仓 `loom-plugin-context-tier` 的名字沿袭迭代 1 历史，指
> 「context layer 的 tier 资源插件」，不是一个独立的插件系统。

---

## 插件规范 — 五面一致判据（唯一锚点）

**核心原则: 「同一规范，三种出身」。** 出身（官方内部 / 官方外部 / 第三方）只决定
代码住哪、谁维护，**不改变任何用户可见行为面**。出身是溯源元数据，不是行为轴。

「符合 context layer plugin 规范」= 以下五面全部成立:

| 面 | 判据 | 用户可见锚点 |
|---|---|---|
| **F1 清单面（声明）** | 每个插件恰有一份 `plugin.json`（v2）七字段全声明。官方内部插件清单住主仓内（如 `crates/plugin-memory/plugin.json`）并纳入官方源数据; 官方外部住其仓并登记官方源; 第三方住己仓。**零无籍插件** | `plugin.json` 存在且可解析 |
| **F2 注册面（供给）** | 统一注册语义——同一标准 inventory 路径（`ContextResourcePlugin` + `inventory::submit!`）。出身不改变注册方式，无编译期专属接线 | `plugin list --verbose` 的 `registered:` 列 |
| **F3 config 面（配置）** | 唯一配置方言 `resources[].config` + per-field 合并。无插件专属顶层配置通道; 遗留字段只能是兼容别名（可迁移、有废弃周期），不是并行通道 | `config keys:` 列（源自 `config_schema`） |
| **F4 枚举面（自省）** | `plugin list` 行**同构**——所有字段可解析（id/name/version/source/priority/config_keys/registered_via/endpoint 无 None 空洞）; 出身差异只体现为 `source` 溯源取值（`official` / `external`） | `plugin list` 表格与 `--json` |
| **F5 排查面（溯源）** | 同一排查路径——`--verbose` 四问结构一致（谁注册/什么身份/接受什么配置/端点在哪），section 级溯源（`SectionSource`）统一覆盖 | `--verbose` 输出 + section 溯源 |

**出身与 `source` 取值的对应**: 官方内部与官方外部插件在 `plugin list` 中均显示
`official`（清单在册 + inventory 注册）; 第三方 inventory 注册但无清单的显示
`external`。宿主内置资源（如 `file`）不是插件——它是装配基线自身依赖的宿主基础设施
（无业务语义、无版本演进诉求），在表格中 version 列显示 `builtin` 以区分。

## 豁免与临时通道条款

**豁免不是免债，是挂账。** 任何对插件规范的豁免、桥接或临时通道（如遗留配置别名、
过渡期注册路径）必须同时满足四要素:

1. **临时标注** — 以「临时」显式标注，禁止表述为永久设计;
2. **回收条件** — 登记触发事件或期限至少其一（如「下一迭代」「S2 落地时」「某版本
   发布后」）;
3. **豁免清单登记** — 记入下方豁免清单（exemption ledger，随本锚点维护）;
4. **回收时 golden 验证** — 回收时以功能守护测试（golden 等价）验证无行为漂移。

**无回收条件的豁免视为规范违例。**

### 豁免清单（exemption ledger）

| 登记项 | 类别 | 临时标注 | 回收条件 | 状态 |
|---|---|---|---|---|
| `spec.memory` actor 级字段 + `inject_memory_envelope` 注入桥 | 数据源桥接（memory 专属） | 是——桥接非终态设计 | actor 记忆配置声明迁至 agentcontext.json `resources[].config`，或桥泛化为通用数据源注入机制时退役; 回收时以 golden 等价验证（AC-U-5 基线） | 活跃（ARCH 技术设计 §2.2 裁定: actor 侧数据源注入，非注册特权，保留） |

> 流程教训（迭代 3 Task #14）: 迭代 2 将 memory 移出 layer 时保留遗留配置通道但未标
> 回收条件，成为后续规范割裂的种子。本条款即由此确立——新增任何桥接先过四要素。

---

## v2 字段表

`$schema` 取 `loom-plugin/v2`:

| 字段 | 必填 | 说明 |
|---|---|---|
| `$schema` | 是 | `loom-plugin/v1` 或 `loom-plugin/v2`（见下文归一化规则） |
| `id` | 是 | 唯一标识，kebab-case（如 `loom-plugin-context-tier`） |
| `name` | 是 | 显示名（`loom plugin list --verbose` 展示） |
| `version` | 是 | 插件版本，`X.Y.Z` |
| `loom_version` | 是 | 兼容范围，仅支持 `>=X.Y.Z` 形式；构建期检查，不满足则 build fail |
| `layer` | 是 | 枚举，S1 仅接受 `"context"`（S2 可能扩展其他层） |
| `skills[]` | 否 | 纯内容形态声明。每项: `id`（必填）、`path`（必填，相对仓根）、`scope`（`global` \| `scope` \| `actor-bundle`，缺省 `global`） |
| `context_resources[]` | 否 | 资源形态声明。每项: `scheme`（必填，对应 `ContextResourcePlugin.scheme`）、`priority`（可选，缺省 100）、`config_schema`（可选，v2 新增，声明本插件接受的 config 字段 key 集，供校验与 `--verbose` 展示） |
| `config_template` | 否 | agentcontext.json 模板的相对路径（init 时供给） |
| `executable` | 否 | **M5 预留字段，本轮不实现装载**: `command`（必填，string）、`args`（可选，string 数组）、`protocol`（必填，枚举仅 `jsonrpc-stdio`）。仅在 schema 层校验合法值，`--verbose` 展示为 reserved |

同一份清单可同时声明 `skills` 与 `context_resources`——schema 同时覆盖资源形态与
纯内容形态（纯内容的运行期装载属 S2）。

## v2 相对 v1 的变更集

| 变更 | 类型 | 理由 |
|---|---|---|
| `context_resources[].registration` 移除 | 删 | 值恒为 `inventory`（实现细节泄漏到清单）；注册方式由宿主侧事实决定 |
| `context_resources[].crate` 移除 | 删 | crate 名是 Rust 实现细节，清单不该声明 |
| `context_resources[].scope` 移除 | 删 | scope 资源分发从未实现（死代码，D-C3 裁决移除）；`skills[].scope` 保留（有消费方） |
| `context_resources[].config_schema` 新增 | 增 | config 拉平后插件可声明接受的 config 字段 |
| `executable` 新增 | 增 | M5 预留端点字段，仅校验不装载 |
| `$schema` 值改为 `loom-plugin/v2` | 改 | 版本标识 |

**v2 严格拒绝移除字段**: v2 清单中仍出现 `registration` / `crate` / `scope`（位于
`context_resources[]` 内）时构建失败并指明字段——版本定稿后再改字段集会造成版本
通胀，因此 v2 从第一天起严格。

## v1 归一化规则

**v1 可读（读取期归一化），不提供迁移工具。**

- build.rs 读取时 v1/v2 均接受；v1 输入在解析层归一化为 v2 内存表示。
- 归一化动作: `context_resources[].registration` / `.crate` / `.scope` 接受并静默
  丢弃；其余字段按 v2 语义解析。
- 理由: 迁移工具的服务对象（未知第三方）在 S2 之前不存在（YAGNI）；现存 v1 清单
  随迭代同步升级。S2 引入运行期安装时再评估迁移需求。

## 实例: loom-plugin-context-tier（v2）

```json
{
  "$schema": "loom-plugin/v2",
  "id": "loom-plugin-context-tier",
  "name": "Context Tier — Hot/Warm/Cold Temperature Model",
  "version": "1.1.0",
  "loom_version": ">=0.1.8",
  "layer": "context",
  "skills": [
    { "id": "context-tier", "path": "skills/context-tier", "scope": "global" }
  ],
  "context_resources": [
    { "scheme": "warm-summary", "priority": 7 },
    { "scheme": "message-list", "priority": 10 }
  ],
  "config_template": "context-resources/default-agentcontext.json"
}
```

## 实例: memory（官方内部插件，v2）

官方内部插件的参考实现——与官方外部插件（如 tier）走**同一规范**，仅出身不同
（清单住主仓内）:

```json
// crates/plugin-memory/plugin.json
{
  "$schema": "loom-plugin/v2",
  "id": "memory",
  "name": "Memory",
  "version": "1.0.0",
  "loom_version": ">=0.1.8",
  "layer": "context",
  "context_resources": [
    {
      "scheme": "memory",
      "priority": 5,
      "config_schema": { "memory": {} }
    }
  ]
}
```

要点:

- **清单住 crate 内**（`crates/plugin-memory/plugin.json`），并在官方源数据的
  `internal` 数组登记（见下节）——满足 F1;
- **注册走 inventory**（`inventory::submit! { ContextResourcePlugin { scheme:
  "memory", factory: memory_resource_factory } }`）——满足 F2，与 tier 完全同路径;
- **config 单方言**（F3）: 工厂从 config envelope 读取 `memory` key（`config_schema`
  声明）; actor 的 `MemorySpec` 由宿主在链构造前注入 envelope（per-field 合并、
  actor 侧胜出; 字段冲突时记 error 级日志但合并不中断——D-D3 裁定），
  malformed 输入 warn 后回退默认（skip-not-truncate）;
- **版本独立语义化**（D-R1-3）: `version: 1.0.0` 是插件自身版本，与 workspace
  crate 版本解耦。

## 官方源数据化（official-plugins.json）

官方插件源清单是数据文件，不是代码: `crates/cli/official-plugins.json`（与
build.rs 同目录，构建期消费）。文件为**对象形态**，按出身分两个数组:

```json
{
  "external": [ { "id": "...", "default_repo_url": "...", "...": "..." } ],
  "internal": [ { "id": "memory", "path": "crates/plugin-memory" } ]
}
```

**external 条目**（官方外部插件，如 tier）:

| 字段 | 说明 |
|---|---|
| `id` | 源标识（`--verbose` 展示用） |
| `dir_env` | 本地目录覆盖的环境变量列表（按序探测） |
| `repo_dir_name` | 缓存目录名 |
| `repo_anchor_rel` | 仓库锚点相对路径（校验仓库身份） |
| `repo_env` | 仓库 URL 覆盖的环境变量 |
| `default_repo_url` | 默认克隆地址 |
| `ref_env` | ref 覆盖的环境变量 |

**internal 条目**（官方内部插件，如 memory）:

| 字段 | 说明 |
|---|---|
| `id` | 插件标识（与 `plugin.json` 的 `id` 一致） |
| `path` | 主仓内清单所在目录（相对仓根） |

**加官方插件 = 编辑此 JSON + 提供清单**（外部另需提供仓），零核心代码改动。
历史顶层数组形态仍兼容读取（等价于 `external`）。

build.rs 的角色是**官方源清单处理器 + 预装缓存加速器**: 读清单 → clone/复用本地
目录（external）或直接定位主仓路径（internal）→ 解析各源 `plugin.json` → 生成嵌入
快照。**注册的事实源是 plugin.json 清单，不是 build.rs**。

## 构建期行为

清单在构建期解析，所有错误 fail loud（带清单路径的 panic）: 缺失必填字段、未知
`$schema`、非法 `layer` 枚举、`executable` 字段类型/枚举错误、`loom_version` 不
满足——数据文件是构建输入，错误不可静默。解析与归一化契约的实现见
`crates/cli/plugin_manifest.rs`（该模块同时编译进 build.rs 与 CLI 测试）。
