# a1-dev-canfeng / classroom — Channel · Thread · Actor 设计文档

> 本文是 a1-dev-canfeng（自动化研发链路）和 classroom（actor 培训）两个频道
> 的拓扑设计文档，基于 Joi 原生元语（actor / channel / thread / event /
> workspace / artifact / action / agent / service），描述两个频道的**职责
> 切分、拓扑、工作流**，以及承接真实剧情所需的工程项。
>
> 设计原则：**少造概念，全部复用现有 Joi 元语；不为剧情新增 protocol
> 抽象**。本文所有"组件"都映射到下面几个之一：
>
> - actor（agent / human / service）
> - channel（人机协作的“工厂”）
> - thread（一次会话 / 一次任务现场）
> - event（content.add / handoff / turn.close / approval.* …）
> - workspace（channel.shared / thread.workspace + mounts）
> - artifact（跨 actor 的不可变快照契约）
> - action.request（人在回路的控制面）
> - agent spec / service spec（可热插拔的能力单元）

---

## 0. 当前状态对照（事实陈述）

| 组件 | 状态 | 说明 |
| --- | --- | --- |
| `chan_31f8fa85d909` a1-dev-canfeng | 已建，6 actor 在线 | router / discovery / delivery / a1-bug-triage / canfeng；`thread_00dec3971ca5` (router-desk) + `thread_2a6d3b569aa6` (discovery-desk) 为常驻 thread；smoke 已绿 |
| `chan_4a634872b6f8` classroom | 已建，3 actor 在线 | classmaster / teacher / canfeng；`thread_eae8326cd10e` (greeting) 常驻；smoke 已绿 |
| 两个 channel 的 **dev-helper 依赖** | 已彻底解除 | spec 全部 `interactive_command + claude`；旧 `command + run-agent.sh` 在 187 上已被替换并备份 |

但「能跑 ping」≠「能承担用户描述的剧情」。下文逐一拆解所需的 thread 拓扑、
artifact 契约、状态转移；同时把现有 spec / service 是否就绪标注清楚。

---

## 1. 命名与角色总览

### 1.1 a1-dev-canfeng channel 的 actor 集

| actor.id | displayName | 类型 | 职责一句话 |
| --- | --- | --- | --- |
| `actor_human_0240d58e` | canfeng | human | 提需求 / 拍板 / 评审 |
| `actor_router` | 路由 | agent (claude) | 接收人类需求，分诊到 discovery / 直接答复 / 升级缺陷分流 |
| `actor_discovery` | 调研 | agent (claude) | 在常驻 thread 中产出「四件套」（任务主题 / 粗方案 / DoD / 待修改仓库 list / 待参考仓库 list） |
| `actor_delivery` | 交付 | agent (claude) | 在派生 thread 中按 openspec 三段式自主写代码、发 MR、收 MR 反馈、修复 |
| `actor_a1_bug_triage` | 缺陷分流 | agent (claude) | 单条 feedback / 报错的归一化分流（产出 bug-triage.v1） |
| `service_a1_feedback_scanner`（待落地） | feedback 扫描器 | service | 定时通过 a1 命令拉 feedback、归一化、生成两份扫描报告 |
| `service_a1_bug_fix_loop`（待落地） | 存量 bug 修复 loop | service | 读取扫描报告中的存量 bug，按既定流程驱动一条龙修复 |
| `service_mr_detector`（已存在） | MR 监听 | service | 单 thread 内监听一个 MR，把 ci/conflict/comment diff 汇成事件 handoff 回 delivery |
| `service_repo_cache`（已存在） | 仓库缓存 | service | channel `shared/repos` 后端，git fetch / mirror 复用 |

> 命名约定：`actor_*` 用于 agent，`service_*` 用于 service。Service 也有
> `actor.id`（参与 channel 成员资格、签 event），但其能力以 ServiceSpec
> 而不是 AgentSpec 描述（不需要 LLM）。

### 1.2 classroom channel 的 actor 集

| actor.id | displayName | 类型 | 职责一句话 |
| --- | --- | --- | --- |
| `actor_human_0240d58e` | canfeng | human | 提教学需求 / 评审教案 / 批准 spec_apply |
| `actor_classmaster` | 班主任 | agent (claude) | 在 channel 公共聊天区与人类对齐「要训练哪个 actor、训练什么」 |
| `actor_teacher` | 教师 | agent (claude) | 在每个训练 thread 中预演 / 模拟 / 批改 / 评分 / 写教案 |
| 被训练的 actor（动态） | — | agent | 训练 thread 启动时由 classmaster 通过 `joi channel invite` 临时拉入 |

> 「被训练 actor」不是 classroom 的常驻成员；classmaster 在创建训练 thread
> 后**临时邀请**对应 actor 进 channel + 进 thread，训练完成后保留 channel
> 成员资格（方便复训），但只在该 thread 中有 resident_as 绑定。

---

## 2. a1-dev-canfeng — 工作流分解

### 2.1 Channel workspace 布局

```
channel_root/
├── shared/
│   ├── repos/                    # 仓库源（由 service_repo_cache 维护的 mirror / worktree 池）
│   │   ├── <repo_id_a>/          # bare-ish mirror，所有 thread 通过 mount 派生 worktree
│   │   └── <repo_id_b>/
│   ├── feedback-baseline.json    # 上一轮 feedback 扫描快照，diff 用
│   └── tasks/                    # 历史任务索引（artifact 引用为主，避免重数据）
│       └── <task_id>/index.json
└── artifacts/                    # channel 级 artifact（task-brief / clone_manifest / scan-report …）
```

`shared/repos` 是 channel-wide 共享目录。所有 thread 通过 §4.2.1 的 mounts
机制把它（或其中的一个 worktree）挂进 `thread.workspace/repos/`。**没有
跨 thread 复制源码**——thread workspace 只持有该任务用到的 worktree 链接 +
本地变更。

### 2.2 常驻 thread：discovery-desk

`thread_2a6d3b569aa6`，resident_as=`actor_discovery`。

**workspace mounts**：

```json
{
  "mounts": [
    { "from": "channel://shared/repos", "to": "shared/repos", "mode": "ro_link" }
  ]
}
```

→ thread 内 `shared/repos` 是 channel `shared/repos` 的只读软链。
discovery 只**读**，不在这里写改动。

**触发与产出**：

- 触发：router 在公共聊天区识别到「这是一条新的开发任务」后，发
  `handoff → actor_discovery`，scope=`thread_2a6d3b569aa6`，message 里附原
  始需求 + 任务初拟 id（`task_<short_uuid>`）。
- discovery 在该 thread 中同人类追问澄清（直接 `joi say` 到 thread；如果
  router 同时把 human invite 进 thread，则三方对话）。
- 产出：发布一个 artifact `task-brief.v1`，schema：

  ```jsonc
  {
    "schema": "task-brief.v1",
    "task_id": "task_xxxx",
    "title": "...",                    // 1. 任务主题
    "rough_plan": "...",               // 2. 粗方案
    "dod": ["..."],                    // 3. Done 完成定义（可机判优先）
    "modify_repos": ["repo_id_a"],     // 4. 待修改仓库
    "reference_repos": ["repo_id_b"]   // 5. 待参考仓库
  }
  ```

  以及一个 `clone_manifest.v1`（已有 schema, 见 artifact-contracts.md §3）：
  modify_repos 用 worktree mode，reference_repos 用 ro_link mode。

- **关单标准**：discovery 用 `__JOI_DONE__` 关闭本轮 turn，artifact 已
  publish。router 通过订阅 `artifact.published` 事件得知 ready。

### 2.3 派生 thread：delivery-task-`<task_id>`

discovery 产出 `task-brief.v1` / clone manifest 且进入交付阶段后，直接调用：

```
joi thread create --in <channel> \
  --title "deliver: <task title>" \
  --bootstrap-artifact artifact://<clone_manifest.v1 uri>
```

runtime 根据 manifest 写 thread `scope.json.mounts`（modify_repos →
worktree，reference_repos → ro_link），ensure_scope 时落盘；发
`thread.bootstrapped` event。

discovery 接着 `handoff → actor_delivery` 进入该 thread，message 内含：

- `task-brief` artifact uri（delivery 自行 fetch）
- 明确指令：按 `openspec-propose → openspec-apply-change → openspec-archive-change`
  推进；在每个修改仓库下独立走一遍这三步（避免单 PR 跨多仓 messy）
- 发起 MR 后用 `a1 mr submit`（已有命令）建 MR，并 `joi service start
  --spec mr-detector --in <thread> --params {"mr_id": "..."}` 拉起监听

**delivery thread workspace**：

```
thread.workspace/
├── repos/                  # 由 mount 投影
│   ├── <repo_id_a>/        # writable worktree
│   └── <repo_id_b>/        # ro symlink
├── openspec/               # delivery 自己用的草稿、proposal、apply 记录
│   └── <change_id>/
└── .joi/state/             # runtime 私有
```

### 2.4 MR 监听 loop（per-thread service）

`service_mr_detector` 在 delivery thread 内常驻（`scope=thread`），按
`spec.params.poll_interval` 周期：

1. 拉 MR 状态（ci 结果 / conflict / comments）。
2. 与上次扫描 diff（state 写在 thread workspace 的 `.mr-detector/state.json`，
   service 私有，不是 artifact）。
3. **有 diff** → 发布 `mr-status-diff.v1` artifact + `handoff →
   actor_delivery` 描述本次扫到的待处理项。
4. **无 diff** → 静默。
5. **发现 MR merged** → publish `mr-merged.v1` artifact + 关闭自己（service
   `self_complete = true`，runtime 收到后 stop service + close thread）。
   delivery 在这一步前已经把 `__JOI_DONE__` 收尾。

> Detector 的 dedup state **写在 thread workspace** 而不是 artifact——
> artifact 是「证据快照、跨 actor」，dedup 是「自己内部进度」，两者职责不
> 同。这点和 §2.5/§4.2 的设计原则一致。

### 2.5 Feedback 扫描 thread（常驻）

新增常驻 thread `feedback-scan`，resident_as=`service_a1_feedback_scanner`。

`service_a1_feedback_scanner`（**待落地的 ServiceSpec**）按 schedule
（默认每天 1 次，由 channel-level scheduler 触发）执行：

1. `a1 feedback list --since <last_scan_at>` 拉增量。
2. 对每条 feedback 调用 `actor_a1_bug_triage`（短 handoff，每条独立 turn），
   收回 `bug-triage.v1` artifact。
3. 把所有 triage 按 `next_actor` / `category` 归一化分桶，生成两份报告
   artifact：

   - `feedback-scan.bugs.v1`：分类=`existing_bug`、`certainty>=0.6`，含
     按 `severity` 排序的 bug list + 每条引用 `bug-triage.v1` artifact。
   - `feedback-scan.others.v1`：除上之外（new_request / unclear / duplicate
     / not_actionable）。

4. **bugs.v1 投递**：`handoff → service_a1_bug_fix_loop`（同 channel，
   独立 thread `bug-fix-queue`），让其按队列消费。
5. **others.v1 投递**：在 channel 公共聊天区 `joi say --channel`（不
   handoff），附 artifact uri + 与上次扫描的 diff 摘要（新增 N 条 / 关闭
   M 条 / 待人决策 K 条），由人类决定是不是要立 task。

> 扫描器**不调用 router**——避免 router 被周期性后台噪音淹没。人类看到
> others.v1 后想推进就直接在公共区 @router 提需求，回到 §2.2 的常规流程。

### 2.6 存量 bug 修复 loop（per-bug thread）

`service_a1_bug_fix_loop`（**待落地的 ServiceSpec**）订阅
`feedback-scan.bugs.v1`，对每条 bug 串行（默认并发 1，可配）执行：

| 步骤 | 动作 | 元语 |
| --- | --- | --- |
| 1 | `a1 feedback claim <id>` 标记“处理中” | a1 命令（dev-helper 替代品） |
| 2 | `joi thread create --in <channel> --title "bugfix: <feedback title>"` 派生 bugfix thread | thread |
| 3 | `handoff → actor_discovery` 到 bugfix thread，prompt 强调「短小 bug，必须一次产出 task-brief.v1」 | event + artifact |
| 4 | 收到 task-brief + clone_manifest 后，`a1 feedback reply <id> --message <粗方案摘要>` 把方案回给提报人 | a1 命令 |
| 5 | discovery 在 bugfix thread 内直接进入 delivery 阶段（按 §2.3 bootstrap + handoff delivery；本 thread 即 delivery thread），再 `handoff → actor_router` 汇报 `[delivery-started]` | event |
| 6 | 监听本 thread 的 `mr-merged.v1` artifact | artifact 订阅 |
| 7 | merged → `a1 feedback reply <id> --message "已合入，将随下次发版上线"` + `a1 feedback set-status <id> fixed` | a1 命令 |
| 8 | 关闭 bugfix thread；继续下一条 | thread |

> 这里的取舍：discovery 完成后不再让 router 进入 delivery 阶段。为了减少
> thread 数量，**bugfix thread 直接复用为 delivery thread**（discovery 在当前
> thread 里 bootstrap mounts + handoff delivery，再向 router 汇报
> `[delivery-started]`）。这要求 §2.3 的 `joi thread create --bootstrap-artifact` 变体
> 同时支持 `joi thread bootstrap --in <existing-thread>`，把 mounts 写进
> 已存在的 thread 的 scope.json——这是 §4.7 的小扩展，不新增概念。

### 2.7 a1-dev-canfeng 整体 thread 蓝图

> **修订（实施反馈）**：router 不再有独立的「办公桌」thread，**router 的工作
> 位置是 channel public chat**（人类 ↔ router 全在公共聊天区进行）。原来
> 187 上预建的 `thread_router-desk` 留作历史 thread，不再使用。

```
chan_31f8fa85d909 (a1-dev-canfeng)
├── public chat (channel scope, no thread)              ← 人类 ↔ router 入口（router 在此活动）
├── thread_discovery-desk  [resident, actor_discovery]  ← 调研常驻
├── thread_feedback-scan   [resident, feedback-scanner] ← 定时扫 feedback
├── thread_bug-fix-queue   [resident, bug-fix-loop]     ← bug 修复总控（loop 自身）
├── thread_deliver-<task>  [transient]                  ← 每个开发任务一个
└── thread_bugfix-<fb>     [transient, 兼 deliver]      ← 每条存量 bug 一个
```

> **AgentSpec id 约束**：所有 handoff target 一律使用带 `actor_` 前缀的
> 规范 id（`actor_router` / `actor_discovery` / `actor_delivery` /
> `actor_a1_bug_triage` / `actor_classmaster` / `actor_teacher`）。
> 历史上存在不带前缀的同名 actor（`discovery` 是 researcher 双态、不归本
> channel 用），handoff 不要派给它们。

---

## 3. classroom — 工作流分解

### 3.1 Channel workspace 布局

```
channel_root/
├── shared/
│   ├── curriculum/           # 课程大纲、训练历史索引
│   └── lesson-plans/         # 已发布教案 artifact 的本地缓存（可选，便于 grep）
└── artifacts/
    ├── lesson-plan.v1/       # 教案（含 spec_apply 块）
    ├── homework.v1/          # 作业题面
    └── grading-report.v1/    # 批改报告
```

### 3.2 常驻 thread：greeting

`thread_eae8326cd10e`，resident_as=`actor_classmaster`。

公共聊天区即 channel scope，但 classmaster 也常驻 greeting thread 收私聊
/ 长任务对齐。**对齐结果**：人类 + classmaster 决定要训练哪个 actor、目标
是什么、用什么作业，写成一份 `training-plan.v1` artifact 然后由 classmaster
派生训练 thread。

### 3.3 派生 thread：training-`<actor.id>`-`<topic>`

classmaster 调用：

```
joi channel invite <chan> <被训练 actor.id>      # 若未在
joi thread create --in <chan> \
  --title "training: <topic>" \
  --invite <被训练 actor.id> \
  --bootstrap-artifact artifact://<training-plan.v1 uri>
```

thread workspace mounts 没有强制约束（默认 channel shared/lesson-plans
ro_link）。teacher 在 thread 内：

| 阶段 | 动作 | 元语 |
| --- | --- | --- |
| 预演 | 出一道 `homework.v1`（场景 + 期望产出 + 自动评分钩子） | artifact |
| 模拟 | `handoff → 被训练 actor` 在 thread 内独立完成作业 | event |
| 批改 | teacher 读 actor 输出 + 自动评分钩子结果，发 `grading-report.v1` | artifact |
| Review | 复盘失败点；如需修 skill bundle，draft 一份 `lesson-plan.v1`（含 `spec_apply` block） | artifact |
| 复核 | 多轮 homework / grading 直到 `grading-report.v1.score >= threshold` | loop |
| 发布 | teacher 发起 `approval.spec_apply` action.request；人类 accept 后 runtime 自动 `joi spec apply` 改写目标 actor spec → `joi agent reload` | action + spec apply |
| 收尾 | 验证新 bundle 仍能通过同一份 homework；teacher `__JOI_DONE__`；thread 标 archived | event + artifact |

> classroom 不限于训练 a1-dev-canfeng 里的 actor——`actor_a1_bug_triage`
> 这种新 agent 的初次发布、router 的提示词改版、teacher 自身的迭代，都
> 通过 classroom 完成。**所有 actor 的“出生 / 改版”都收口到 classroom**
> 一个 channel，避免散落多处。

### 3.4 classroom 整体 thread 蓝图

```
chan_4a634872b6f8 (classroom)
├── public chat (channel scope)             ← 人类 ↔ classmaster
├── thread_greeting       [resident, classmaster]
└── thread_training-<actor>-<topic>  [transient, teacher + 被训练 actor]
```

---

## 4. 关键 artifact 契约（新增 / 已有）

> **实施修订（2024-Q2）**：原计划新增 `task-brief.v1` 单一聚合 schema 承
> 载「四件套」，实施时改为复用已有的 `task-goal.json` + `definition-of-done.json`
> + `clone-manifest.json` 三件组——五项内容（标题 / 粗方案 / DoD /
> 待改仓库 / 参考仓库）已被三件组完整覆盖（标题+narrative→task-goal，
> DoD→definition-of-done，repos→clone-manifest）。下表中 `task-brief.v1`
> 行因此作废，本文其它处仍出现的 “task-brief” 字样请按「task-goal +
> DoD + clone-manifest 三件组」理解；不再新增独立 schema。

| schema | 已有 / 待加 | 生产者 | 消费者 |
| --- | --- | --- | --- |
| `task-goal.json` | ✅ | discovery / router / classmaster | delivery / teacher |
| `definition-of-done.json` | ✅ | discovery / classmaster | delivery / teacher |
| `clone-manifest.json` | ✅ | discovery | thread create / bootstrap |
| ~~`task-brief.v1`~~ | 作废（见上方修订说明） | — | — |
| `bug-triage.v1` | ✅（artifact-contracts.md §7） | a1-bug-triage | router / feedback-scanner |
| `feedback-scan.bugs.v1` | 待加 | feedback-scanner | bug-fix-loop |
| `feedback-scan.others.v1` | 待加 | feedback-scanner | 人类（公共聊天） |
| `mr-status-diff.v1` | 待加（mr-detector 当前发 turn 内 message） | mr-detector | delivery |
| `mr-merged.v1` | 待加 | mr-detector | bug-fix-loop / 任意订阅者 |
| `training-plan.v1` | 待加 | classmaster | teacher |
| `homework.v1` | 待加 | teacher | 被训练 actor |
| `grading-report.v1` | 待加 | teacher | classmaster / 人类 |
| `lesson-plan.v1`（含 spec_apply） | ✅（在 §4.4 已设计） | teacher | spec apply runtime |

> 所有 schema 集中写到 `docs/artifact-contracts.md`，本文只列“为什么需要”。


---

## 5. 事件 / 状态机（关键迁移）

### 5.1 a1-dev-canfeng：开发任务状态机

```
[公共区: 人类提需求]
        │ router 识别 → 发 handoff
        ▼
[discovery-desk: clarifying]
        │ discovery publish task-brief.v1 + clone_manifest.v1
        ▼
[router: review + approval.task_start]
        │ user accept
        ▼
[deliver-<task>: bootstrap mounts + handoff delivery]
        │ delivery: openspec-propose → apply-change → archive
        │ delivery: a1 mr submit + start mr-detector
        ▼
[deliver-<task>: monitoring]
        │ mr-detector handoff back on diff (loop)
        │ mr-detector publish mr-merged.v1
        ▼
[deliver-<task>: closed]  → thread archived
```

### 5.2 a1-dev-canfeng：feedback 扫描 / bug 修复

```
[scheduler tick]
        ▼
[feedback-scan: scanner pulls a1 + handoff per-item to bug-triage]
        │ scanner publish bugs.v1 + others.v1
        ├── others.v1 → channel public say (人类决策)
        └── bugs.v1   → handoff bug-fix-loop
                ▼
        [bug-fix-queue: loop pop next bug]
                │ create bugfix thread + claim feedback
                ▼
        [bugfix-<fb>: discovery one-shot brief]
                │ a1 reply 粗方案 + handoff router
                │ discovery bootstrap mounts + handoff delivery (复用 §5.1 后半段)
                ▼
        [bugfix-<fb>: mr-merged] → a1 reply + a1 set-status fixed → 回 loop
```

### 5.3 classroom：actor 训练状态机

```
[公共区/greeting: 人类 ↔ classmaster 对齐]
        │ classmaster publish training-plan.v1
        ▼
[training-<actor>: 邀 actor 入场 + handoff teacher]
        │ teacher: homework → handoff actor → grading
        │ (loop 直到 score >= threshold)
        ▼
[teacher publish lesson-plan.v1 (with spec_apply)]
        │ approval.spec_apply
        │ user accept → runtime: joi spec apply + agent reload
        ▼
[training-<actor>: regression homework on new bundle]
        │ pass → archive
        │ fail → 回到 loop
```

---

## 6. 与现状的差距 / 待落地清单

按「不动元语，只补 spec / artifact / 命令」的原则梳理：

### 6.1 必修（缺少这些则剧情跑不通）

1. **artifact schema**：`task-brief.v1`、`bug-triage.v1`、
   `feedback-scan.bugs.v1`、`feedback-scan.others.v1`、`mr-status-diff.v1`、
   `mr-merged.v1`、`training-plan.v1`、`homework.v1`、`grading-report.v1`
   写入 `docs/artifact-contracts.md`（schema-only PR，无代码）。
2. **discovery skill 升级**：当前 SKILL.md 没有“四件套 + 双 artifact 同
   时发布”的契约文字，需要补；并教 discovery 区分「常规 task」与「short
   bugfix one-shot」两种 prompt 形态。
3. **router skill 升级**：识别「新需求 vs 闲聊 vs 单条缺陷」三类入流；
   收到 task-brief + clone_manifest 后发 `approval.task_start` action
   而不是直接派 delivery（人在回路）。
4. **`joi thread bootstrap --in <thread> --bootstrap-artifact <uri>`**：
   现状只支持 thread create 时 bootstrap，bug-fix loop 复用既有 thread
   时需要这个变体。改 `crates/cli/src/cmd/thread.rs` 加子命令；runtime
   ensure_scope 已经会按 scope.json.mounts 投影，所以底层不动。
5. **`service_a1_feedback_scanner` ServiceSpec**：schedule + a1 命令
   pipeline + 双报告 publish。脚本主体放 `data/services/a1-feedback-scanner
   /bundle/`，spec 在 `data/services/a1-feedback-scanner/spec.json`。
6. **`service_a1_bug_fix_loop` ServiceSpec**：订阅 bugs.v1、串行驱动
   bugfix thread。bundle 同上。
7. **`service_mr_detector` artifact 化**：把当前 inline message 升级成
   `mr-status-diff.v1` / `mr-merged.v1`，便于 loop 订阅。
8. **classmaster / teacher skill 升级**：补 training-plan / homework /
   grading 的产出契约；teacher 学会发起 `approval.spec_apply`。
9. **classroom 邀请被训练 actor 的工具链**：classmaster skill 中明确
   `joi channel invite <chan> <actor>` + `joi thread create --invite`
   的使用步骤；runtime 已支持，无需动协议。

### 6.2 可观察（不阻塞剧情，但建议尽早）

10. **`a1` 命令在 spec 中的可见声明**：目前 a1 是外部 CLI，agent 通过
    bash 调用；建议在 router / discovery / delivery / scanner / loop
    的 spec 中通过 `transport.env.PATH` 显式声明 a1 binary 位置，避免
    PATH 漂移（与本次 cutover 中遇到的 claude PATH 问题同源）。
11. **`provider.settings.mode=actor_profile` 落盘 hook**：本次 cutover
    踩过的坑——runtime 只 mkdir 父目录，不创 settings.json；建议在
    `joi agent install` / 首次 `agent serve` 启动时若文件缺失则从
    `~/.claude/settings.json` 软链或报错（明示而非静默）。
12. **resident_threads 在 channel scope.json 中显式登记**：当前 router
    通过约定名找 discovery-desk，建议改为读 `scope.json.resident_threads
    .discovery`（设计文档 §4.7 已定义）以便未来重命名。

### 6.3 设计待澄清（需要人类决策）

13. **bug-fix loop 的并发度**：默认串行 1。是否允许同时跑多条 bug
    （并发 N）？并发会让 mr-detector 数量上涨，对 187 资源是个问题。
    本设计文档暂定 N=1，后续按观察调整。
14. **「others.v1 与上次的 diff」摘要在公共区刷屏**：scanner 周期 1d
    时可控，若调短到 1h 则会噪音化。建议加阈值——「diff 为空时静默」。
15. **classroom 训练失败回滚**：当 lesson-plan.v1 spec_apply 后回归
    homework 不通过，是否自动 `joi spec apply --revert <prev>`？本设计
    建议**不自动回滚**——保留失败现场，由 classmaster handoff 人类决策；
    避免静默回滚掩盖问题。

---

## 7. 名词复用矩阵（验证“不造新概念”）

| 用户口语 | Joi 元语 | 备注 |
| --- | --- | --- |
| “channel 公共聊天” | channel scope event stream | `joi say --channel` / `joi event list --in <chan> --channel` |
| “常驻 discovery thread” | thread + `scope.json.resident_threads.discovery` | §4.7.1 |
| “shared/repos” | channel workspace + service_repo_cache | §4.5 |
| “thread workspace 软链 channel shared” | workspace mounts (mode=ro_link) | §4.2.1 |
| “任务四件套 / DoD” | artifact `task-brief.v1` + `clone_manifest.v1` | §4.3.1 |
| “openspec 三段式” | delivery skill 内部行为 | 无新协议 |
| “MR 监听” | per-thread service `mr-detector` | §7 |
| “避免重复扫描的状态” | service 私有 state 写在 thread workspace | §4.2 |
| “扫 feedback 出两份报告” | service `a1-feedback-scanner` 发 2 个 artifact | §6.1 #5 |
| “bug 修复 loop” | service `a1-bug-fix-loop` 串行消费 artifact | §6.1 #6 |
| “a1 命令” | dev-helper 替代 CLI（已存在） | 不在 Joi 协议层；agent / service 通过 bash 调用 |
| “教案 / 作业 / 批改” | artifacts `lesson-plan.v1` / `homework.v1` / `grading-report.v1` | §4 |
| “发布新 bundle” | `approval.spec_apply` + runtime spec apply + agent reload | §4.4 / §7.1 |
| “更新所有 actor 为最新版” | `joi agent reload --all` 或单 actor 触发 | 已支持 |

**结论**：本设计文档没有引入任何 protocol-level 新概念；所有“新东西”都
是 artifact schema 和 ServiceSpec 的补齐——它们本身就是 §2.6 强调的
“可热插拔的能力单元”，与“元语”是分层的。

---

## 8. 推进顺序建议（不在本文档承诺时间）

阶段 A（让 a1-dev-canfeng 端到端可跑通最短链路）：
1. §6.1 #1（仅 task-brief / clone_manifest 已有部分）+ #2 + #3 + #4
2. 在 187 上人工触发一条「请帮我做 X」走完 router → discovery → delivery
   → mr-detector → merged 全程

阶段 B（feedback / bugfix loop 接入）：
3. §6.1 #5 + #6 + #7
4. 跑一次「真实 feedback → bug-fix-loop → 修复 → reply fixed」全程

阶段 C（classroom 能改 actor）：
5. §6.1 #8 + #9 + 剩余 schema
6. 用 classroom 给 router 做一次提示词改版（最小闭环验证 spec_apply）

阶段 D（清理 / 可观察项）：
7. §6.2 全部
8. 退役 187 上的旧 channel `chan_bcf8e1e730bd` / `chan_68b967d1627f`

---

## 9. 速查：当前 channel / thread / actor 落地表

| channel | thread | actor | 状态 |
| --- | --- | --- | --- |
| a1-dev-canfeng | (channel public) | actor_router / canfeng | ✅ Phase A smoke 绿 |
| a1-dev-canfeng | router-desk (legacy, retired) | — | 保留历史，新流程不再使用 |
| a1-dev-canfeng | discovery-desk (resident) | actor_discovery | ✅ Phase A smoke 绿；三件组 publish 由 SKILL 落地 |
| a1-dev-canfeng | feedback-scan (resident) | feedback-scanner | ❌ 待落地（§6.1 #5） |
| a1-dev-canfeng | bug-fix-queue (resident) | bug-fix-loop | ❌ 待落地（§6.1 #6） |
| a1-dev-canfeng | deliver-`<task>` (transient) | delivery + mr-detector | ⚠️ 单步可跑，artifact 化未做（§6.1 #7） |
| a1-dev-canfeng | bugfix-`<fb>` (transient) | discovery + delivery + mr-detector | ❌ 依赖 §6.1 #4 + #6 |
| classroom | (channel public) | classmaster / canfeng | ✅ |
| classroom | greeting (resident) | classmaster | ✅ smoke 绿 |
| classroom | training-`<actor>` (transient) | teacher + 被训练 actor | ⚠️ artifact 契约未补（§6.1 #8） |

---

> 本文为设计参照；任何与现实代码不符之处以代码为准，PR 修改本文与代码同步。
> 如有 §6.3 中的待澄清项，可以直接在 PR 评论里讨论，更新表格 + 进入对应
> 阶段。
