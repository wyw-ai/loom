# mr-detector —— bundle

**Thread-bound** scheduler 服务。每个 thread 实例独立轮询一组 MR 状态，将
变化以 `mr-event-*.json`（`docs/artifact-contracts.md` §6）的形式 publish。
当所有目标 MR 都 `merged` 或 `closed` 时实例自我完成，host 会停掉它并释放
thread 常驻资源。

- Kind：`scheduler`
- Lifecycle：`thread-bound`（设计文档 §4.7、Phase 4c）
- Bind：`scope = thread`；`auto_stop_on = ["thread.closed", "service.self_complete"]`
- Params schema：thread instantiate 时由调用方提供 `mrs[]`、可选 `poll_interval_s`。
- Poll backend：`MR_DETECTOR_FETCH_CMD` 环境变量可换成任意命令，方便对接
  GitLab / GitHub / 自研 server。默认 fixture 仅在 `--dry-run` 中使用。
- 自我完成协议：终态轮的 stdout 末尾输出
  `{"service.self_complete":true,"reason":"merged"|"closed"}`。

事件 fingerprint：`raw_event_fingerprint = "sha256:" + sha256(canonical_payload)`，
保证去重。

## 离线冒烟

```sh
data/services/mr-detector/bundle/poll.sh --dry-run
```

退出码 0；stdout 逐行 JSON；不联网、不写入磁盘。
