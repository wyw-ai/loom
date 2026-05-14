---
name: a1-dev-actor-optimizer
description: Optimize, debug, and evolve the a1-dev-canfeng multi-agent actors and services. Use this skill whenever the user asks to adjust router/discovery/delivery/examiner/mr-watcher/bug-fix-loop behavior, fix handoff loops, improve MR review quality, reduce channel noise, change approve/merge gates, sync actor profiles to 187, or diagnose why a task thread is stuck. Prefer this skill even when the user only says an actor "处理错了", "卡住了", "公共频道太吵", or "这个链路应该怎么走".
---

# a1-dev Actor Optimizer

Use this skill to change the `a1-dev-canfeng` actor system without making the workflow more chaotic. The goal is not just to patch one prompt; it is to preserve clear ownership, high-quality review, and a quiet public channel.

## Working Model

Keep this mental model stable:

```text
discovery 出题
examiner 判题
delivery 做题
mr-watcher 报事实
router 管状态
bug-fix-loop 管队列
```

Do not blur these boundaries to solve a local symptom. If a change seems to require crossing a boundary, either route it through the correct gate or make the boundary change explicit in the actor profiles and docs.

## First Move: Inspect The Effective System

Before editing, inspect both local files and the remote effective runtime when the user is asking about live behavior.

Local sources usually live under:

```text
data/agents/router/profile/
data/agents/discovery/profile/
data/agents/delivery/profile/
data/agents/examiner/profile/
data/services/mr-watcher/spec.json
data/services/a1-bug-fix-loop/
docs/a1-dev-canfeng-final-actors.md
docs/examiner-actor-design.md
```

Remote live host:

```text
ssh canfeng@11.158.213.187
```

Remote effective actor profiles normally live under:

```text
/home/canfeng/joi-apps/data/agents/
/home/canfeng/.agentx/machines/ws_dd43dfa2/actor_human_368136/canfeng_s_workhome_2cc53184/agents/
```

Remote MR watcher runtime:

```text
/home/canfeng/joi-apps/data/runtime-tools/joi-auto-dev/adapters/mr-watcher/poll.py
```

When the user says “现在生效的是不是这样”, “thread 卡住了”, or “为什么没触发”, check live events, MR status, watcher process, and daemon logs before proposing changes.

## Actor Boundaries

### router

Router owns state transitions and human-facing summaries. It does not judge code quality and does not write code.

Router may:

- Start `spec_review`, `mr_review`, `design_review`, and `terminal_review`.
- Perform MR approve when policy allows and only with the dedicated examiner a1 config.
- Escalate human/platform gates.
- Archive terminal task threads.

Router must not:

- Directly fix code.
- Treat delivery no-op/ack as progress.
- Merge MR. Router currently has no merge permission.
- Spam the public channel with internal state.

### discovery

Discovery produces the five-piece discovery output and starts delivery only after spec review passes. It does not judge its own work and does not directly launch delivery before `spec_review`.

### delivery

Delivery implements the approved task, pushes code, opens MR, fixes CI/comment issues, and does post-merge feedback closure. It does not change the assignment, self-review, approve, or merge.

### examiner

Examiner is the judge. It reviews discovery outputs, MR quality, design disputes, and terminal decisions. It does not write code, create delivery threads, close feedback directly, or merge MR.

For `mr_review`, examiner must do real code review:

- Read MR diff and relevant surrounding code.
- Use inline MR comments for concrete line-level findings.
- Use `[examiner-result]` only as the structured workflow signal.
- If verdict is `quality_pass`, also add the exact MR comment:

```text
LGTM - actor_examiner
```

### mr-watcher

mr-watcher reports facts: comments, CI, conflicts, readyToMerge, merged/closed, and examiner structured comments. It does not make quality or design judgments.

It should route:

- `examiner-result needs_changes/action_target=delivery` to delivery.
- `examiner-result quality_pass/action_target=none` to router approve/merge gate.
- `LGTM - actor_examiner` as examiner self marker, not as a delivery action.
- CI/conflict/comment issues to delivery only when there is a real executable action.

## Gate Semantics

### spec_review

Runs after discovery publishes the five-piece output and before delivery starts.

Ask:

- Is this worth doing?
- Is the target right?
- Is DoD verifiable?
- Is repo scope complete and minimal?

### mr_review

Runs after MR opened and after delivery fixes a previous review/CI issue.

Ask:

- Does the MR satisfy the approved task and DoD?
- Is implementation reasonable?
- Are tests and CI sufficient?
- Are review comments resolved or correctly classified?

Do not mechanically block on every open discussion. Classify discussion first:

- Blocking: code, DoD, test, security, compatibility, release risk, or requirement deviation.
- Non-blocking/platform note: open-ended, administrative, out-of-scope, or not actionable by delivery.

### approve / merge

Keep approve and merge separate:

- `quality_pass`: examiner quality judgment.
- `approve`: router may execute platform approval if needed and permitted.
- `merge`: router does not execute merge; human/platform performs merge.

Approve command must clear proxy and use the dedicated config:

```bash
env -u HTTP_PROXY -u HTTPS_PROXY -u http_proxy -u https_proxy -u ALL_PROXY -u all_proxy \
  A1_CONFIG_DIR=/home/canfeng/.config/a1-examiner \
  a1 repo mr approve <mr_id> --repo <repo>
```

Never use bare `a1 repo mr approve ...`, and never set only `A1_CONFIG_DIR` without clearing proxy.

### design_review

Use this when the issue is not a local code fix: wrong goal, wrong scope, bad DoD, reviewer says the bug is invalid, or implementation reveals a design mismatch.

### terminal_review

Use this for closing/abandoning MR or non-Fixed feedback outcomes. Delivery executes the mechanical close only after the correct gate.

## Channel Hygiene

The public channel is for human-readable project updates, not actor logs.

Allowed public channel messages:

- Human needs to decide or handle a platform/permission gate.
- Task starts.
- MR created.
- Review result matters to human.
- readyToMerge / merged / closed.
- Exceptional escalation.
- Human explicitly asks for status.

Forbidden in the public channel:

- Handoff details.
- no-op / ack / waiting / “本回合结束”.
- Full watcher reports.
- Artifact ids, event ids, raw thread ids, raw note ids.
- Duplicate status notices.

Prefer this shape:

```text
「<任务标题>」<人话状态摘要>。
MR：<url>
```

Use URL when available. URL is good; raw ids are usually not.

Example:

```text
「a1 app cr submit --pipeline-id 同步返回流水线实例 ID 和发布页 URL」代码审查已通过，CI 和审批已通过；当前仅剩一条“Agent 页面更新了吗？”讨论需评论作者或平台侧关闭后才能合并。
MR：https://code.alibaba-inc.com/aone/a1/codereview/27373769
```

Internally keep ids for idempotency:

```text
channel_notice_key=<thread_id>:<state>:<mr_id>:<blocker_signature>
```

If the same key was already reported, stay silent or write only to the task thread.

## Stuck Thread Diagnosis

When a thread is stuck, check these in order:

1. Latest Joi events in the thread.
2. MR status: approval, test, discussion, readyToMerge.
3. Latest MR comments and whether new comments are self markers, examiner results, reviewer requests, or delivery replies.
4. mr-watcher process and state file.
5. Whether router treated a gate as no-op.
6. Whether a platform action failed due to permissions.

Common fixes:

- Missing next review: router should handoff `actor_examiner gate=mr_review`.
- `quality_pass` routed to delivery: fix mr-watcher classification.
- public channel spam: tighten router channel hygiene and add dedupe.
- discussion gate stuck: attempt resolve only if policy permits; if platform says only author can resolve, escalate to author/platform, not delivery.
- approve needed: router approve with `A1_CONFIG_DIR=/home/canfeng/.config/a1-examiner`.
- merge needed: router reports ready; human/platform merges.

## Editing Workflow

1. Announce what actor/service boundary you are changing.
2. Patch local source files first.
3. If live behavior matters, sync to 187 effective directories.
4. Restart only the affected process when needed; for daemon restarts, preserve the Node/npm PATH rules below.
5. Verify with one live event, MR status, watcher one-shot, or daemon log.
6. Update docs when the behavior changes, especially `docs/a1-dev-canfeng-final-actors.md`.

Use `apply_patch` for local edits. For remote runtime-only scripts like `poll.py`, patch carefully and run `python3 -m py_compile` before restart.

## Node/npm Provider Deployment

This is a hard rule for all Node/npm-based providers, including `codex`, `copilot`, `claude`, and wrapper scripts that eventually execute npm-installed CLIs.

The daemon is often started from a non-interactive shell, cron-like environment, or service wrapper. Do not rely on login shell PATH, nvm hooks, or `~/.zshrc`. On 187, a bad restart can make `node` resolve to `/usr/bin/node v12.22.9` while `codex` points at a Node 24 npm package, causing errors like:

```text
SyntaxError: Unexpected reserved word
const childResult = await new Promise(...)
```

### Actor spec rule

For every actor spec using an npm-installed CLI, make the transport command self-contained. If the command also sources proxy setup, source proxy first, then put the required Node bin directories at the front of `PATH` before running the CLI. This order matters: on 187, `~/open-proxy` can reset/reorder `PATH`, so `export PATH=Node24...; source ~/open-proxy; exec codex ...` is still broken.

```json
"args": [
  "-lc",
  "source ~/open-proxy >/dev/null; export PATH=/home/canfeng/canfeng-projects/.data/.nvm/versions/node/v24.14.1/bin:/home/canfeng/.nvm/versions/node/v24.14.1/bin:/home/canfeng/.local/bin:$PATH; exec codex exec --skip-git-repo-check --json --sandbox danger-full-access -c sandbox_workspace_write.network_access=true --add-dir /home/canfeng/.joi-apps \"$@\"",
  "joi-codex"
]
```

Use the same pattern for Copilot/Claude wrappers if they are npm-provided or depend on npm-provided node binaries. The exact CLI can differ, but the PATH prefix must be in the actor spec, not only in the daemon startup command.

Wrong order example:

```bash
export PATH=/home/canfeng/canfeng-projects/.data/.nvm/versions/node/v24.14.1/bin:$PATH
source ~/open-proxy >/dev/null
exec codex ...
```

This can still fail with Node v12 because `open-proxy` may rewrite PATH after the export. Correct order:

```bash
source ~/open-proxy >/dev/null
export PATH=/home/canfeng/canfeng-projects/.data/.nvm/versions/node/v24.14.1/bin:/home/canfeng/.nvm/versions/node/v24.14.1/bin:/home/canfeng/.local/bin:$PATH
exec codex ...
```

### Daemon restart rule

When restarting the daemon manually on 187, use a login bash wrapper so normal PATH is available for provider discovery:

```bash
cd /home/canfeng/joi-apps
nohup bash -lc 'cd /home/canfeng/joi-apps && exec ./joi daemon --server ws://11.158.213.187:7878/rpc' \
  > /tmp/joi-daemon-restart.log 2>&1 < /dev/null &
```

Do not start it with a plain non-login `nohup ./joi daemon ...` unless you have explicitly exported the provider PATH first.

### Required deployment checks

After every actor/provider deployment or daemon restart, run all three checks:

```bash
ssh canfeng@11.158.213.187 'bash -lc "command -v node; node -v; command -v codex || true; command -v copilot || true; command -v claude || true"'
ssh canfeng@11.158.213.187 'source ~/open-proxy >/dev/null; export PATH=/home/canfeng/canfeng-projects/.data/.nvm/versions/node/v24.14.1/bin:/home/canfeng/.nvm/versions/node/v24.14.1/bin:/home/canfeng/.local/bin:$PATH; node -v; codex --version'
ssh canfeng@11.158.213.187 'tail -80 /tmp/joi-daemon-restart.log | grep -E "skipping .* provider|connected to|actor_examiner|canfeng-codex"'
ssh canfeng@11.158.213.187 'cd /home/canfeng/joi-apps && ./joi actor list | grep -E "actor_examiner|actor_router|actor_delivery|actor_discovery|canfeng-codex"'
```

If daemon logs contain `skipping actor_examiner because provider codex is not available on PATH`, the deployment is not valid even if files were synced correctly.

For Codex actors, also run a minimal handoff healthcheck after restart:

```bash
joi handoff --as actor_router --in <thread_id> actor_examiner -m "healthcheck: 请只回复 OK，不运行任何命令。"
```

Do not start a real MR review until the healthcheck produces a normal actor response instead of `Agent run failed`.

## Remote Sync Patterns

For actor profiles, sync both repo and effective machine directories:

```bash
tar cf - data/agents/router/profile | ssh canfeng@11.158.213.187 'cd /home/canfeng/joi-apps && tar xf - && base=/home/canfeng/.agentx/machines/ws_dd43dfa2/actor_human_368136/canfeng_s_workhome_2cc53184/agents && for a in router actor_router; do mkdir -p "$base/$a/profile"; cp -f /home/canfeng/joi-apps/data/agents/router/profile/identity.md "$base/$a/profile/identity.md"; cp -f /home/canfeng/joi-apps/data/agents/router/profile/soul.md "$base/$a/profile/soul.md"; done'
```

Adjust `router actor_router` to the target actor pair:

```text
discovery actor_discovery
delivery actor_delivery
examiner actor_examiner
```

For actor specs, sync the repo file and the effective machine spec. Example:

```bash
tar cf - data/agents/examiner/spec.json | ssh canfeng@11.158.213.187 'cd /home/canfeng/joi-apps && tar xf - && root=/home/canfeng/.agentx/machines/ws_dd43dfa2/actor_human_368136/canfeng_s_workhome_2cc53184; cp data/agents/examiner/spec.json "$root/agents/actor_examiner/spec.json"'
```

When changing shared provider behavior, update sibling specs too. For example, a Codex PATH fix should usually touch both `data/agents/examiner/spec.json` and `data/agents/canfeng-codex/spec.json`.

Daemon restart may be required for agent profile reload. Ensure provider PATH includes node v24, claude, codex, and copilot if restarting daemon.

mr-watcher restart should preserve the correct PATH so `joi` is available:

```bash
cd /home/canfeng/joi-apps
export PATH=/home/canfeng/joi-apps:/home/canfeng/.nvm/versions/node/v24.14.1/bin:/home/canfeng/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH
nohup python3 /home/canfeng/joi-apps/data/runtime-tools/joi-auto-dev/adapters/mr-watcher/poll.py >/tmp/mr-watcher.log 2>&1 &
```

## Quality Bar

Before finishing, answer:

- Did this preserve actor boundaries?
- Did it reduce loops rather than add another loop?
- Does the public channel stay quiet?
- Is the MR quality gate independent from delivery?
- Is approve separate from merge?
- Are a1 commands using the correct config?
- Did remote live state actually load the change?
