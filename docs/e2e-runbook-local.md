# Local e2e Runbook — `feat/remove-dev-helper`

> 适用范围：dev-helper → Joi 原生迁移在本地的最终验收。
> 远端 cutover (187) **必须** 在本文所有步骤本地全绿后进行——见
> `docs/remove-dev-helper-migration-design.md` §10。

## 前置依赖

- Cargo workspace 已 build：`cargo build --release -p joi -p joi-server`。
  或使用 `bash scripts/e2e/local-up.sh build`。
- `claude` CLI 在 PATH 里（real-LLM 剧情需要）；agent profile
  symlink 指向 `~/.claude/settings-glm.json`（默认配置已挂在
  `data/agents/<id>/profile/claude/settings.json` 上）。
- `jq` 在 PATH 里。
- 端口 7900 可用（`local-up.sh` 默认值；`JOI_E2E_PORT` 可改）。

## 全量回归

一条命令跑全部 5 个 smoke：

```bash
scripts/e2e/run-all.sh
```

只跑合成 smoke（跳 LLM）：

```bash
SKIP_LLM=1 scripts/e2e/run-all.sh
```

退出码 0 = 全绿。任何 smoke 失败时该 smoke 自带日志 tail，
脚本立即停在那里，不继续后面的项。

## 单项 smoke

各项均可独立运行，前提是 `scripts/e2e/local-up.sh start` 已起来。

| 脚本 | 验收点 |
| --- | --- |
| `run-mr-detector.sh` | M3：thread-bound mr-detector 发 `status.update` + `attaches_artifact` 事件 + merged 后 `service.self_complete`，host 删 `request.json`。|
| `run-spec-apply.sh` | M5/O6：从合成 lesson-plan 走 `action.request → action.response → joi spec apply`，断言 `spec.json` 落盘、bundle write 落盘、`agent.config.changed` 携带 `reload_epoch_ms`、receipt artifact 发出。|
| `run-classroom.sh` | M7 leg 1：真 LLM teacher 在 thread 里产 lesson-plan + 发 `action.request`；driver accept → spec apply → 断言同 M5。|
| `run-a1-auto-dev.sh` | M7 leg 2：真 LLM router→discovery→delivery 三跳，driver 用 `joi thread create --bootstrap-artifact` 派生 delivery thread，最后串 thread-bound mr-detector 收尾。|
| `run-thread-closed.sh` | M6：open 钉子的 mr-detector 在 thread 被关闭时被 host 收割（`auto_stop_on=thread.closed`，§4.7.3）。|

> **执行顺序约束**：`run-thread-closed.sh` 重启 harness 用的是 *open*
> fixture（mr-detector 永不自完成）。它必须放在所有期望 *merged*
> fixture 的 smoke 之后；`run-all.sh` 已经按此顺序排好。

## 启停/清理

```bash
scripts/e2e/local-up.sh start   # idempotent；已在跑就跳过
scripts/e2e/local-up.sh nuke    # 停进程 + 删 /tmp/joi-e2e
scripts/e2e/local-up.sh build   # 仅 build
```

## 失败排查

1. **`adapter ready` 之后无任何 actor event**：99% 是 LLM 调用卡住或
   spec.json 配置不对。先 `tail /tmp/joi-e2e/logs/agent-host.log`，
   再 `ps aux | grep "claude --print"` 看 LLM 进程是否在跑。
2. **`spec apply` 报 "spec.json not found"**：`JOI_AGENT_SPECS` 没
   被 driver 看到；确认 `local-up.sh` 已 source 了 env。
3. **mr-detector 不发 `service.self_complete`**：`MR_DETECTOR_FETCH_CMD`
   指向 open 而不是 merged fixture。`run-thread-closed.sh` 会切换到
   open，跑完应重新 `local-up.sh nuke && start`。
4. **router→discovery 永远超时**：检查 channel members 是否包含
   `actor_router` / `actor_discovery`；`run-a1-auto-dev.sh` 第 1 步
   会做 invite，但只在脚本自己创建的 channel 里有效。
5. **GLM-5.1 不按指示走**：driver prompt 用字面量 + JSONFENCE 占位符
   降低翻译损耗。如果模型仍然偏，先在 `~/.claude/settings-glm.json`
   把 `ANTHROPIC_DEFAULT_SONNET_MODEL` 切到更强的模型再重跑。

## 进入 187 cutover 前的 checklist

- [ ] `scripts/e2e/run-all.sh` 在最近一次 `cargo build --release` 后绿。
- [ ] `git status` 干净（所有新增脚本、spec.json restructure 都已落盘并 commit）。
- [ ] 在 187 上做的事仅限：拉新分支 → `cargo build --release` → 替换二进制 →
      停旧 dev-helper / dev-helper-bridge 进程 → 启动 `joi-server` /
      `joi agent serve` / `joi service serve` → 跑 1.4b/1.4c 同款剧情。
- [ ] 不在 187 上跑 `cargo test`（产物会污染目录）。
