# `data/services/` —— ServiceSpec 库

Joi 原生的 `ServiceSpec` 集合，通过 `joi service register` / `joi service serve`
注册和运行。每个子目录交付：

- `spec.json` —— 单文件 `ServiceSpec`。可被 `joi service validate <path>`
  验证通过。
- `bundle/*.sh` —— 服务用到的脚本（cron 拉取、手动子命令等）。所有脚本都
  支持 `--dry-run`：纯输出 JSON、不碰文件系统、不联网，便于在 CI / 本机
  做离线冒烟。
- `bundle/README.md` —— 给运维的说明（参数、产出 artifact、依赖）。

Phase 3 交付的服务清单：

| Service id        | Kind        | Lifecycle    | 触发                | 主要产出 |
| --- | --- | --- | --- | --- |
| `repo-cache`      | scheduler   | channel-level | cron `*/15 * * * *` | 仓库镜像（本地缓存） |
| `repo-notes`      | command     | channel-level | 手动子命令           | a1 kbase 仓库笔记 |
| `repo-provision`  | command     | thread-bound  | `thread.bootstrap`   | `repo-provision-receipt.json` + `thread.bootstrapped` |
| `mr-detector`     | scheduler   | thread-bound  | cron + 自我完成      | `mr-event-*.json`（artifact-contracts §6） |

## 约定

- `id` 为 kebab-case；`actor.kind` 必须是 `Service`，`actor.id` 由 host 在
  注册时按 `service_<id>` 生成。
- `bundle.source` 用工作区相对路径；`installMode: copy` 让 host 在注册时
  把 bundle 拷到 `<service.data_dir>/bundle/<version>/`。
- Channel-level 服务读 `<channel_ws>/.joi/repos/manifest.json` 等频道工作区
  状态；thread-bound 服务通过参数（`bind.scope = thread` + `params_schema`）
  接受调用方注入的 thread 上下文。
- thread-bound 服务的"自我完成"协议：在最终回合的 stdout 末尾输出一行
  `{"service.self_complete":true,"reason":"..."}`，host（p4a 落地）会把它转成
  `service.self_complete` 事件并停掉对应实例。
- 产出 artifact 的形状统一对齐 `docs/artifact-contracts.md`。

## 离线冒烟

```sh
# scheduler 类
data/services/repo-cache/bundle/sync.sh --dry-run
data/services/mr-detector/bundle/poll.sh --dry-run

# command 类
data/services/repo-notes/bundle/pull.sh --dry-run
data/services/repo-notes/bundle/push.sh --dry-run
data/services/repo-notes/bundle/verify.sh --dry-run
data/services/repo-provision/bundle/provision.sh --dry-run
```

每条命令都应该退出码 0、stdout 是合法 JSON、不创建任何真实文件。
