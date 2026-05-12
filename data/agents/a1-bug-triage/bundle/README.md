# a1-bug-triage —— bundle

a1-dev-canfeng 频道里的缺陷／反馈分流器。接到 router 转交或 human 直接
说出来的反馈，把它归一化成 `bug-triage.v1.json` artifact，并给出下一跳建议
（delivery 直接修 / discovery 进一步查 / router 找用户补 repro）。

- 消费：反馈正文 + 可能挂着的 `feedback-bundle.json` / `mr-event-*.json` /
  仓库 mount。
- 产出：一个 `bug-triage.v1.json` artifact + 一次 handoff。
- 不修代码。

Provider：`claude`，走 `interactive_command`，envelope 同其它 actor。
