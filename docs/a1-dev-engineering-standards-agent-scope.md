# a1-dev-canfeng 研发规范与验证方案 Agent 改动范围

> 状态：方案范围文档，尚未落地到线上 profile / spec。  
> 目标：补齐 a1-dev-canfeng 链路中“研发规范制定、远端 debug 手段、验证方案编排”的职责缺口。

## 背景

当前 a1-dev-canfeng 已经把研发闭环拆成：

```text
discovery 出题，examiner 判题，delivery 做题，mr-watcher 报事实，router 管状态，bug-fix-loop 管队列。
```

已有研发规范主要覆盖仓库内动作，例如构建、测试、lint、OpenSpec、commit、MR 描述和 review 回复。  
但遇到远端服务、发布链路、平台 API、AppCenter、pipeline、CR、配置或数据状态时，delivery 往往只能做到“代码写完 + 本地测试”，缺少稳定的远端 debug 手段和可审计验证方案。

本次改动的核心不是泛化地补“远端验证步骤”，而是建立：

```text
先有 Debug Playbook，再有 Validation Plan，再有 Validation Evidence。
```

## 新增角色

新增 agent：

```text
actor_engineering_standards
displayName: 研发规范员
```

职责一句话：

```text
制定 repo 开发规范 + 服务 debug playbook + 验证方案编排，不写代码、不判题、不推进 MR。
```

### 允许做什么

- 读取待修改仓库和参考仓库的代码、README、CI、OpenSpec、历史 MR、reviewer 评论、已有 kbase 规范。
- 制定或更新 kbase 74121 中的仓库研发规范。
- 为远端服务类任务编排 `validation-plan.v1`。
- 把验证方案中的每个关键步骤绑定到具体规范页、章节和证据来源。
- 当规范缺失时，产出 `repo-dev-standard-proposal.v1` 或直接创建/更新 kbase 页面。
- 将通用踩坑、debug 入口、日志查询、trace key、发布/回滚入口沉淀回规范。

### 禁止做什么

- 不写业务代码。
- 不修改 delivery worktree。
- 不审 MR 质量，不输出 `quality_pass` / `needs_changes`。
- 不创建 delivery thread，不直接唤醒 delivery。
- 不 merge，不 approve，不改 feedback 状态。
- 不把单次偶发现象沉淀为长期规范；缺证据时输出 proposal 或 human gate。

## 职责边界变化

新增后的边界：

```text
discovery 定义问题、repo scope、粗方案
engineering_standards 制定/补齐规范，并基于规范编排验证方案
examiner 审查五件套、验证方案、MR 和验证证据
delivery 按规范和验证方案执行
mr-watcher 报 MR/CI/comment/终态事实
router 管状态、human gate、权限 gate、发布 gate
bug-fix-loop 管队列
```

router 不再直接维护规范正文；router 只识别触发条件并 directed message `actor_engineering_standards`。

## kbase 74121 结构升级

kbase 74121 仍作为研发规范的唯一源头。每个仓库可以有多篇规范，文章名必须匹配：

```text
[<group>/<repo>] <title>
```

每个涉及远端服务的仓库规范必须补齐以下章节。

### Repo Dev Standard

```text
## 适用范围
## 构建 / 测试 / Lint 命令
## OpenSpec / 变更流程
## 分支与 commit 规范
## MR 描述与 review 回复规范
## CI / 合并 gate
```

### Service Debug Playbook

```text
## Debug 手段

### 版本定位
- 如何确认 daily / pre / prod 当前运行的 commit、镜像、包版本或 pipeline instance。
- 对应命令、页面、API 或平台入口。

### 请求注入
- 如何构造最小请求。
- 使用哪个测试账号、app、project、workitem、cr、pipeline。
- 是否可以注入 request_id、trace_id、biz_id。

### 日志入口
- 查哪个平台、哪个 app/service、哪个环境。
- 精确查询语句或 URL。
- 关键字段：trace_id、request_id、user、appId、workitemId、pipelineId、errorCode。
- 时间窗口选择规则。

### 链路追踪
- 如何从入口请求追到下游服务。
- 关键下游依赖。
- 哪些错误说明问题在调用方，哪些说明问题在被调方。

### 状态检查
- 只读查询数据库、配置、缓存、发布单、CR、pipeline 状态的方式。
- 预期状态和异常状态。

### 失败定位矩阵
- 症状、首查入口、下一步、可能责任边界。
```

### Release / Rollback Standard

```text
## 部署 / 发布
- 是否需要发布服务、前端资源、CLI 包、SDK、配置、数据库变更。
- 发布入口、pipeline、appId、环境、权限要求。
- human gate 条件。

## 回滚与风险
- 是否可回滚。
- 回滚入口。
- 兼容性风险。
- 是否需要灰度或观察期。
```

规范页必须保留“证据来源”和“最近更新记录”，避免凭空沉淀。

## Discovery 产物升级

当前 discovery 产物应从三件套扩展为五件套：

```text
task-goal.json
definition-of-done.json
clone-manifest.json
implementation-outline.json
validation-plan.json
```

其中：

- `task-goal.json`：问题、目标、范围、非范围。
- `definition-of-done.json`：可验证完成标准。
- `clone-manifest.json`：待修改仓库和参考仓库。
- `implementation-outline.json`：粗方案、影响面、涉及服务/API/页面/发布链路。
- `validation-plan.json`：由 `actor_engineering_standards` 基于研发规范编排的验证方案。

`validation-plan.json` 不应由 discovery 自己凭经验生成。正确流程是：

```text
discovery 先确定 target repos / reference repos / 粗方案
  -> directed message actor_engineering_standards gate=compose_validation_plan
  -> engineering_standards 读取 kbase 74121 相关规范
  -> publish validation-plan.v1
  -> discovery 把 validation-plan art id 放进 [discovery-ready]
```

## validation-plan.v1

`validation-plan.v1` 必须引用具体规范来源，不能只写泛化步骤。

示例结构：

```json
{
  "schema": "validation-plan.v1",
  "schema_version": "1",
  "producer": "actor_engineering_standards",
  "channel_id": "chan_...",
  "thread_id": "thread_...",
  "target_repos": ["aone/a1"],
  "reference_repos": ["aone/a1-server", "aone/app-center"],
  "affected_services": ["a1-server", "app-center"],
  "requires_remote_debug": true,
  "requires_deploy": true,
  "standard_refs": [
    {
      "repo": "aone/a1",
      "page_id": "12345",
      "title": "[aone/a1] App CR 调试与发布规范",
      "section": "Debug 手段 / 请求注入",
      "used_for": "构造最小 a1 app cr submit 验证命令"
    }
  ],
  "validation_steps": [
    {
      "id": "v1",
      "type": "remote_debug",
      "environment": "daily",
      "command_or_action": "a1 app cr submit ...",
      "expected_signal": "返回 pipeline instance id 和发布页 URL",
      "debug_keys": ["request_id", "pipeline_id"],
      "log_or_trace_query": "按 request_id 查询 a1-server/app-center 日志",
      "state_check": "查询 CR / pipeline 状态为已提交",
      "source_refs": ["aone/a1#12345:Debug 手段 / 请求注入"]
    }
  ],
  "evidence_required": [
    "命令输出",
    "request_id 或 trace_id",
    "日志命中结果",
    "远端状态检查结果"
  ],
  "human_gates": [
    {
      "kind": "permission",
      "reason": "delivery 无发布权限时由 router 请求 human 执行"
    }
  ],
  "known_blind_spots": []
}
```

## Delivery 产物升级

delivery 仍负责实现和验证，但必须按 `validation-plan.v1` 执行。涉及远端服务、发布或平台链路时，delivery 在 MR 阶段前或 MR 汇报时必须产出：

```text
validation-evidence.v1
```

示例结构：

```json
{
  "schema": "validation-evidence.v1",
  "schema_version": "1",
  "producer": "actor_delivery",
  "validation_plan": "artifact://...",
  "executed_steps": [
    {
      "step_id": "v1",
      "status": "passed",
      "environment": "daily",
      "command_or_action": "...",
      "observed_output": "...",
      "request_id": "...",
      "trace_id": "...",
      "log_evidence": "...",
      "state_check": "...",
      "artifacts": ["artifact://..."]
    }
  ],
  "remaining_gates": [],
  "conclusion": "远端行为符合 validation-plan.v1"
}
```

如果 validation plan 要求发布、查询日志或访问环境，但 delivery 没有权限，delivery 必须 directed message router 发起 human / permission gate，不能把任务宣称为验证完成。

## Gate 变化

### spec_review

examiner 在 `gate=spec_review` 中新增检查：

- 涉及远端服务、发布、平台 API、配置、数据状态或跨服务调用时，必须存在 `validation-plan.v1`。
- `validation-plan.v1` 必须引用 kbase 74121 中的具体规范页和章节。
- 如果目标仓库或参考仓库缺少 debug playbook，不能直接启动 delivery；应要求 `actor_engineering_standards` 补规范或输出 human gate。
- 只有本地 CLI、纯文档、纯测试、纯静态页面等不涉及远端行为的任务，才允许没有远端 debug 步骤。

### mr_review

examiner 在 `gate=mr_review` 中新增检查：

- delivery 是否读取并遵守相关 kbase 规范。
- delivery 是否按 `validation-plan.v1` 执行。
- 涉及远端服务时，是否存在足够的 `validation-evidence.v1`。
- 证据是否能证明真实远端行为正确，而不只是本地测试通过。
- 缺少 debug evidence 时，输出 `needs_changes` 或 `blocked`，而不是 `quality_pass`。

### design_review

如果验证方案暴露出目标、scope、DoD 或服务边界错误，应进入 `design_review`，而不是让 delivery 继续在错误仓库里修。

## 触发条件

router 应在以下场景 directed message `actor_engineering_standards`：

- delivery 报 kbase 74121 规范 `MISSING`。
- discovery 识别到任务涉及远端服务、发布、配置、数据状态或跨服务链路。
- human 纠正规范、debug 手段、发布方式或验证路径。
- reviewer 多次提出同类“没有远端验证 / 页面是否更新 / 发布是否生效 / 日志怎么证明”的问题。
- delivery 提交 `[standards-suggestion]`，说明某个 repo/service 有通用坑需要沉淀。
- examiner 在 `spec_review` / `mr_review` 中发现验证方案或 debug 手段缺失。

## 非目标

本次范围不包含：

- 新增 merge 权限。
- 让 router 写代码或判质量。
- 让 `actor_engineering_standards` 替代 examiner 审 MR。
- 自动执行生产发布。
- 绕过 human 权限 gate。
- 把每个任务的临时调试过程都沉淀成长期规范。

## 落地阶段

### Phase 1：文档与 profile 协议

- 新增 `actor_engineering_standards` 的 profile。
- 更新 router / discovery / delivery / examiner profile。
- 更新 `docs/a1-dev-canfeng-final-actors.md`。
- 定义 `validation-plan.v1` 和 `validation-evidence.v1` artifact schema。

### Phase 2：启动脚本与 directed message

- `start-delivery.sh` 支持传入 `validation-plan` artifact id。
- discovery 在 `[discovery-ready]` 中必须带 `validation-plan`，或明确 `validation_not_required`。
- delivery 启动消息中包含 target repo 规范和 validation plan。

### Phase 3：门禁强化

- examiner `spec_review` 检查 validation plan。
- examiner `mr_review` 检查 validation evidence。
- 缺少 debug playbook 时阻塞到 `actor_engineering_standards`，而不是让 delivery 盲写。

### Phase 4：规范沉淀闭环

- delivery / examiner / mr-watcher 发现通用 debug 或发布问题时，形成 standards suggestion。
- `actor_engineering_standards` 评估并更新 kbase 74121。
- 更新后由 router 唤醒原任务继续。

## 待确认问题

- `actor_engineering_standards` 使用 Claude 还是 Codex provider。
- kbase 74121 是否继续承载所有 repo dev + service debug + release 规范，还是拆出独立服务规范库。
- 对约定型规范更新是否需要 human approval gate。
- validation plan 是否在所有任务中强制存在，还是只对远端服务类任务强制。
- 是否需要脚本级 validator 检查 `repo-specs/` 和 `validation-evidence.v1` 完整性。
