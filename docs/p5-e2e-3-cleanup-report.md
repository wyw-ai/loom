# p5-e2e/3 — 收尾 + prod 新 channel 报告

继 `p5-e2e/2` 187 cutover 完成（commit `2f358b0`）后，3 阶段做代码侧收尾、
prod 启用 dev-helper 替代后的目标 channel，以及备份清理观察项。

## 3a 代码侧收尾（commit `05ec4bf`）

| 项 | 处理 |
| --- | --- |
| O5 `joi service start --params` ↔ `ServiceSpec.params_schema` | `crates/cli/src/cmd/service.rs` 的 `start` 路径在 JSON 解析后调用新增的 `validate_params(&value, schema)`，做 JSON-schema 子集校验：`required[]` 必填检查 + 每个 property 的 `type` 校验（string / number / boolean / object / array / null）。schema 缺省或为空则透传。新增 5 个单测覆盖 missing-required / type-mismatch / 多 required / 无 schema pass-through。 |
| N2 mr-detector 死字段 | `data/services/mr-detector/spec.json` 中 `config.self_complete_on` 三行删除。该字段不被任何代码读取，实际 self-complete 由 `bundle/poll.sh` 在 merged 转换时写 sentinel + scheduler 在 receipt artifact 后关 thread 完成；保留只会误导未来读 spec 的人。 |
| O3 ALLOWED_PREFIXES 白名单 | `crates/cli/tests/no_legacy_refs.rs` 全文审计：白名单条目都是必要保留——设计文档反向引用 + `migrate-dev-helper` 工具自身需要在源码中提到 dev-helper。无可删除项。 |

回归：本地 `scripts/e2e/run-mr-detector.sh` PASS（确认 N2 无破坏 + O5 路径
继续 happy）。

## 3b prod 新 channel 拓扑

187 上 `joi agent serve --allow-actors` 扩展为
`classmaster,teacher,router,discovery,delivery,bug-triage`（6/13 specs 加载）。
6 个 agent 全部 `[connected to ws://127.0.0.1:7878/rpc as Agent]`。

| channel id | title | member actors | resident threads |
| --- | --- | --- | --- |
| `chan_31f8fa85d909` | a1-dev-canfeng | canfeng (`actor_human_0240d58e`), router, discovery, delivery, bug-triage | `thread_9233aa879002` resident_as=router；`thread_d7351e218562` resident_as=discovery |
| `chan_4a634872b6f8` | classroom | canfeng, classmaster, teacher | `thread_ac96f52872ce` (greeting) |

设计意图：

- **a1-dev-canfeng** 替代旧 `chan_bcf8e1e730bd` (a1-auto-dev-old)，承载
  router → discovery → delivery + per-thread mr-detector 完整闭环以及缺陷分流；
  router / discovery 走 §4.7.1 resident_threads 模式，router 通过角色名寻址
  discovery 而非硬编码 actor id。
- **classroom** 替代旧 `chan_68b967d1627f` (学习小课堂-old)，专门承载
  classmaster 收题 → teacher 自跑批 → lesson-plan artifact (`spec_apply` 块) →
  `approval.spec_apply` → `joi spec apply` 改写其它 agent spec → reload 的
  「actor 迭代车间」剧情。

旧 channel 暂留（已被服务端重命名为 `*-old`），由 canfeng 在新 channel 稳定后
自行决定 retire 时机。

### Smoke

不主动发 `joi say` hello 触发真实 LLM dispatch（避免烧 prod token）。
smoke 通过条件取以下三项联合签收：

1. `joi channel members` 列出预期 actor 拓扑；
2. `joi thread list --channel` 列出 resident + greeting threads；
3. `~/logs/joi-agent-serve-multi.log` 显示 6 个 agent 全部 `[connected]`，
   `~/logs/joi-server.cutover3a.log` 无 ERROR / panic。

## 3c 备份与可清理项

观察一周后可删（一周内若新 channel 无回归再清理）：

- `~/.local/bin/joi{,-server}.cutover-20260503-231701`（Phase 5 baseline 二进制）
- `~/.local/bin/joi{,-server}.cutover3a-20260503-233453`（3a 重启前快照）
- `~/joi-apps/data.cutover-20260503-231701`（Phase 5 baseline 数据 19MB）
- 187 worktree `~/joi-apps-e2e`（branch `e2e-cutover` tracking
  `origin/feat/remove-dev-helper`）— `git worktree remove ~/joi-apps-e2e`

清理命令草稿（不在本阶段执行）：

```bash
ssh canfeng@11.158.213.187 '
rm -f ~/.local/bin/joi.cutover-20260503-231701
rm -f ~/.local/bin/joi-server.cutover-20260503-231701
rm -f ~/.local/bin/joi.cutover3a-20260503-233453
rm -f ~/.local/bin/joi-server.cutover3a-20260503-233453
rm -rf ~/joi-apps/data.cutover-20260503-231701
cd ~/canfeng-projects/joi/joi-apps && git worktree remove ~/joi-apps-e2e
'
```

## 整体收尾

- branch `feat/remove-dev-helper` 本地 5/5 e2e 绿，187 prod 已 cutover 并
  扩展 actor 集，新 channel 拓扑就绪。
- dev-helper → Joi 原生迁移（actor / channel / thread / event / workspace /
  artifact / action / agent / service 复用，无新增概念）目标达成。
- 后续如需把 `feedback-fix-orchestrator` / `feedback-scanner` 等 service 也
  接入 a1-dev-canfeng（用户口中"bug 分析的这几个 agent"），通过扩展
  `--allow-actors` + `joi channel invite` 即可，无需改代码。
