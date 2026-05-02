# repo-provision —— bundle

Thread-bootstrap 触发的 `command` 服务。读 `clone-manifest.json`
（`docs/artifact-contracts.md` §3），用 `git clone --shared` 从 `repo-cache`
的缓存里把每个仓库布置到 `{thread_ws}/repos/<repo_id>/`，并产出
`repo-provision-receipt.json` + 发布 `thread.bootstrapped` 事件让上层 agent
继续。

- Kind：`command`
- Autostart：`false`
- Trigger：`thread.bootstrap`
- 输入：clone manifest（artifact id 由调用方注入）。
- 输出：单一 JSON 对象（receipt），字段含每个仓库的
  `repo_id/ref/from/to/readonly/purpose/sha/status`。

## 离线冒烟

```sh
data/services/repo-provision/bundle/provision.sh --dry-run
```

退出码 0；stdout 一个 JSON 对象；不联网、不创建任何真实仓库目录。
