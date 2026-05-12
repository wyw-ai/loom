# repo-notes —— bundle

频道级 `command` 服务，封装 `a1 kbase repo-stg-*` 系列命令，给 agent 用来
拉取 / 推送 / 校验仓库知识库笔记。

- Kind：`command`（forward-compat：p4a 之前 host 不会启动 command 类，但
  spec 已合法可注册）。
- 子命令：`pull`、`push`、`verify`，分别调 `bundle/{pull,push,verify}.sh`。
- 每个子命令都支持 `--dry-run`，stdout 输出 JSON 状态。

## 离线冒烟

```sh
data/services/repo-notes/bundle/pull.sh --dry-run
data/services/repo-notes/bundle/push.sh --dry-run
data/services/repo-notes/bundle/verify.sh --dry-run
```
