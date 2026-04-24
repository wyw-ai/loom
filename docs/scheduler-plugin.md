# Scheduler Plugin

The `scheduler` plugin runs cron-driven jobs inside `joi service serve`.
Each job ticks on a 5-field UTC schedule, fetches a body (`command` or
`http`), and writes one Joi event into the configured scope — optionally
with a `hands_off_to` relation so an agent picks it up.

> 状态：S3 plugin。第一个真正用 §6.2 长进程 trait 的 service。短进程
> handler（`am`）走 `joi service am-handler`，scheduler 不走那条路。

## 1. ServiceSpec

放到 `~/.config/joi/services/<id>.json`。最小示例（一条 command job +
一条 http job）：

```json
{
  "id": "ops_watchers",
  "kind": "scheduler",
  "actor": {
    "id": "svc_scheduler",
    "kind": "service",
    "displayName": "Ops Scheduler"
  },
  "channelId": "chan_ops",
  "config": {
    "jobs": [
      {
        "id": "ci_watch",
        "schedule": "*/10 * * * *",
        "source": {
          "kind": "command",
          "command": "a1",
          "args": ["ci", "run", "list", "--json"]
        },
        "scope": {
          "kind": "thread",
          "id": "thread_ci",
          "channelId": "chan_ops"
        },
        "targetAgent": "actor_planner",
        "cursorBy": "body_hash"
      },
      {
        "id": "release_watch",
        "schedule": "7 9 * * 1-5",
        "source": {
          "kind": "http",
          "url": "https://example.com/api/release/today",
          "method": "GET",
          "headers": { "Authorization": "Bearer ${TOKEN}" },
          "timeoutMs": 10000
        },
        "scope": { "kind": "channel", "id": "chan_ops" }
      }
    ]
  }
}
```

校验：

```bash
joi service validate ~/.config/joi/services/ops_watchers.json
```

启动：

```bash
joi service serve --allow-services ops_watchers
```

`channelId` 在 spec 顶层不是 scheduler 必需的（每个 job 自带 scope），
留着是为了 §6.1 的字段对齐 + 工具 / UI 可以快速找到主 channel。

## 2. SchedulerConfig 字段

| 字段 | 默认 | 说明 |
| --- | --- | --- |
| `jobs` | `[]` | 每个 job 一个对象，schema 见下表。空数组允许（boot 时只 idle，便于先把 spec 推上去再补 job）。|

## 3. JobSpec 字段（§8.3）

| 字段 | 必填 | 默认 | 说明 |
| --- | --- | --- | --- |
| `id` | 是 | – | spec 内唯一。同时是 `cursors/<id>.json` 文件名、§8.4 dedupe key 一段、`_meta.jobId`。|
| `schedule` | 是 | – | 5-field UTC cron。`*` / `*/N` / `M-N` / `M-N/S` / 列表。语法实现：`crates/cli/src/service/scheduler/cron.rs`。|
| `source.kind` | 是 | – | `command` 或 `http`。|
| `source.command` / `args` / `env` / `timeoutMs` | command 时必填 command | timeoutMs=30000 | `Command::new(command).args(args)` 起子进程。捕获 stdout 作为 body；exit ≠ 0 视为失败（`tracing` warn，不 emit event，不 retry — 等下次 tick）。|
| `source.url` / `method` / `headers` / `body` / `timeoutMs` | http 时必填 url | method=`GET`，timeoutMs=30000 | reqwest（rustls-tls，无 openssl）。仅支持 GET / POST。非 2xx 同样 warn 跳过。|
| `scope.kind` | 是 | – | `thread` 或 `channel`。|
| `scope.id` | 是 | – | thread id 或 channel id。|
| `scope.channelId` | `kind=thread` 时必填 | – | scheduler 启动时会 `ensure_channel_member` 这个 channel；`kind=channel` 时省略（channel id 就是 scope id）。|
| `targetAgent` | 否 | – | 设置后事件带 `hands_off_to -> actor:<id>`；不设置就是纯 logging。|
| `dedupeBy` | 否 | `payload_hash` | `payload_hash` / `source_event_id` / `none`。`source_event_id` 当前与 `payload_hash` 行为相同（S4 引入 source-side id 提取后才会真正按 source id 去重，schema 留着便于平滑过渡）。|
| `cursorBy` | 否 | `none` | `none` 每 tick 都 fire；`body_hash` 把 sha256(body) 写到 `cursors/<id>.json`，下次 tick 命中相同 hash 跳过。`jq:<expr>` 在 §8.3 表里列出但当前未实现，留到后续再补。|
| `singleInFlight` | 否 | `true` | 同 job 上一次 fire 没结束时，下一次 tick 跳过（warn 日志）。§8.5 默认值。|
| `awaitReply` | 否 | `false` | 设为 true 时 handoff 后阻塞等 `RespondsTo`；`targetAgent` 必填。结果只用于日志，不回写。|
| `awaitTimeoutSecs` | 否 | `60` | `awaitReply=true` 时的 timeout。Doc 推荐 ≤ tick 间隔的 1/2。|

## 4. 触发链路

```text
cron tick
  -> exec_source(command/http)
  -> sha256(body) → cursor diff（body_hash 模式）
  -> dedupe_once(key)            ← §8.4 必须先于 append
  -> runtime.append_content / handoff
  -> cursor_save (成功后)
  -> [optional] await_responds_to
```

每个 emit 的 event 一定带：

```json
{
  "_meta": {
    "service": "scheduler",
    "jobId": "<job_id>",
    "fireTimeUtc": "2026-04-24T09:00:00Z",
    "sourceKind": "command",
    "sourceCursor": "<sha256 hex, only when cursorBy=body_hash>"
  }
}
```

## 5. 状态目录

```text
~/.local/share/joi/service-host/services/<service_id>/
  cursors/<job_id>.json   # body_hash 模式下的上次 sha256
  dedupe.jsonl            # §8.4 append-only key 表
  logs/                   # 由 host 创建；plugin 暂不直接写
```

cursor / dedupe 都是 connector 私有状态，不进 server journal。删掉
`cursors/<job_id>.json` 等价于"下一次 tick 当成新 body"，删 `dedupe.jsonl`
等价于"重新允许重复"。

## 6. 与 §9 协议路径的关系

- **handoff event** 携带 `_meta.service = scheduler` + `_meta.jobId`，目标
  agent 在自己回复里照协议补 `responds_to -> trigger_event_id`（§9.1
  不变量），server 通过 §9.5 把回复反向投递到 `svc_scheduler` 的 inbox。
- `awaitReply=true` 走的就是 §6.3 `await_responds_to` —— 当前是 polling
  实现（`delivery/list` 每 500ms），latency 满足 watcher 类报告 job；如果
  你的 job 需要秒级 round-trip，等 hybrid drain-then-watch 上线（见
  `docs/service-plugin-system-design.md` §12 S1 已知限制）。
- `singleInFlight` 是 plugin 级语义；server 不感知。多 host 进程并发跑
  同一个 ServiceSpec 时 §9.4 connection lease 会让后来的抢占前者，等价
  于 in-flight 状态被自动清掉。

## 7. 不适合 scheduler 的任务

§8.2 列了一遍，挑两个最容易踩的：

- **journal compact / artifact GC** —— server 内部维护任务，应该跟 server
  生命周期一起跑，不是 connector。
- **delivery retry** —— §9.2 inbox 自带重试语义，scheduler 跑这个会和
  server 的 receipt/record 互相干扰。

## 8. 常见 cron pattern

| 用途 | cron |
| --- | --- |
| 每 5 分钟一次 | `*/5 * * * *` |
| 工作日 9 点 | `0 9 * * 1-5` |
| 每月 1 号 0 点 | `0 0 1 * *` |
| 每周一 9:07 | `7 9 * * 1` |
| 每小时整点 + 30 分 | `0,30 * * * *` |
| 每 3 小时（9-18） | `0 9-18/3 * * *` |

UTC only。需要 timezone 的 job 自己换算 cron（e.g. 北京时间 9 点 →
UTC 1 点 → `0 1 * * *`）。

## 9. 调试

- 临时改成 `*/1 * * * *` + `dryRun` 风格的 source（`echo $(date)` 之类）
  验证链路。
- `tracing` 等级开到 `debug` 看 cursor 命中 / dedupe 命中：
  `RUST_LOG=joi_cli::service::scheduler=debug joi service serve …`。
- 单元测试看 `crates/cli/src/service/scheduler/{cron,source,plugin,spec}.rs`
  的 `mod tests` 部分；调度真值表都在 `cron.rs` 里。
