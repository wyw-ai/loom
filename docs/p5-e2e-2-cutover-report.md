# p5-e2e/2 — 187 cutover 报告

dev-helper → Joi 原生迁移分支 `feat/remove-dev-helper`（HEAD `c525198`）已在 187
完成 cutover。

## 时序

1. **平行 harness 验证（无侵入）**
   - 在 187 worktree `~/joi-apps-e2e` checkout `feat/remove-dev-helper`，
     `cargo build --release -p joi-cli -p joi-server`（2m10s）。
   - 用 `scripts/e2e/run-all.sh` 在端口 7900 / `/tmp/joi-e2e/*` 跑全部 5 条
     smoke：mr-detector → spec-apply → classroom (real LLM) →
     a1-auto-dev (real LLM 3-hop chain) → thread-closed。
   - **5/5 PASS**，prod 7878 实例完全不受影响。

2. **修复 Linux GNU coreutils mktemp 兼容**
   - macOS BSD `mktemp -t prefix` 自动加随机后缀；Linux GNU 要求 X-template。
   - commit `c525198`: 全部 e2e driver 改用 `mktemp /tmp/<prefix>.XXXXXX`。

3. **数据兼容性核查**
   - 把 prod `~/joi-apps/data`（19MB, 22971 条 journal 行）copy 到
     `/tmp/prod-data-snapshot`，新 `joi-server` bind 7902 加载，RPC
     `joi channel list` 列出 4 个 prod channels（学习小课堂 / a1-auto-dev /
     A1-Dev-Xingchu / Agent-run），无 schema 不兼容。

4. **prod cutover**
   - 备份 `~/.local/bin/joi{,-server}` → `*.cutover-20260503-231701`
   - 备份 `~/joi-apps/data` → `~/joi-apps/data.cutover-20260503-231701`
   - 停掉旧 prod processes（pid 3586125 joi-server / 2034985 + 3803207
     joi agent serve）
   - 拷贝 `~/joi-apps-e2e/target/release/{joi,joi-server}` 到
     `~/.local/bin/`
   - 重启 joi-server（同参数：`--bind 0.0.0.0:7878 --data-dir
     ~/joi-apps/data`）
   - 重启 joi agent serve（同参数：`--server ws://127.0.0.1:7878/rpc
     --specs ~/.config/joi/agents --allow-actors classmaster,teacher`）
   - 4 个 prod channels 仍可列出；classmaster + teacher actor connection
     建立成功。

## 验收

- 187 平行 harness 双剧情真 LLM e2e 全绿。
- prod 7878 server 切到新二进制后启动 OK，channel 列表完整，actor
  连接重建成功。
- prod data-dir 在新二进制下 journal 加载成功，未触发任何 migration。

## 回滚

```bash
# stop new prod
pgrep -af "joi-server.*0.0.0.0:7878" | awk '{print $1}' | xargs -I{} kill {}
pgrep -af "joi agent serve.*ws://127.0.0.1:7878" | awk '{print $1}' | xargs -I{} kill {}

# restore old binaries
cp ~/.local/bin/joi.cutover-20260503-231701 ~/.local/bin/joi
cp ~/.local/bin/joi-server.cutover-20260503-231701 ~/.local/bin/joi-server

# (data-dir 没动过，无需还原)

# restart with same args as before
nohup ~/.local/bin/joi-server --bind 0.0.0.0:7878 --data-dir ~/joi-apps/data \
  > ~/logs/joi-server.rollback.log 2>&1 &
nohup ~/.local/bin/joi agent serve --server ws://127.0.0.1:7878/rpc \
  --specs ~/.config/joi/agents --allow-actors classmaster,teacher \
  > ~/logs/joi-agent-serve-classroom.rollback.log 2>&1 &
```

## 留尾（p5-e2e/3）

- ALLOWED_PREFIXES 白名单收敛（O5 之外的硬编码白名单清理）
- O5 service params_schema 校验
- O7 dead config 字段清理
- 187 worktree 在 prod 上线后可删除：`git worktree remove ~/joi-apps-e2e`
- 旧 backup（binaries + data-dir）观察一周后可删除

## p5-e2e/3 后续（已在 commit `05ec4bf` 起完成）

### 3a 代码侧收尾（commit `05ec4bf`）
- O5: `joi service start --params` 现按 `ServiceSpec.params_schema` 做
  required + 简单 type 校验（5 个单测覆盖 missing / wrong-type / 多 required /
  无 schema pass-through 场景）。
- N2: `data/services/mr-detector/spec.json` 中无人读取的 `config.self_complete_on`
  字段删除，避免误导（实际 self-complete 由 `bundle/poll.sh` 写 sentinel 触发）。
- O3: ALLOWED_PREFIXES 白名单全文审计；所有条目都是合法引用（设计文档反向
  引用 + migrate-dev-helper 工具自身），无可删除项。

### 3b prod 新 channel 拓扑

187 上 `joi agent serve --allow-actors` 已扩展为
`classmaster,teacher,router,discovery,delivery,bug-triage`（6 个 actor 全部
`[connected]`）。新建 prod channel：

| channel id | title | member actors | resident threads |
| --- | --- | --- | --- |
| `chan_31f8fa85d909` | a1-dev-canfeng | actor_human_0240d58e (canfeng), router, discovery, delivery, bug-triage | `thread_9233aa879002` resident_as=router；`thread_d7351e218562` resident_as=discovery |
| `chan_4a634872b6f8` | classroom | actor_human_0240d58e (canfeng), classmaster, teacher | `thread_ac96f52872ce` (greeting) |

定位：

- **a1-dev-canfeng**：router 负责分发反馈 / 提需求；discovery 负责调研；
  delivery 负责落地（提 MR + 走 mr-detector 关闭闭环）；bug-triage 负责
  缺陷分流。设计 §4.7.1 的 resident_threads 把 router / discovery 各放在
  自己常驻 thread，方便 router 通过角色名寻址 discovery，复用现有 a1-auto-dev
  完整链路。
- **classroom**：classmaster 收用户出题 / 发任务；teacher 自跑批生成 lesson-plan
  artifact（带 `spec_apply` 块）→ `approval.spec_apply` → human accept →
  `joi spec apply` 改写其它 agent 的 spec → reload。专门作为「迭代其它 actor
  的车间」。

旧 prod channel `chan_bcf8e1e730bd` (a1-auto-dev-old) / `chan_68b967d1627f`
(学习小课堂-old) 不动，由 canfeng 在新 channel 稳定后自行 retire。

### 3c smoke 与日志

- 6 个 agent 全部 `[connected to ws://127.0.0.1:7878/rpc as Agent]`；
  joi-server 日志 INFO 行 `long-lived host connection preempting existing
  actor_conn binding` 是 agent serve restart 的正常现象，不是错误。
- 出于不烧 prod LLM token 考虑，3b 阶段不做 hello content.add 真实 dispatch
  smoke；连接 + 入会 + 拓扑可见即视为 smoke 通过。

### 关联备份与可清理项

- `~/.local/bin/joi{,-server}.cutover-20260503-231701` — Phase 5 baseline 二进制
- `~/.local/bin/joi{,-server}.cutover3a-20260503-233453` — 3a 重启前备份
- `~/joi-apps/data.cutover-20260503-231701` — Phase 5 baseline 数据快照
- 187 worktree `~/joi-apps-e2e`（branch `e2e-cutover` tracking
  `origin/feat/remove-dev-helper`）

以上观察一周后可删除。
