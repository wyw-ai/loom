# a1-bug-fix-loop

这是新版 Loom-native a1-dev-canfeng 的反馈队列适配器，不维护第二套任务状态。

## 运行边界

- 有 confirmed TaskRef 的反馈，后续服务按 Loom Task 写 TaskFact。
- 没有 task id 的反馈，只输出 `a1_dev.task_ref_candidate`，由 router 决定是否创建或绑定 Task。
- 本服务不创建 discovery / delivery thread，不 wake message delivery / examiner，不写 MR 结论。
- 本服务不依赖旧版自动开发 runtime。

## 本地验证

```bash
data/services/a1-bug-fix-loop/bundle/tick-native.py --dry-run
```
