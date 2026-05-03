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
