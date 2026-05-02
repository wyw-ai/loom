# Remove dev-helper Migration — Review (v3)

> 第三轮 review，针对修订后的 `docs/remove-dev-helper-migration-design.md`（759 行）。
>
> 用户本轮两条核心要求：
>
> 1. Joi 自身改造方案是否遵循 Joi 设计理念（不造多余概念）。
> 2. 在该理念下，agent / service 迁移方案是否完备，且 **agent 之间可插拔、耦合不重**。

参照基线：

- `docs/protocol/open-multi-actor-collaboration-protocol-v0.md`
- `docs/protocol/channel-workspace-model.md`
- `docs/service-plugin-system-design.md` / `docs/scheduler-plugin.md`
- `docs/architecture.md`
- 远端 inventory（7 agents + 5 services + bridge 脚本）

---

## 0. 总评

### 0.1 Joi 改造侧（problem 1）：基本达标

v2 review 列的概念蔓延问题，本轮**几乎全部修掉**。逐条核对：

| v2 提的问题 | v3 状态 |
| --- | --- |
| `work_item` / `{work_item.id}` 抬到 runtime 模板变量 | 已删，模板变量表只剩通用维度 |
| `.joi/work-items/{id}/...` 进 workspace 标准目录 | 已删，§2.3 明确"`work-items/` 等是 skill 约定，不是 runtime 标准目录" |
| preFlight 硬编码业务子路径 | 例子改成 `classroom/current/...` 并加注 "runtime 不理解 work_item / clone_manifest 等业务含义" |
| `scope_role: resident_discovery` 业务 role 抬进 runtime | 已删，scope.json 只剩 `read_only` 通用属性 |
| `canonical_work_item_id` 在 thread scope.json | 已删 |
| `event _meta.workItemId` protocol 业务洞 | 已删，§6.3 明确 "runtime 不通过 regex 从 prompt 提 id" |
| dispatch-state 进 workspace | 已迁到 agent host data dir（§6.3） |
| ServiceSpec `mode: schedule/poll/subscribe/once/hybrid` 重复枚举 | 已删，§7 显式说 "复用 service-plugin-system-design / scheduler-plugin，不新增 service mode 枚举" |
| `JOI_REPOS_DIR` env / `joi repo-notes` 顶层命令 | 已删，repo-notes 改成 `joi service once repo-notes -- ...` |
| `.joi/` 是 scope-shared 但与 channel-workspace-model 私有原则冲突 | §2.3 已声明扩展关系 |
| `feedback-fix-orchestrator` 错放 service | §7 明确归类为 agent；§10/§11 也跟进 |
| `researcher` alias 缺失 | §8 已加 `discovery.aliases = ["researcher"]` |

**结论**：Joi runtime 这一侧的设计现在确实只在 protocol 已有的 event / workspace / artifact / action / agent runtime / service runtime 之上做"补能力"，没有自造新一等概念。**这一条达标**。

### 0.2 Agent/Service 迁移完备性 + 可插拔（problem 2）：仍有结构性缺口

设计**能让链路跑起来**，但 **agent 之间会通过共享 workspace 文件产生强耦合**，做不到"可插拔"。具体见 §1–§3。还有一些 agent/service 在 phase plan 里没排到位，见 §4。

Severity：

- **B（必改）**：影响"可插拔"目标，落地后再回头改成本高。
- **C（应改）**：完备性缺口，phase plan 里漏排或不清晰。
- **D（可改）**：偏好建议。

---

## 1. 可插拔的核心缺口：agent 间靠"共享 workspace 文件路径"耦合 — **B**

### 1.1 现状

- `.joi/` 之外的 workspace 子树是 **scope-shared** 且 mutable（§2.3 把 channel-workspace-model 的 per-agent private 显式扩展成 scope-shared）。
- §14 附录把 `task_goal.json` / `definition_of_done.json` / `clone_manifest.json` / `execution_plan.json` / `progress_snapshot.json` / `lesson_plan.md` 全放在 `<scope_ws>/classroom/work-items/<id>/` 下。
- §10 Phase 3b 写的是 router/discovery/delivery "同时接入 repo-cache service 和 delivery provision"——这意味着这几个 agent 现实中通过**同一组路径名 + 同一组字段名**互相消费输出。
- §5.1 preFlight `requireWorkspaceFiles` 也强化了"下游 agent 通过路径字面量声明上游 agent 必须产出的文件"。

### 1.2 问题

这套布置等于把"agent A 和 agent B 之间的协议"放到了**workspace 路径字符串 + 文件 schema** 上。后果：

1. **不可插拔**。换掉 discovery 实现，必须保持它写出来的 `task_goal.json` / `definition_of_done.json` 字段名、文件名、目录名一字不差。
2. **没有版本化**。workspace 文件是 mutable，任何 agent 都能写，没法回放、没法追溯"是哪一个 turn 产出的这份 DoD"。
3. **违反 §2.6 自己写的原则**。§2.6 说"能表示为协作事实的写成 event；需要稳定引用或验收证据的发布成 artifact"。但 `task_goal` / `definition_of_done` / `clone_manifest` 恰好是 agent 之间的"协作事实+验收证据"，现在却落在 mutable workspace 文件里。
4. **回滚困难**。一个 agent 的 bug 把共享 mutable 文件写坏，下游全部受牵连，没有 immutable 版本可以回退。

### 1.3 建议（必须落到设计文档）

补一节 **"§X. Agent 间协作契约：事件 + artifact，禁路径约定"**，明确：

1. **跨 agent 的"输入 / 输出"必须通过 event + artifact**，不通过共享 workspace 文件名。
   - 上游 agent 完成一份产出（task_goal / DoD / clone_manifest / execution_plan / lesson_plan / progress_snapshot 任意一份），先写自己的 workspace 草稿，**达到可被消费的状态时发布成 artifact**，然后用 `content.add` event + `attaches_artifact` relation 通告。
   - 下游 agent 通过 actor inbox 拿到 handoff event → 通过 relation 读 artifact，**不去 stat 一个约定路径上的 mutable 文件**。
2. **workspace 文件只是 agent 自己的工作现场**。允许 scope-shared 用于运维/复盘可见，但**不是跨 agent 协议**。
3. **preFlight `requireWorkspaceFiles` 的语义要收紧**：只用于检查"agent 自己启动需要的本地输入"，例如 router 自己之前 turn 留下的状态。**不允许**用 preFlight 表达"我等上游 agent 产出文件"——后者必须通过 handoff event 携带 artifact。
4. **§14 附录的迁移映射要分两层**：
   - `task_goal / definition_of_done / clone_manifest / lesson_plan / validation_report / release_receipt / runtime_receipt` 这些是**跨 agent 契约**，迁移目标是 **artifact**（命名 + DoD relation），不是 workspace 文件。
   - `execution_plan / progress_snapshot / capability_atlas` 这种 single-owner 的可变草稿，可以落在 workspace，但应限定 owner（"discovery 自己的 atlas"、"delivery 自己的 plan"）并明确"只有 owner agent 写"。
5. **可选**：若想保留 channel-workspace-model 的 per-agent 私有模型，可以在 scope workspace 下增加 per-agent 子目录 `<scope_ws>/.joi/agents/<actor_id>/private/` 作为单 owner 的可写区，scope-shared 区只读其它 agent 的内容（或更直接：跨 agent 可见内容只走 artifact）。

落到 Phase 计划：Phase 3b 之前必须先有一条 "**定义 cross-agent artifact 契约清单**"——枚举哪些产出是跨 agent 的，统一发布成 artifact 而不是文件。

---

## 2. AgentSpec / ServiceSpec 的"可插拔"语义没明说 — **B**

### 2.1 现状

设计反复说"AgentSpec / ServiceSpec 是 deployment artifact"，但**没说**：

- 一个 spec 文件丢进配置目录就能注册一个 agent / service？
- 删除 spec 就下线？无残留状态？
- spec 之间是否允许相互引用？例如 ServiceSpec 引用 AgentSpec id 做 handoff target？
- AgentSpec 之间是否允许相互引用？例如 router AgentSpec 在 prompt 里写死下游 actor id？

### 2.2 问题

如果 router/discovery/delivery 的 prompt template 里写死 "handoff to actor:teacher"，换掉 teacher 实现就要改 router。还是耦合。

### 2.3 建议

补一节"**§X. Spec 是可插拔单元**"，明确：

1. **AgentSpec 是 agent 的唯一注册入口**，drop-in / drop-out。运行时状态全部在 agent host data dir，删 spec 后重启即清。
2. **跨 actor 引用**只允许通过：
   - protocol 事件（handoff 写 actor id 是 OK 的，但具体 actor 由 channel membership 决定，prompt 不应写死）。
   - artifact uri（不可变引用）。
   - workspace 路径**仅限 agent 自己的 workspace**。
3. **prompt template 里不允许写死下游 actor id**。下游目标由 skill 自己决定（基于 channel membership / scope.json / artifact metadata）。如果当前 classroom 业务必须写死 teacher，应放在 skill SKILL.md 里、由 skill 自己决定 handoff target，而不是 AgentSpec 模板字段。
4. **ServiceSpec 同理**。其副作用（写哪个 channel、handoff 给哪个 agent）由 ServiceSpec 自己声明，而不是与其它 ServiceSpec 隐式协调。

---

## 3. workspace 跨 scope 读权限没定义 — **B**

### 3.1 现状

§2.3 给出 channel-scope 和 thread-scope 两棵 workspace 树，但**完全没说 thread agent 是否能读所在 channel 的 workspace**。

实际上 classroom 业务里：

- router 在 channel scope 跑，会写 channel workspace（capability atlas、当前 work item 状态等）。
- teacher / classmaster / delivery 在 thread scope 跑，需要读 channel workspace（capability atlas、repo manifest）。

### 3.2 问题

如果只能读自己 scope 的 workspace，channel-level shared 的 capability_atlas / repo manifest 就没法被 thread agent 看见。设计目前把这套放在 `<channel_ws>/...`，但 §4.2 的 env 变量是 `JOI_SCOPE_WORKSPACE_DIR` / `JOI_CHANNEL_WORKSPACE_DIR` / `JOI_THREAD_WORKSPACE_DIR` 三个并列——也就是 **runtime 默认是允许跨 scope 读** 的，但**没在文档里写出来**。

### 3.3 建议

§4.2 显式声明：

- thread agent 通过 `JOI_CHANNEL_WORKSPACE_DIR` 可**读** parent channel workspace。
- 是否能**写** parent channel workspace 由 scope.json 的 `read_only` 控制（默认禁止跨 scope 写）。
- channel agent 不能读取 thread workspace（thread 是子作用域）。

把这一条写进设计才能让 §1.5 建议的"per-agent 私有 + scope-shared 只读"模型自洽。

---

## 4. 完备性：phase plan 仍漏一些 agent/service — **C**

远端实有 7 agents + 5 services（含 service-specs-disabled 但 bare process 在跑的 a1-e2e-trigger / scanner）。对照 §10 Phase plan：

| 实体 | 类型 | 在 §10 中的位置 | 状态 |
| --- | --- | --- | --- |
| classmaster | agent (Copilot) | Phase 3a | ✅ |
| teacher | agent (Copilot) | Phase 3a | ✅ |
| router | agent (claude-code) | Phase 3b | ✅ |
| discovery | agent (claude-code) | Phase 3b | ✅ |
| delivery | agent (claude-code) | Phase 3b | ✅ |
| sensai | agent (claude-code) | §8 列表里有，§10 无 | ❌ 漏 |
| bug-triage | agent (claude-code) | §8 列表里有，§10 无 | ❌ 漏 |
| feedback-fix-orchestrator | agent | Phase 3b | ✅ |
| feedback-scanner | service | Phase 4a | ✅ |
| triage-writer | service | Phase 4a | ✅ |
| mr-watcher | service | Phase 4b | ✅ |
| a1-e2e-trigger | service | Phase 4b | ✅ |
| a1-e2e-scanner | service | Phase 4b | ✅ |
| repo-cache | service（新增） | Phase 3b 里"同时接入" | ⚠️ 没单独排 |
| repo-notes | service（新增） | Phase 4? 没排 | ❌ 漏 |

**建议**：

1. Phase 3a 增加 sensai / bug-triage（两个都是简单 claude-code agent，与 classmaster/teacher 同形态可顺带迁）。
2. Phase 3b 之前应有 **Phase 3.0：repo-cache + repo-notes service 上线**（并完成 readonly-repos 的旧→新切换），否则 router/discovery/delivery 没 source code 输入，3b 会卡住。
3. Phase 4 的 cutover 顺序补一句：**a1-e2e-trigger / scanner 的 cutover 必须先 dry-run 比对 gap_fingerprint**（设计 §13 写到这一点了，但没串进 phase plan）。

---

## 5. 服务侧细节 — **C / D**

### 5.1 service 触发 agent 的语义没收敛 — **C**

§7 写了 "service 写 `event/append --handoff <agent>`，由 agent runtime 唤醒对应 agent"。这是对的方向。但漏了：

- service 写 event 时使用的 actor 身份是什么？是 service 自己的 actor id 吗？需要明确：每个 ServiceSpec 注册一个 service-actor（actor.kind = service），它在 channel membership 里以 service 身份出现。
- handoff 的 target 必须是 channel membership 内的 actor。设计应明确："service 在 channel 里 publish event 必须先 join membership"。

protocol v0 §7.7 已有 actor concept，但没显式区分 agent / service / human。建议本设计里说明 "ServiceSpec → service actor，AgentSpec → agent actor，二者都按 protocol §7.7 处理"。

### 5.2 a1-e2e-trigger / scanner 应合并为一个 ServiceSpec 多个 job — **D**

scheduler-plugin 已有 "一个 ServiceSpec 内多 job" 的能力。trigger（定时触发 pipeline）和 scanner（周期性扫描 result）是同一对外部系统的两个 cron job，合并能少一个 spec、共享 cursor 文件、共享 dedupe state。

### 5.3 service "once" 与 "subscribe" 的 idempotency — **D**

§7 表格列了 subscribe plugin / once command，但没说幂等约束。建议引用 scheduler-plugin §9.2 的 RespondsTo 反向投递 + dedupe.jsonl，避免本设计在 service 层重新发明。

---

## 6. 其它小点 — **D**

1. §2.3 workspace 树形图里仍有 `skills/` 子目录，但 §4.2 又用 `JOI_SCOPE_SKILLS_DIR` 通过 env 提供。两者关系（skills 是装在 workspace 里、还是从 bundle 投影进来）应该写清楚。建议明确 "skills 是 runtime 投影，不是 workspace 持久文件"。
2. §5 模板变量表里 `{agent.bundle}` 的解析机制没说明。建议加一行 "由 AgentSpec.bundle 字段或 actor.kind=agent 注册时的 bundle 路径解析"。
3. §6.3 第 4 条 "session signature 字段" 列举包括 "settings"——Claude provider 的 settings 文件较大，建议改为 "settings 文件 hash"。
4. §11 Todo #4 把 "repo cache / repo notes" 合在一项，但二者是两个独立 service。建议拆成 4a / 4b 与 §10 phase 对齐。
5. §14 附录表头加一句免责："本表是 classroom skill 自己的迁移参考，**不是 Joi runtime 强制约定**"——v2 review 提过，这次把这句话补进表头会更稳。

---

## 7. 总结：要落到设计上的 must-fix

按"先解决可插拔、再补漏排"顺序：

1. **§1（B）**：补一节"Agent 间协作契约：事件 + artifact，禁路径约定"，把 `task_goal / DoD / clone_manifest / lesson_plan / validation_report / release_receipt` 类跨 agent 产出明确为 artifact，不再当 mutable workspace 文件协议。preFlight `requireWorkspaceFiles` 收紧为"agent 自己的本地输入"。
2. **§2（B）**：补一节"Spec 是可插拔单元"，明确 drop-in / drop-out、删除 spec = 下线、prompt 不写死 actor id、跨 actor 只通过 event/artifact + membership。
3. **§3（B）**：§4.2 / §6 显式定义跨 scope workspace 读权限（thread → channel 可读、不可写；channel ⇏ thread）。
4. **§4（C）**：§10 Phase plan 补 sensai / bug-triage（3a），增加 Phase 3.0 (repo-cache + repo-notes service 上线)，并把 a1-e2e cutover 的 fingerprint 比对串进 phase。
5. **§5.1（C）**：明确 ServiceSpec 注册的是 service-actor，service publish event 走 channel membership，与 AgentSpec 的 agent-actor 在 protocol §7.7 上对齐。
6. **§5.2 / §5.3 / §6（D）**：合并 a1-e2e 双 job、引用 scheduler dedupe、skills/bundle 关系澄清、settings hash、Todo 拆分、附录表头免责。

做完 1–4 后，这套方案才真正能做到"agent 是负责一部分事情的可插拔模块，谁实现 task_goal 的产出都行，下游只看 event + artifact 不看路径"。

## 8. 一句话结论

**Joi 改造方案这一侧已经清干净了，遵循 Joi 设计理念基本到位（v2 review 几乎全部采纳）；但 agent/service 这一侧的"协作契约"还停留在共享 workspace 文件路径上，必须升级为 "event + artifact"，否则可插拔目标不可达。** 6 处必改 / 应改要落到设计文档里，再开 Phase 1 实施。
