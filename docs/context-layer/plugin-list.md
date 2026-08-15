# loom plugin list — 资源插件自省

> `loom plugin list` 枚举 context layer 的实际装配参与者。每行都有一条真实的供给
> 路径支撑——没有幽灵条目，没有暗注册。
>
> 相关文档: [plugin.json 清单指南](./plugin-guide.md) | [架构 Wiki](./architecture.md)

---

## 语义: 枚举 = 参与者

`loom plugin list` 的输出是**实际装配参与者的精确投影**，不是配置声明的罗列。每个
scheme 背后必须有真实的供给路径:

| 来源 | 供给路径 | 表格显示 |
|---|---|---|
| 官方内部资源插件（memory） | 主仓内清单（plugin.json）声明 + inventory 注册 | `official` |
| 官方外部资源插件（tier） | 外部仓清单（嵌入快照）声明 + inventory 注册 | `official` |
| 宿主内置资源（file） | 工厂表 builtin 条目（宿主基础设施，非插件） | `official`（version 列为 `builtin`） |
| 外部资源插件 | 仅 inventory 注册（`discover_plugins()`） | `external` |

> 官方内部与官方外部插件走**同一规范**（见 [plugin.json 清单指南](./plugin-guide.md)
> 的五面一致判据），出身差异只决定清单住哪。

**零幽灵**: 清单声明了 `context_resources` 但既无 inventory 注册、也不是 builtin
的 scheme，不会展示为可用——`list` 对账失败，非零退出并给出诊断。这类 scheme 在
装配时只会触发 unknown-scheme 警告并被跳过，展示为"可用"即是说谎。

**零暗注册**: inventory 注册但清单未声明的 scheme 正常展示（`external`）——
inventory 自注册是外部仓的合法供给线，注册表在 `discover_plugins()` 返回值中完全
可见；「暗」的定义是绕过一切可见清单且宿主不知情。

## 三种输出形态

### 表格（默认）

```
SCHEME        SOURCE    PRIORITY  VERSION
memory        official         5  1.0.0
warm-summary  official         7  1.1.0
message-list  official        10  1.1.0
file          official        20  builtin
```

### --verbose（排查四问的最小形态）

每插件一段，覆盖排查四问中最高频的「**这个插件从哪来 / 为什么没生效**」:

```
memory:
  source:      official
  version:     1.0.0
  priority:    5
  plugin:      Memory (memory)
  config keys: memory
  registered:  inventory
  endpoint:    in-process factory
```

| 字段 | 说明 |
|---|---|
| `source` | 供给路径分类（表格口径: official/external） |
| `version` | 清单声明的插件版本; 宿主内置资源（file）为 `builtin` |
| `priority` | 装配优先级（越小越先装配） |
| `plugin` | 所属插件的显示名与 id（清单声明的资源才有） |
| `config keys` | 插件声明的接受 config 字段（`config_schema` 的 key 集） |
| `registered` | 注册路径: `inventory` 或 `builtin factory table` |
| `endpoint` | 端点形态: 当前均为 `in-process factory`（进程边界端点属后续阶段） |

「为什么被降级 / 内容是什么」需要装配期 trace，属后续 `plugin inspect` 范畴。

### --json（experimental）

机器可读输出，供脚本与对账测试消费。**experimental**: 输出契约在 S2 前可变。

注意口径差异: JSON 使用精确分类 `builtin` / `official` / `external`（对账需要精确
区分），表格按用户口径将 builtin 显示为 `official`:

```json
[
  {"scheme":"memory","source":"official","priority":5,"version":"1.0.0",
   "plugin_id":"memory","plugin_name":"Memory","config_keys":["memory"],
   "registered_via":"inventory","endpoint":"in-process factory"},
  {"scheme":"warm-summary","source":"official","priority":7,"version":"1.1.0",
   "plugin_id":"loom-plugin-context-tier",
   "plugin_name":"Context Tier — Hot/Warm/Cold Temperature Model",
   "config_keys":[],"registered_via":"inventory","endpoint":"in-process factory"},
  {"scheme":"file","source":"builtin","priority":20,"version":"builtin",
   "plugin_id":null,"plugin_name":null,"config_keys":[],
   "registered_via":"builtin factory table","endpoint":"in-process factory"}
]
```

## 对账语义

`list` 的输出集与装配参与者集合做双向对账（嵌入清单 scheme ∪ inventory scheme ∪
builtin scheme == 输出集），由测试 `plugin_list_matches_assembly_participants`
守护——枚举漂移即测试失败。幽灵检测（清单声明 ∖ (inventory ∪ builtin) = ∅）同在
此测试内断言。
