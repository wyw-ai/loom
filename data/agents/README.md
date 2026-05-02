# `data/agents/` — AgentSpec 库

Joi 原生的 `AgentSpec` 定义集合，通过 `joi agent register` 注册。每个子目录交付：

- `spec.json` —— `AgentSpec` JSON。可被 `serde_json::from_str::<AgentSpec>(...)`
  正确解析，并能通过 `joi agent register` 完整往返（参见下面的 *验证* 小节）。
- `bundle/SKILL.md` —— actor 的 skill 正文。Agent 在首轮通过 `firstTurnPrefix`
  载入（`Read your skill from {agent.bundle}/SKILL.md.`）。
- `bundle/README.md` —— 给运维者的说明（handoff 契约、产出/消费的 artifact、
  prompt template 变量）。

Phase 3 交付的 actor 清单：

| Actor               | Provider | Slash 前缀（handoff prefix） | Scope     | 主要产出 |
| --- | --- | --- | --- | --- |
| `classmaster`       | claude   | `/classmaster`               | channel   | `task-goal.json`、`definition-of-done.json` |
| `teacher`           | claude   | `/teacher`                   | channel/thread | `lesson-plan.md`、`validation-report.json` |
| `lesson-designer`   | claude   | `/lesson-designer`           | channel   | `lesson-plan.md` |
| `router`            | claude   | `/router`                    | channel   | 派发到 discovery/teacher/delivery |
| `discovery`         | claude   | `/discovery`                 | channel/thread | `clone-manifest.json`、仓库笔记 |
| `delivery`          | claude   | `/delivery`                  | thread    | 代码改动、MR、`validation-report.json` |
| `feedback-fix-orchestrator` | claude | `/feedback-fix-orchestrator` | channel/thread | 派发反馈修复任务 |

上述所有 artifact 严格遵循 `docs/artifact-contracts.md`。

## 约定

- `actor.id` 沿用既有舰队的 `actor_<带下划线的 kebab>` 命名；`displayName` 用
  自然语言（中文）。
- `transport.kind = "interactive_command"`。`provider.kind` 决定走 `claude` 还是
  `copilot`；如果 spec 里写了 `transport.model`，runtime 会把 `--model=<id>`
  接到底层命令的参数尾。
- `prompt_template.everyTurnPrefix` 里写标准的 `[joi handoff v1]` 信封
  （设计文档 `docs/remove-dev-helper-migration-design.md` §5）。`firstTurnPrefix`
  里写 `[joi bootstrap]` 引导段。这两段保留英文，避免将来 runtime 抽取字段时
  踩到本地化坑；它们不影响 LLM 的中文理解。
- `handoff.triggerPromptPrefix` 在每次 inbox 派发时由 runtime 自动注入到 trigger
  内容前面，让底层 provider 激活对应的 skill（设计文档 §5.0）。
- `bundle.source` 为工作区相对路径。Agent host 在 register 时把 bundle 拷到
  `{agent.root}/bundles/<version>/`。

## 验证

每个 spec 都必须能解析为合法的 `AgentSpec`。冒烟脚本：

```sh
JOI_AGENT_SPECS=$(mktemp -d) cargo run -q --bin joi -- \
    agent register data/agents/classmaster/spec.json
```

正常退出 + `registered actor_classmaster at …` 即代表 JSON 结构合法。同一个
`JOI_AGENT_SPECS` 目录可以传给 `joi agent serve --specs <dir>` 做 runtime
冒烟（Phase 5）。
