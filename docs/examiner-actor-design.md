# a1-dev-canfeng 审查员 Actor 设计

> 状态：已落地为当前方案。最终角色分工和状态机见
> [a1-dev-canfeng-final-actors.md](./a1-dev-canfeng-final-actors.md)。本文保留
> 审查员 actor 的详细设计背景和 gate 说明。

## 1. 结论

新增一个 agent：

```text
actor_examiner
displayName: 审查员
Role: 架构审查员 / 交付验收官 / MR 质量守门人
```

只设一个审查员，不拆成架构审查员和 MR 审查员。审查员通过 `gate` 切换工作模式：

| gate | 触发点 | 核心问题 |
| --- | --- | --- |
| `spec_review` | discovery 产出五件套后 | 这件事该不该干、目标对不对、DoD 能不能验收 |
| `mr_review` | delivery 发起或更新 MR 后 | MR 是否满足目标、DoD、代码质量、测试、CI、评审要求 |
| `design_review` | MR 阶段暴露出方案/目标争议 | 这是实现问题，还是题目、scope、架构方案本身有问题 |
| `terminal_review` | 需要废弃 MR 或关闭 feedback 时 | 是否可以自动终止，还是必须 human 裁决 |

审查员不是第二个 router，也不是新的 delivery。它的定位是：

```text
独立判断 + 证据产出 + 有限状态建议 + 必要时执行受控审查动作
```

## 2. 现有角色审查

当前 `a1-dev-canfeng` 的主链路是：

```text
human -> actor_router -> actor_discovery -> actor_delivery -> actor_router
                                            ^                 |
                                            |                 v
                                      mr-watcher / loop / human gate
```

现有定位整体可行，但存在一个结构性缺口：**出题和判题没有分离**。

| 角色 | 当前职责 | 问题 |
| --- | --- | --- |
| `actor_router` | human 入口、分类、thread 复用、handoff、终态汇报 | 不写代码、不深度审查；适合做状态机，不适合判质量 |
| `actor_discovery` | 需求分析、五件套、delivery thread 创建、workspace provision | 是出题人，又承担部分 MR 复核，存在自我确认倾向 |
| `actor_delivery` | 实现、测试、发 MR、处理 CI/comment、feedback 收口 | 写作业的人，不能做最终判题；但仍应保留具体执行权 |
| `actor_a1_bug_triage` | feedback 分类、归一化、队列分类 | 只做分流，不应介入交付质量审查 |
| `mr-watcher` / `mr-detector` | MR 状态、CI、评论、终态监听 | 是传感器和推进器，不做质量判断 |
| `a1-bug-fix-loop` | 串行推进存量 bug、维护 queue、识别终态后推进下一条 | 是队列/状态执行器，不做架构或代码质量判断 |

因此新增审查员是合理的，但必须同步收敛现有职责：

- discovery 不再是 MR 质量复核的默认 owner。
- router 继续是状态机，但不要承担技术判断。
- delivery 保留代码/MR/feedback 的具体执行动作。
- mr-watcher 保留进度推进和事实监听。
- loop 保留 feedback 队列状态维护。
- examiner 获得“判定权”，并在少数低风险场景获得“受控执行权”。

## 3. 核心原则

### 3.1 一个审查员，多 gate

同一 `actor_examiner` 处理五件套、MR、设计争议和终态建议。原因：

- 架构问题和 MR 问题经常来自同一条事实链。
- 拆成多个审查员会导致升级循环和互相等待。
- 一个审查员可以保持判断尺度一致。
- router 只需要识别一个审查结果来源。

### 3.2 强怀疑，弱阻塞

审查员要有架构师能力和质疑精神，但不能用完美主义拖垮链路：

```text
blocker = 证据明确 + 影响核心目标/架构安全/可验证性/可合并性/可发布性 + 有明确修正路径
```

缺少任何一项，只能降级为 `major`、`advisory` 或 `question`。

### 3.3 状态 owner 清晰

审查员可以直接做部分审查动作，但不能随意接管所有系统状态。

| 事项 | 判断 | 裁决 | 执行 |
| --- | --- | --- | --- |
| 五件套质量 | `actor_examiner` | `actor_router` 按 verdict 调度 | discovery 修订或 delivery 继续 |
| MR 质量通过 | `actor_examiner` | mr-watcher 扫 MR 结构化评论；router 只处理升级 | examiner comment；approve 另设显式 gate，merge 另设 human gate |
| MR 废弃/关闭 | `actor_examiner` | examiner policy 或 router/human gate | `actor_delivery` 执行 close |
| feedback Fixed | MR merged 事实 + post-merge 证据 | loop/delivery 识别 | delivery/loop 回评和改状态 |
| feedback 非 Fixed | `actor_examiner` / delivery / discovery 提出 | examiner policy 或 router/human gate | delivery/loop 回评和改状态 |
| thread archive | router 判断终态 | router | router |

### 3.4 router 管升级，mr-watcher 管 MR 常规推进

审查员不得直接 handoff `actor_discovery` 或 `actor_delivery`。MR 常规审查出口是：

```text
actor_examiner -> MR [examiner-result] comment -> mr-watcher -> actor_delivery
```

升级审查出口是：

```text
actor_examiner -> actor_router
```

这样可以避免：

```text
discovery -> examiner -> discovery -> examiner -> delivery -> examiner -> delivery
```

这类循环。

审查员可以通过 `a1` 对 MR 发结构化评论；approve 默认关闭，只能在 router/human
明确打开 approve gate 后执行。

## 4. 审查员能力模型

审查员需要具备五类能力：

1. **目标判断**：判断需求是否成立、bug 是否真实、目标是否偏离、DoD 是否可验收。
2. **架构判断**：判断模块边界、依赖方向、跨仓契约、兼容性、发布顺序和回滚风险。
3. **代码质量判断**：判断实现是否符合代码风格、错误处理、测试覆盖、安全/权限/性能要求。
4. **MR / CI 判断**：判断 CI 失败、review comment、冲突、readyToMerge、依赖失败的根因。
5. **部署 / 发布判断**：判断配置、版本、服务重启、feature flag、环境依赖和灰度风险。

允许动作：

- 读 Joi thread、artifact、workspace。
- 读代码、MR diff、CI 日志、review comment、部署日志。
- 运行只读或验证命令。
- 发布审查 artifact。
- 通过 `a1 repo mr comment create` 发 MR 评论。
- 在满足策略时执行 `a1 repo mr approve`。

禁止动作：

- 修改业务代码。
- 重写 discovery 五件套。
- 创建 delivery thread。
- 直接 handoff discovery/delivery。
- 在没有证据时阻塞任务。
- 把个人偏好当 blocker。
- 直接 merge MR。

## 5. Gate 设计

### 5.1 `spec_review`

触发：discovery 完成 `task-goal.json`、`definition-of-done.json`、`clone-manifest.json`
并发出 `[discovery-ready]` 之后、启动 delivery 之前。

触发方式：router 收到 discovery 的 `[discovery-ready]` 后，在同一 thread handoff
`actor_examiner gate=spec_review`。不再新增独立 `spec-review-watcher` service。

审查问题：

- 原始需求是否被正确理解。
- 这是 bug、需求、优化、误用还是已有能力。
- 是否应该做，是否有更合适的终态。
- target repo 和 branch 是否合理。
- DoD 是否客观、可执行、可复现。
- 是否缺关键仓库、测试环境、发布/依赖说明。
- 是否存在架构或权限风险。

`spec_review` 是 delivery 启动前硬门禁。未通过时 discovery 只能修订五件套或等待
human/terminal 裁决，不能启动 delivery。

### 5.2 `mr_review`

触发：delivery handoff router 并带 `[mr-opened v1]`，或 delivery 修复完 review/CI 后
请求复审。

审查问题：

- MR diff 是否覆盖原始目标和 DoD。
- 多仓任务是否所有 repo/branch 都覆盖。
- 测试证据是否足够。
- CI 失败是否真实回归、环境问题、依赖问题还是部署问题。
- reviewer 评论是否已合理处理。
- MR 描述、关联 workitem、openspec archive 是否完整。
- 是否存在安全、兼容、发布风险。

`mr_review` 必须是真实代码审查，不只是流程留痕。examiner 必须读取 MR diff 和关键
上下文；对能定位到文件行的具体问题，优先使用 Code MR 行级评论：

```bash
env -u HTTP_PROXY -u HTTPS_PROXY -u http_proxy -u https_proxy -u ALL_PROXY -u all_proxy A1_CONFIG_DIR=/home/canfeng/.config/a1-examiner a1 repo mr comment create \
  --repo <repo> --mr <mr_id> --file <changed/file> --line <new_line> \
  -m "<问题、影响、建议>"
```

`A1_CONFIG_DIR` selects the a1 auth store (`auth.yaml`); `--config` is not an
auth-store selector. For approve/permission questions, always verify the real
platform identity with the same prefix plus `a1 -f json auth whoami` and do not
treat the directory name or an `auth.yaml` `user` label as identity.

MR 顶层 `[examiner-result]` 只作为结构化结论和 mr-watcher 路由信号，不替代行级
code review。没有可执行代码问题时，examiner 应明确说明依据，而不是为了协作留空泛评论。

discussion 需要分类处理。代码、DoD、安全、测试、兼容性、发布风险相关且仍有明确动作的
discussion 可以阻塞；开放性、行政性、超出当前题范围或无明确改动要求的问题只记录为
platform note，不应机械打回 delivery。

`mr_review` 替代当前由 discovery 默认承担的 MR 复核。discovery 仍负责出题和
rescope，但不再是默认判题人。

### 5.3 `design_review`

触发：MR 阶段出现原则性质疑，例如 reviewer 认为“不是 bug”“API 已支持”“方案不合理”，
或 delivery/examiner 发现实现问题根源来自五件套。

审查问题：

- 这是实现问题还是题目问题。
- DoD 是否应修改。
- clone-manifest 是否需要加入/移除仓库。
- 当前 MR 是否还能继续，还是应撤回。
- 是否需要 human/API owner 裁决。

`design_review` 由同一个 `actor_examiner` 完成，不再让 discovery 自己复核自己出的题。

### 5.4 `terminal_review`

触发：需要关闭/废弃 MR，或需要把 feedback 标为非 Fixed 终态。

审查问题：

- 是否有足够证据自动终止。
- 是否需要 human 确认。
- 是否是 wrong scope，需要 rescope 而不是关闭。
- MR 是否应关闭，feedback 是否应 Closed/Won't Fix/already_covered/not_a_bug。

该 gate 的目标是把“以前必须 human 确认”的部分场景策略化自动化，但保留高风险
human gate。

## 6. Verdict 契约

统一输出 `examiner-review-result.v1` artifact。

```json
{
  "schema": "examiner-review-result.v1",
  "schema_version": "1",
  "producer": "actor_examiner",
  "gate": "spec_review",
  "thread_id": "thread_x",
  "channel_id": "chan_31f8fa85d909",
  "task_id": "task_x",
  "verdict": "pass",
  "severity": "pass",
  "confidence": "high",
  "can_continue": true,
  "requires_router_action": false,
  "requires_human_confirmation": false,
  "findings": [
    {
      "id": "ex-1",
      "severity": "blocker",
      "type": "unverifiable_dod",
      "evidence": "DoD 要求验证 app create，但 manifest 未包含 app-center 或 mock-server 验证路径。",
      "impact": "delivery 即使完成代码，也无法证明用户路径被修复。",
      "required_action": "补充验证路径，或 rescope 加入相关仓库/测试环境。"
    }
  ],
  "recommended_next_action": {
    "target": "discovery",
    "action": "revise_spec",
    "message": "请补充可执行 DoD 和必要仓库。"
  },
  "created_at": "2026-05-13T00:00:00Z"
}
```

### 6.1 `spec_review` verdict

| verdict | 含义 | router 动作 |
| --- | --- | --- |
| `pass` | 五件套可交付 | 不打扰 delivery |
| `advisory` | 有建议但不影响主线 | 可转给 delivery 或记录 |
| `needs_revision` | DoD/scope/目标需修订 | handoff discovery 修订，并通知 delivery 暂停关键改动 |
| `rescope` | 仓库或责任域错误 | handoff discovery 重做五件套，必要时新建 delivery |
| `reject` | 不该做或缺陷不成立 | 进入 `terminal_review` 或 human gate |
| `human_decision` | 需要人决策 | channel 摘要并请求 human |

### 6.2 `mr_review` verdict

| verdict | 含义 | router 动作 |
| --- | --- | --- |
| `quality_pass` | 代码质量、DoD 和 Code 平台硬 gate 均通过 | 常规路径不接收；由 MR 评论被 mr-watcher 扫描后进入 CI/reviewer/merge gate |
| `needs_changes` | 需要 delivery 修改 | 常规路径不接收；由 MR 评论被 mr-watcher 扫描后 handoff delivery |
| `blocked` | 缺权限、环境、依赖或外部系统 | 普通阻塞走 MR 评论 + mr-watcher；需要 human/状态机升级才 handoff router |
| `design_review_needed` | 不是局部实现问题 | handoff examiner 进入 `design_review` |
| `reject` | MR 不应继续 | 进入 `terminal_review` |

### 6.3 `design_review` verdict

| verdict | 含义 | router 动作 |
| --- | --- | --- |
| `keep_plan` | 原设计正确，delivery 继续修 | handoff delivery |
| `revise_dod` | DoD 要改 | handoff discovery 修订 |
| `rescope` | 责任仓库/范围要改 | handoff discovery 重做 |
| `withdraw_mr` | 当前 MR 应撤回 | 进入 `terminal_review` |
| `human_decision` | 需要 human/API owner | 请求 human |

### 6.4 `terminal_review` verdict

| verdict | 含义 | 执行策略 |
| --- | --- | --- |
| `auto_close_mr` | 可自动关闭 MR | router handoff delivery 执行 close |
| `need_human_close_mr` | 关闭 MR 需要 human 确认 | router 请求 human |
| `auto_feedback_nonfixed` | 可自动把 feedback 收为非 Fixed | router handoff delivery/loop 执行 |
| `need_human_feedback_terminal` | feedback 终态需要 human 确认 | router 请求 human |
| `rescope_not_close` | 问题真实但 scope 错，不能关闭 | handoff discovery 重做 |
| `no_terminal_action` | 证据不足，不终止 | 回到 delivery/discovery |

## 7. 通过、废弃、feedback 状态策略

### 7.1 MR 通过

MR 通过拆成四层，不混用：

| 层级 | 含义 | owner |
| --- | --- | --- |
| `quality_pass` | 审查员确认 MR 满足目标、DoD、代码质量 | examiner |
| `approve` | 在 MR 平台上 approve | router 在 examiner `quality_pass` 后执行；失败则请求有效 reviewer/human |
| `merge_ready` | 已具备合并条件 | router 汇报，human 或 merge policy 决定 |
| `merged` | MR 真实合并 | mr-watcher / status API 事实 |

第一阶段建议：

- examiner 可以发“审查通过”评论。
- router 可以在 examiner `quality_pass` 后执行 approve。
- examiner 不执行 merge。
- router 不执行 merge。
- merge 仍为 human gate；未来可新增 `merge-service`，按白名单和策略执行。

### 7.2 MR 废弃/关闭

以前 MR close 基本要求 human 明确确认。未来允许自动判断，但必须满足低风险策略。

允许 `auto_close_mr` 的条件：

- MR 是 agent 自己创建的，且未被 human 明确要求保留。
- MR 无 reviewer 已通过/无合并中状态/无生产发布依赖。
- 关闭原因属于以下之一：
  - `wrong_scope_superseded`：scope 修正后已有新 MR/thread 接替。
  - `duplicate_mr`：重复 MR，另一个 MR 覆盖同一目标。
  - `obsolete_before_review`：未进入有效 review，任务已被新五件套替代。
  - `not_a_bug_confirmed`：证据明确不是 bug，且无代码价值。
  - `already_covered_confirmed`：当前版本已有能力覆盖，且 MR 无独立价值。
- examiner 给出证据和关闭说明。

必须 human 确认的条件：

- human 明确要求继续或观察。
- reviewer 已给过 approve，或 MR 已 readyToMerge。
- MR 包含可能有独立价值的改动。
- 关闭会影响外部承诺、发布计划或多仓依赖。
- 证据只是推断，不是事实。

执行仍由 delivery 做：

```text
router -> delivery: [examiner-close-approved] repo=<repo> mr_id=<id> reason=<...>
delivery -> a1 repo mr close ...
```

### 7.3 feedback Fixed

Fixed 仍以事实为准：

- MR merged。
- delivery 已回评 feedback。
- status API 更新成功，或 loop 能可靠补偿。

examiner 不直接标 Fixed。它可以在 `mr_review` 中给 `quality_pass`，但不能把未合并
MR 对应 feedback 标 Fixed。

### 7.4 feedback 非 Fixed 终态

允许自动 `auto_feedback_nonfixed` 的条件：

- 结论来自 `terminal_review`，且证据充分。
- 不存在真实跨仓问题；如果存在，应 `rescope_not_close`。
- outcome 属于：
  - `not_a_bug`
  - `already_covered`
  - `not_reproduced`
  - `duplicate`
  - `withdrawn`
- 已有 MR 时，MR close 策略也满足自动关闭，或 MR 已外部 closed。

必须 human 确认的条件：

- 用户反馈可能仍有真实影响但缺数据。
- 关闭会让存量 bug 队列跳过未解决问题。
- reviewer/human 对结论有明确异议。
- outcome 需要业务 owner 裁决。

执行建议：

- 具体回评和状态更新仍由 delivery 或 `a1-bug-fix-loop` 执行。
- loop 是 queue ledger owner，负责 `fix_status`、`closed_at`、`archived_at` 等队列字段。
- delivery 可作为 Aone workitem 的实际写入者，写入后 handoff router；loop 读取终态后推进下一条。

## 8. Handoff 防循环规则

1. examiner 不直接 handoff discovery/delivery；`mr_review` 常规 verdict 只发 artifact + MR `[examiner-result]` 评论。
2. service 只触发 examiner，不触发 discovery/delivery。
3. mr-watcher 是 MR 常规推进入口；router 是升级状态机 owner。
4. 每个 gate 有最大轮次：

```json
{
  "max_examiner_rounds": {
    "spec_review": 2,
    "mr_review": 20,
    "design_review": 2,
    "terminal_review": 1
  }
}
```

5. `mr_review` 应持续复核到没有新的可执行问题为止；超过 20 轮后 router 必须请求 human，而不是继续循环。
6. `advisory` 不阻塞 delivery。
7. `needs_changes` 只允许通过 MR 评论由 mr-watcher 指向 delivery；`rescope` / `revise_dod` 只允许由 router 指向 discovery。
8. 若 examiner 输出缺少 `evidence` 或 `impact`，router 不得按 blocker 处理。
9. 如果 Code 平台 `test=false` / CI failed，examiner 不得输出 `quality_pass`；discussion unresolved / readyToMerge=false 必须拆因，只有代码、DoD、安全、测试、兼容性、发布风险相关的阻塞项才挡质量结论。开放性或非代码问题记录为 platform note，不能机械打回 delivery。

## 9. 与现有 actor 的迁移

### 9.1 router

新增职责：

- 识别 examiner verdict。
- 按有限状态转移调度升级型 discovery/delivery/human；不接管 `mr_review` 常规推进。
- 维护每个 thread 的 examiner round count。
- 在 channel 汇报升级型审查结论。

迁出职责：

- 不再让 discovery 默认做 MR 质量复核。
- 不自行解释复杂技术争议。

### 9.2 discovery

保留：

- 需求发现、五件套、clone-manifest、delivery thread 创建/provision。
- rescope 后重做五件套。

迁出：

- 默认 MR 复核。
- reviewer 原则性质疑的最终裁决。

新增：

- 收到 examiner `needs_revision` / `rescope` 后修订五件套。
- 修订时引用 examiner artifact，说明采纳/不采纳理由。

### 9.3 delivery

保留：

- 实现、测试、push、MR、CI/comment 处理。
- human/examiner policy 批准后的 MR close。
- feedback 回评和状态更新的具体执行。

新增：

- 处理 examiner `needs_changes`。
- 执行 `[examiner-close-approved]`。
- 当发现题目问题时 handoff router，请求 examiner `design_review`。

迁出：

- 不再让 delivery 自己判断 MR 已质量通过。
- 不再在原则性争议中继续说服式推进。

### 9.4 mr-watcher / mr-detector

保留：

- MR 状态、CI、评论、conflict、merged/closed 事实监听。
- 推进 delivery 处理普通评论、CI 和 examiner 的 MR 结构化评论。

新增：

- 扫描 `[examiner-result]`，把 `needs_changes` / 普通 `blocked` 作为唯一常规入口推给 delivery。
- 当 watcher 发现 readyToMerge 或 reviewer approve 时，可触发 merge gate 汇报。

不做：

- 不做质量判断。
- 不决定 close/merge。

### 9.5 a1-bug-fix-loop

保留：

- feedback queue ledger。
- 读取 merged/closed/non-fixed 终态并推进下一条。

新增：

- 识别 examiner terminal artifact 或 router 转写的 terminal signal。
- 对 `rescope_not_close` 不归档、不推进下一条。

不做：

- 不做架构/代码/MR 质量判断。

## 10. spec_review 触发方式

当前方案不新增 `spec-review-watcher`。`spec_review` 由 router 直接触发：

```text
actor_discovery [discovery-ready]
  -> actor_router
  -> actor_examiner gate=spec_review
```

这样可以保证 delivery 启动前必经 spec 审查，也避免新增 service 带来的重复触发和
状态 owner 混乱。

## 11. 实施顺序

### Phase 1：只读审查

- 新增 `actor_examiner` spec/profile。
- router 在 MR opened 后 handoff examiner `mr_review`。
- router 在 `[discovery-ready]` 后触发 `spec_review`。
- examiner 只输出 artifact 和 MR 评论，不 approve、不 close、不改 feedback。

### Phase 2：替换 discovery MR 复核

- router 的“delivery 报 MR 后 handoff discovery 复核”改成 handoff examiner。
- discovery 只在 `rescope` / `revise_dod` 时被 router 唤醒。
- delivery Step 6 从 `review-result.v1` 迁到 `examiner-review-result.v1`。

### Phase 3：受控 approve / auto terminal

- 打开 `allow_examiner_approve`，允许 examiner 对 `quality_pass` MR approve。
- 增加 `terminal_review`，支持低风险 `auto_close_mr` 和 `auto_feedback_nonfixed`。
- router 继续保留 human gate fallback。

### Phase 4：merge policy

- 如需自动 merge，单独新增 `merge-service` 或 router action gate。
- merge 不由 examiner 直接执行。

## 12. 可行性评估

方案可行，原因：

- Joi 已有 actor/profile/service/artifact/handoff 元语，不需要改协议。
- 当前缺口主要是角色职责，不是 runtime 能力。
- 一个 actor 多 gate 可以避免多审查员通信复杂度。
- router 保持唯一状态机，可以控制循环。
- terminal 自动化可以渐进打开，不必第一版承担全部风险。

主要风险：

- examiner 过度阻塞，导致交付变慢。
- router 未严格执行 round limit，出现循环。
- terminal 自动化证据不足，误关 MR 或 feedback。
- discovery/delivery profile 未同步迁移，导致新旧复核链路并存。

对应控制：

- blocker 必须有 evidence + impact + required_action。
- advisory 不阻塞。
- 所有 gate 固定最大轮次。
- Phase 1 只读运行，观察一段时间再打开 approve/auto terminal。
- 自动 close / feedback terminal 只允许低风险白名单场景。
