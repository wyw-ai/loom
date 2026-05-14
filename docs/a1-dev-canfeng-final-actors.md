# a1-dev-canfeng 最终 Actor 分工

> 状态：当前生效方案。本文定义 a1-dev-canfeng 自动化开发闭环的角色边界、状态机和升级规则。

## 目标

搭建一套可持续自动化开发系统：有人出题、有人做题、有人独立判题，MR 和 feedback
终态可追踪，质量问题不会被“自评通过”或并行 handoff 冲掉。

核心原则：

```text
discovery 出题，examiner 判题，delivery 做题，mr-watcher 报事实，router 管状态，bug-fix-loop 管队列。
```

## 角色边界

| 角色 | 做什么 | 不做什么 |
| --- | --- | --- |
| `actor_router` | human 入口、状态机 owner、handoff 调度、升级 human、终态汇报 | 不写代码，不做技术质量审查，不替 examiner 判题 |
| `actor_discovery` | 理解需求/bug，判断缺陷真实性和 repo scope，产出五件套，spec 通过后启动 delivery | 不自审，不直接启动 delivery，不做 MR 质量复核 |
| `actor_examiner` | 审五件套、审 MR、审设计争议、审终态 | 不写代码，不改五件套，不建 thread，不直接改 feedback/MR 状态 |
| `actor_delivery` | 按通过的五件套实现、验证、push、建 MR、处理 CI/comment、回评 feedback | 不改题，不自审，不绕过 examiner，不自行 merge |
| `mr-watcher` | 监听 MR 评论、CI、conflict、readyToMerge、merged/closed、examiner 结构化评论 | 不做质量判断，不裁决设计争议 |
| `a1-bug-fix-loop` | 串行取 bug、维护队列状态、识别终态后推进下一条 | 不判断代码质量，不插手单个 MR 的设计裁决 |
| bug 分流/triage | 扫描、分类、去重、入队 | 不写 DoD，不决定技术方案 |

## 状态机

```text
bug_candidate/new_task
  -> discovery_ready
  -> spec_review_passed
  -> delivery_running
  -> mr_opened
  -> mr_review_running
  -> mr_needs_changes | mr_review_passed | design_dispute | blocked
  -> merge_gate_waiting
  -> merged/closed
  -> feedback_closed
  -> archived
```

状态推进 owner：

| 状态变化 | 允许推进者 |
| --- | --- |
| `bug_candidate/new_task -> discovery_ready` | discovery |
| `discovery_ready -> spec_review_passed` | examiner，经 router 调度 |
| `spec_review_passed -> delivery_running` | discovery，经 router 授权 |
| `delivery_running -> mr_opened` | delivery |
| `mr_opened -> mr_review_running` | router |
| `mr_review_running -> mr_needs_changes` | examiner MR 评论 + mr-watcher |
| `mr_needs_changes -> mr_review_running` | delivery 修完后 handoff router |
| `mr_review_running -> design_dispute` | examiner / delivery / mr-watcher 发现，router 调度 |
| `mr_review_passed -> merge_gate_waiting` | mr-watcher |
| `merge_gate_waiting -> merged/closed` | human / Code 平台事实 |
| `merged/closed -> feedback_closed` | delivery / bug-fix-loop |
| `feedback_closed -> archived` | router / bug-fix-loop |

## 主流程

1. router 收到 human 或 bug-fix-loop 任务，使用 `start-discovery.sh` 创建独立
   discovery thread 并 handoff discovery；不要复用 `discovery-desk` 承载任务细节。
2. discovery 产出三件套并发 `[discovery-ready]`，不启动 delivery。
3. router 启动 `actor_examiner gate=spec_review`。
4. spec 通过后，router 要求 discovery 调 `start-delivery.sh` 启动 delivery。
5. delivery 实现、验证、发 MR，handoff router 并附 `[mr-opened v1]`。
6. router 启动 `actor_examiner gate=mr_review`。MR 审查 handoff 必须带
   `--handoff-prefix $'/review [joi]\n'`，让支持 review mode 的 agent 产品优先进入
   代码审查模式；正文仍从 `gate=mr_review` 开始，spec/design/terminal gate 不加该 prefix。
7. examiner publish artifact，并在 MR 发 `[examiner-result]` 评论；常规结论不 handoff。
8. mr-watcher 扫 MR 评论/CI/reviewer/终态，统一推进 delivery 或等待 human merge。
9. MR merged 后，delivery 或 bug-fix-loop 回评 feedback 并改 Fixed；loop 归档并取下一条。
   bugfix-loop 任务的标准归档集合是 discovery thread、bugfix/loop anchor thread、
   delivery thread，按这个产生顺序归档。
10. terminal archive 必须以真实 CLI 状态为准：历史消息里写过“已归档”不算完成。
    router 处理 `mr.final` 时必须执行或验证 `joi thread archive <thread_id>`；
    active list 仍能查到的 thread 不允许 no-op。

## Discovery Thread Workspace

每个 discovery 任务必须有独立 thread，避免旧任务正文、artifact 和 actor session
污染新任务。独立 thread 仍然需要看见频道级只读大库：

```text
~/joi-workspaces/thread/<discovery_thread_id>/shared/repos
  -> ~/.agentx/channels/<channel_id>/shared/repos
```

router 必须通过 `~/joi-apps/data/runtime-tools/joi-auto-dev/scripts/start-discovery.sh`
创建该 thread 和软链。discovery 只读 `shared/repos` 用于理解仓库结构、历史提交和
分支状态；更新 repo cache 必须回到 router 走 `cache-ctl.sh`。

## Examiner Gate

| gate | 触发 | 结论 |
| --- | --- | --- |
| `spec_review` | discovery `[discovery-ready]` | `pass` / `advisory` / `needs_revision` / `rescope` / `reject` / `human_decision` |
| `mr_review` | MR opened 或 delivery 修完 examiner 意见 | `quality_pass` / `needs_changes` / `blocked` / `design_review_needed` / `reject` |
| `design_review` | design_dispute | `keep_plan` / `revise_dod` / `rescope` / `withdraw_mr` / `human_decision` |
| `terminal_review` | 需要关闭 MR 或 feedback 非 Fixed | `auto_close_mr` / `need_human_close_mr` / `auto_feedback_nonfixed` / `need_human_feedback_terminal` / `rescope_not_close` / `no_terminal_action` |

`mr_review` 最多 20 轮。审查员应持续复核到没有新的可执行问题；第 20 轮仍无法通过时升级 human。

## design_dispute

原则性争议不按发现者划分，统一按事件处理。

来源：

- delivery 发现题目、scope、方案边界不对。
- examiner 在 MR review 发现不是局部实现问题。
- mr-watcher 扫到 reviewer 原则性质疑。

统一处理：

```text
source -> router -> actor_examiner gate=design_review
```

delivery 在 design_review 结论回来前暂停，不继续说服式回复 reviewer。

## MR 硬门禁

以下任一存在时，不能 `quality_pass`：

- `test=false`
- 必需 CI failed
- 阻塞型 discussion unresolved：直接指向代码、DoD、安全、测试、兼容性、发布风险且有明确处理动作
- `readyToMerge=false` 且原因是代码/CI/必需 reviewer/阻塞型 discussion gate

coverage-threshold、历史基线、平台阈值、作者不可自审、开放性 discussion 可以解释责任归属。
非代码阻塞不得机械打回 delivery；应记录为 platform note。代码/CI 硬 gate 红灯不能写成质量通过。
合法动作只有：修绿、升级 human/CI gate、或 terminal/design review 给出裁决。

## MR 代码审查要求

examiner 的 `mr_review` 必须读取 diff 和关键上下文，判断实现是否合理；不能只检查
MR 描述、CI 状态和协作证据。具体代码问题优先发 MR 行级评论：

```bash
A1_CONFIG_DIR=/home/canfeng/.config/a1-examiner a1 repo mr comment create \
  --repo <repo> --mr <mr_id> --file <changed/file> --line <new_line> \
  -m "<问题、影响、建议>"
```

顶层 `[examiner-result]` 只用于结构化结论和 mr-watcher 路由，不替代 code review。

## 仓库研发规范

研发规范的唯一源头是 kbase 74121。每个仓库可以有多篇规范文章，文章名必须匹配：

```text
[<group>/<repo>] <title>
```

discovery/start-delivery 只传 kbase 引用，不复制规范正文。对每个 `mode=worktree`
repo，handoff 给 delivery 的格式是：

```text
- aone/a1: <page-id-1>([aone/a1] 构建与测试命令) <page-id-2>([aone/a1] OpenSpec 开发流程)
```

delivery 在编码每个仓库前必须逐个读取该仓库全部 kbase 规范，保存到 workspace 的
`repo-specs/<group>__<repo>/` 作为本次执行快照，并按规范执行开发、测试、OpenSpec、
commit、MR 描述和 review 回复。缺少规范或规范冲突时，delivery 暂停并 handoff router。

examiner 在 `mr_review` 中必须检查 delivery 是否读取并遵守目标仓库的 kbase 研发规范。
违反规范可以作为 `needs_changes`，但必须引用具体规范标题或 page-id。

## Approve 与 Merge

MR 通过和合并拆开：

- `quality_pass`：examiner 的质量判断。
- `approve`：router 只有在看到当前 MR 的 `[examiner-result] verdict=quality_pass
  action_target=none` 和普通评论 `LGTM - actor_examiner` 后，才可以执行 MR 平台
  approve；若 examiner 最新轮失败、超时或只存在 delivery/reviewer 自述通过，router
  必须重试 examiner 或升级 human 排障，不能 approve。
- `merge`：router 当前没有合并权限，不执行 merge；只在 readyToMerge 后通报 human/平台合并。

approve 必须使用审查官身份并清掉代理：

```bash
env -u HTTP_PROXY -u HTTPS_PROXY -u http_proxy -u https_proxy -u ALL_PROXY -u all_proxy \
  A1_CONFIG_DIR=/home/canfeng/.config/a1-examiner \
  a1 repo mr approve <mr_id> --repo <repo>
```

## Channel 公共区卫生

channel 只放 human 需要知道或处理的摘要，不放 actor 日志。默认静默；只有 human 决策、
任务开始、阶段性结果、异常升级、终态变化才允许发公共区。

公共区文案必须人类可读：

- 使用任务标题、MR 标题、feedback 标题、discussion 摘要和可点击 URL。
- 不展示裸 `thread_id` / `mr_id` / `note_id` / `artifact_id`，除非 human 明确排障。
- 有 MR/workitem/文档/发布页 URL 时尽量贴 URL；URL 是公共区允许展示的追踪入口。
- 同一状态只通报一次；内部用 `channel_notice_key` 去重。
- watcher 扫描报告、handoff/no-op/等待中/本回合结束只留在 thread 或直接静默。

示例：

```text
「a1 app cr submit --pipeline-id 同步返回流水线实例 ID 和发布页 URL」代码审查已通过，CI 和审批已通过；当前仅剩一条“Agent 页面更新了吗？”讨论需评论作者或平台侧关闭后才能合并。
MR：https://code.alibaba-inc.com/aone/a1/codereview/27373769
```

## mr-watcher 补偿

mr-watcher 对同一 MR 的同一 CI 错误签名做幂等扫描；如果错误持续 30 分钟仍未消除，
且没有新的处理结果，就补偿重发一次扫描结果，避免 delivery 卡死。

mr-watcher 只报事实，不做质量裁决。

## 废弃项

`mr-detector` 不再属于当前 a1-dev-canfeng 生效链路。MR 监听统一由
`mr-watcher` 承担；若目录仍存在，只能视为历史残留，不允许新流程引用。
