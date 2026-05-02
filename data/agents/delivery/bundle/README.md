# delivery —— bundle

Thread 级执行者。消费 lesson plan + clone manifest，在已 provision 的仓库里
落地代码、开 MR，并产出供 teacher 打分用的验证证据。

- 消费：`task-goal.json`、`definition-of-done.json`、`lesson-plan.md`、
  `clone-manifest.json`，以及 thread-bound `mr-detector` 流式吐出的
  `mr-event-*.json`。
- 产出：代码改动、MR descriptor artifact、验证证据 artifact。
- Handoff 去向：`teacher`（验证阶段）；遇到硬阻塞时退回 `router`。

Provider：`claude`，走 `interactive_command`，`maxTurnMs = 3600000` 留给较长的
代码编辑回合。设计文档 §5.1 描述的 preFlight gate（要求 `.joi/state/scope.json`
存在）暂未在 spec 里声明 —— 该字段是 forward-compat hook，会随 p4c 一起落地。
