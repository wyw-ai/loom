# classmaster —— bundle

classroom 频道级编排 agent。负责 actor 训练项目的 **收敛、归档、发布审批**：

- 消费：用户的训练需求、a1-dev-canfeng 的 actor 缺陷上报。
- 产出：`actor-defect.v1`、`training-plan.v1`、`definition-of-done.json`，
  并在通过后发起 `approval.spec_apply`。
- Handoff 去向：主要是 `actor_teacher`；发布结果和训练状态回 classroom 公共频道。

Provider：Codex CLI（`codex-joi` wrapper），走 `interactive_command`
transport（见 `spec.json`）。
完成契约：`__JOI_DONE__` 哨兵。

每轮注入到 skill 的 prompt envelope 变量：

| 变量 | 含义 |
| --- | --- |
| `{scope.kind}` / `{scope.id}` | 触发事件所在的 scope |
| `{actor.id}` | 当前 actor（`actor_classmaster`） |
| `{trigger.actor_id}` / `{trigger.id}` | 上游 handoff 来源 |
| `{channel.id}` / `{thread.id}` | 解析后的 scope 地址 |
| `{workspace.dir}` | 可写工作区根目录 |
| `{scope.skills}` | scope 视角下投影出的 skill 文件 |
| `{agent.bundle}` | 当前 bundle 在 agent host 上的安装根 |
