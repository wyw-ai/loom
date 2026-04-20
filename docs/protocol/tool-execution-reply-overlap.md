# Tool Execution 展开与 Reply 按钮冲突修复说明

> 日期：2026-03-31  
> 背景：消息卡片（`MessageItem.tsx`）在展示 Agent 工具调用/执行轨迹（execution）时，展开区域与 `Reply` 浮动按钮出现 UI 冲突。

---

## 1. 问题描述

当消息包含工具调用（tool execution trace）并且用户将 execution 展开时：

- 右下角绝对定位的 `Reply` 按钮会与 execution 的折叠/展开控制（collapse toggle）发生**遮挡/重叠**
- 用户期望：发生冲突时**隐藏浮动 Reply**，并将 `Reply` 放到 execution 控制区域（通常在 collapse toggle 的左侧），避免重叠且更符合上下文

---

## 2. 已实现的修复思路（来自协作实现）

在 `MessageItem.tsx` 中对交互入口进行重新布局：

- execution 展开时：右下角浮动 `Reply` 不应继续显示（避免与展开控制冲突）
- execution 展开时：在 execution 控制栏内提供一个 **inline 的 Reply**（靠近 collapse toggle，位于其左侧）
- execution 收起时：保留原先的右下角浮动 `Reply`

---

## 3. Code Review 发现：无障碍/键盘交互风险（重要）

如果“隐藏浮动 Reply”的做法只是：

- 通过 `opacity: 0` / `pointer-events: none` 让它**视觉上不可见、鼠标不可点**

那么它仍然可能：

- **留在 DOM / 可访问树**里
- 被键盘 **Tab 聚焦**到（出现“看不见但能聚焦”的按钮）
- 被读屏软件读到，且在展开时与 inline Reply 形成**重复的同名控件**

这会造成 a11y 与键盘用户体验回归。

---

## 4. 推荐的最终实现方式（建议落地）

### 4.1 优先使用条件渲染移除浮动 Reply

- `showExecution === false` 时：渲染右下角绝对定位 Reply
- `showExecution === true` 时：不渲染右下角 Reply（从 DOM 与可访问树中移除）

> 不建议仅“视觉隐藏”，除非同时完整处理可访问树与焦点管理（成本更高、也更容易遗漏）。

### 4.2 inline Reply 与浮动 Reply 复用同一逻辑

- 两个入口必须触发相同的 reply handler（例如同一 `onReply()` / `openThreadReply()`），避免行为不一致

---

## 5. 验收清单（建议合入前自测）

### 5.1 UI/交互

- execution 展开时：不会出现任何按钮遮挡/重叠
- execution 展开时：inline Reply 可见且易点（不挤压 collapse toggle 的点击区域）
- execution 收起时：右下角浮动 Reply 正常显示
- 快速展开/收起多次：布局不抖动、不出现重复按钮

### 5.2 键盘与无障碍（必须）

- 展开 execution 后：Tab 不会聚焦到“看不见的 Reply”
- 展开 execution 后：读屏不会读到重复的 `Reply` 控件
- inline Reply 有清晰的可访问名称（必要时可用更明确的 `aria-label`）

---

## 6. 可复用的团队 Skill（草案）

**Skill 名称**：`joi-frontend-overlap-fix`  
**适用场景**：任意“展开面板（trace/timeline/etc）与浮动 CTA（Reply/Copy/etc）”发生重叠的 UI  
**强约束**：冲突时隐藏必须做到“从交互中移除”，优先条件渲染，而不是仅视觉隐藏。

