# plugin.json 清单指南（资源插件声明格式）

> 一个资源插件（plugin）= context layer 的一个供给单元。本文定义它的声明清单
> `plugin.json`（v2 schema）：字段表、v1 归一化规则、`executable` 预留字段，以及
> 官方源 `official-plugins.json` 的数据化说明。
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

## 官方源数据化（official-plugins.json）

官方插件源清单是数据文件，不是代码: `crates/cli/official-plugins.json`（与
build.rs 同目录，构建期消费）。每条源:

| 字段 | 说明 |
|---|---|
| `id` | 源标识（`--verbose` 展示用） |
| `dir_env` | 本地目录覆盖的环境变量列表（按序探测） |
| `repo_dir_name` | 缓存目录名 |
| `repo_anchor_rel` | 仓库锚点相对路径（校验仓库身份） |
| `repo_env` | 仓库 URL 覆盖的环境变量 |
| `default_repo_url` | 默认克隆地址 |
| `ref_env` | ref 覆盖的环境变量 |

**加官方插件 = 编辑此 JSON + 提供仓**，零核心代码改动。

build.rs 的角色是**官方源清单处理器 + 预装缓存加速器**: 读清单 → clone/复用本地
目录 → 解析各源 `plugin.json` → 生成嵌入快照。**注册的事实源是 plugin.json 清单，
不是 build.rs**。

## 构建期行为

清单在构建期解析，所有错误 fail loud（带清单路径的 panic）: 缺失必填字段、未知
`$schema`、非法 `layer` 枚举、`executable` 字段类型/枚举错误、`loom_version` 不
满足——数据文件是构建输入，错误不可静默。解析与归一化契约的实现见
`crates/cli/plugin_manifest.rs`（该模块同时编译进 build.rs 与 CLI 测试）。
