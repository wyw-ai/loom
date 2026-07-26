---
name: local-launch
description: 在本地启动一套可用的 Loom 环境(server + daemon + agent),并验证它能真正跑通。当用户说"本地起一下 loom"、"启动 loom 环境"、"跑个 demo"、"把 server/daemon 拉起来"、"本地测试 loom",或提到 docker compose、loom-server、loom-daemon、agent 冒烟测试时使用本技能——即使用户没有明确说"本地启动"四个字。
---

# 本地启动 Loom

目标:从零到"agent 能在频道里回消息"的可工作状态,而不是仅仅把进程拉起来。

## 两条路径,先选一条

**路径 A:Docker(推荐)** — 一键拉起 server + daemon,agent CLI 烤在镜像里,环境干净可复现。需要 Docker 可用,以及一个已发布的 release(镜像通过 release 的 install.sh 下载 loom 二进制)。

**路径 B:源码构建** — 改代码调试用。不需要 Docker,但要 Rust toolchain,Windows 下还要 GNU make。

不确定时先问用户一句:是要"快速跑起来用"(A)还是"改完代码本地验证"(B)。

## 路径 A:Docker 启动

1. 前提检查:`docker version` 和 `docker compose version` 必须可用。没有就停下来告诉用户。
2. 配置:`cp .env.example .env`,填入:
   - `AGENTS`(逗号分隔,如 `claude,codex`)— 填什么,daemon 里就有什么命令
   - 对应的 API key(如 `ANTHROPIC_API_KEY`);用挂载配置的(claude settings.json 之类)可以不填 key,见"配置挂载"一节
3. 构建并拉起:`docker compose up -d --build`
4. 镜像下载 release 的 URL 由 build args 控制:`LOOM_REPO`(默认 `wyw-ai/loom`)、`LOOM_VERSION`(默认 `latest`)。**如果目标仓库是私有的,匿名下载会 404** — 这时先用有权限的方式(如 `gh release download`)把 install.sh、对应平台的 tar.gz 和 SHA256SUMS 取回,放进构建上下文,然后把 Dockerfile 里的下载步骤临时换成 `COPY` + `install.sh --package-dir <资产目录>`;校验逻辑不变,资产真实性仍由 SHA-256 保证。

## 路径 B:源码构建启动

```bash
make build
export PATH="$PWD/target/debug:$PATH"
loom-server --bind 127.0.0.1:7878   # 终端 1
loom who                            # 终端 2,首次运行生成 ~/.loom/cli.toml
loom channel create --title general
loom chat
```

Windows 上没有 make 就用 cargo 直接构建,见 `docs/windows-build-guide.md`。

## 验证(必做,不要跳过)

进程"起来了"不等于"能用"。按顺序验证:

1. `loom who --server ws://127.0.0.1:7878/rpc` — 通,说明 server 正常
2. `loom provider list` — 目标 provider 显示 `detected`(不是 `missing`)
3. 创建 agent:`loom machine agent create --machine local --provider claude --name <名字> --model <模型> --instructions "..."`
4. 建频道并邀请 agent:`loom channel create --title t` → `loom channel invite <chan> <actor_id>`
5. 发问:`loom message ask @<actor_id> --target "#<chan>" --text "What is 2+2?"`
6. **等 60-90 秒**(adapter 冷启动 30-60s + 模型延迟),然后**读 thread**:`loom message read --target "#<chan>:<msg_id>"`

注意:**agent 的回复在 thread 里,不在频道主时间线**。这是 loom 的设计,不是 bug——读 `#chan:msg_id` 才能看到回复。

## 已知坑(都是实测踩过的,按命中率排序)

1. **端口被占**:宿主机 7878 常有开发中的 loom-server。不要杀用户的进程,改 compose 端口映射(如 `127.0.0.1:17878:7878`)。注意 compose 多个文件的 `ports` 是追加合并,要用 `!override` 才能替换。
2. **claude 拒 root**:`--dangerously-skip-permissions cannot be used with root`。daemon 镜像已改为 `node` 用户(uid 1000);挂载配置目录的路径是 `/home/node/.claude`,不是 `/root/.claude`。
3. **缺 onboarding 文件**:claude-code 没有 `~/.claude.json` 会输出警告而不干活。`claude.sh` 的 `agent_configure()` 会自动写入 `{"hasCompletedOnboarding":true}`,entrypoint 每次启动都会执行 configure。
4. **旧数据卷属主**:命名卷如果被 root 时代的容器写过,换非 root 用户后 worker 报 `Permission denied (os error 13)`。修法和预防:`docker run --rm -v <卷名>:/data alpine chown -R 1000:1000 /data`。
5. **挂载目录被污染**:同一个配置目录被 root 容器写过后,里面会留下 root 拥有的 `backups/`、`projects/`、`sessions/`,node 容器写不进。修复:`sudo chown -R 1000:1000 <目录>`。建议挂载用**副本**而不是生产 `~/.claude`。
6. **agent spec 不持久**:spec 在容器本地的 `/home/node/.loom/agents`,容器重建就丢,`machine list` 显示 `agents=0` 就是这个原因——重建 agent 即可。
7. **凭证分两类**:API key 类走 `.env`;OAuth/配置文件类(settings.json)走挂载。token 无效(401)和配额耗尽(429)都会在 agent 回复里体现为 "Agent run failed",先裸跑 `claude -p "hi"` 在容器里定位是配置问题还是 loom 问题。

## 配置挂载(以 claude + 第三方端点为例)

settings.json 里 `env` 块的 `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / 模型映射会被 claude-code 读取。两种注入方式都验证过:

- **挂载**:compose override 里加 `- /path/to/config-copy:/home/node/.claude`
- **环境变量**:把 settings.json 的 `env` 块转成 `KEY=VALUE` 写进 `.env.claude`,compose `env_file` 加上它

## 清理

测试完问用户是否要清理:`docker compose down -v`(连卷一起删)并删除测试目录。不要默认清理——环境可能还要用。
