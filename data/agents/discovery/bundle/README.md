# discovery —— bundle

频道 / thread-bootstrap 级 agent。负责决定一个任务需要哪些仓库，并发布
`clone-manifest.json`（`docs/artifact-contracts.md` §3）。

- 消费：`task-goal.json`、`definition-of-done.json`，以及可选的频道级
  repo-cache 镜像。
- 产出：`clone-manifest.json`。
- Handoff 去向：`delivery`（典型）或退回 `router`。

Provider：`claude`，走 `interactive_command`，envelope 同其它 actor。
