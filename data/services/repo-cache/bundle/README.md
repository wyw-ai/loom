# repo-cache —— bundle

频道级 scheduler 服务，负责把 `<channel_ws>/.joi/repos/manifest.json` 里列出的
仓库以裸仓库形式镜像到本地缓存。`joi thread create --bootstrap-artifact` 写入
的 thread mounts 直接通过 `service://repo-cache/cache/<repo_id>` 引用此缓存，
agent serve 在 ensure_scope 时按 mount 用 `git worktree add` / `git clone --shared`
快速展开 thread workspace 内的 `repos/<repo_id>`。

- Kind：`scheduler`
- Cron：`*/15 * * * *`
- Job：`sync-all` 调 `bundle/sync.sh`，每行 stdout 输出一个 JSON 对象描述一个
  仓库的同步结果。
- Cursor：`body_hash`；Dedupe：`payload_hash`。
- Data dir：`<service.data_dir>/cache/<urlencoded(repo_id)>/`（裸仓库）。

## 离线冒烟

```sh
data/services/repo-cache/bundle/sync.sh --dry-run
```

退出码 0；stdout 是逐行 JSON；不联网、不写入磁盘。
