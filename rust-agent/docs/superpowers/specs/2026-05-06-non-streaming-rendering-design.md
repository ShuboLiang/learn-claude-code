# 前端非流式渲染改进设计

## 背景

当前前端在流式输出过程中，工具调用卡片会突然插入到正在流式的文本前面，导致页面跳动、排版混乱。特别是在 Bot 子代理（`call_bot`）参与时，嵌套工具调用的层级展开后信息密度过高，用户难以阅读。

## 目标

- 消除流式输出过程中的视觉跳动
- 保持文本的实时感知
- Bot 子代理的工具调用结果排版清晰
- 最小化实现复杂度

## 方案

采用**方案B：只流式文本，工具完成后统一显示**。

### 核心改动

#### 1. `chat.ts:runSSELoop` — 累积事件而非实时更新 UI

- 流式期间不再通过 `set()` 实时更新 `streaming` 状态中的 blocks
- 文本增量（`text_delta` / `thinking_delta`）仍实时更新到 `streaming.assistantText` / `streaming.thinking`，保持阅读体验
- 工具调用事件（`tool_call` / `tool_result`）仅累积到内部数组，**不渲染到 UI**
- 收到 `done` 事件后，从累积的事件数组构建完整的 `UIMessage`，一次性推入 `messages`
- 累积期间，在 UI 底部显示一个迷你状态条，如"🛠 正在运行 3 个工具..."

#### 2. 流式期与完成期的渲染差异

| 阶段 | 文本 | 工具调用 |
|------|------|---------|
| 流式期 | 实时增量追加 | 折叠为单行状态条（不占空间） |
| 完成期 | 完整文本块 | 完整 ToolCallCard 展开 |

#### 3. Bot 子代理的嵌套处理

- `call_bot` 容器在流式期间也折叠为单行
- Bot 内部的子工具调用不再在流式期展开
- 完成后，`call_bot` 卡片可展开查看其子工具列表（保持现有 `children` 数据结构）

#### 4. `MessageBubble.tsx` — 移除重复 footer

- 删除 `AssistantBlocks` 中第 96-110 行的重复 footer 渲染
- 只在消息级别（第 57-67 行）显示一次 API calls / token usage

## 影响范围

- `web/src/store/chat.ts` — 重写 `runSSELoop` 的事件处理逻辑
- `web/src/components/MessageBubble.tsx` — 删除重复 footer，可选调整 loading 状态展示
- `web/src/types/ui.ts` — 无需修改

## 验收标准

- [ ] 发送消息后，文本实时流式显示，无跳动
- [ ] 工具调用期间页面高度基本稳定，只有底部单行状态条变化
- [ ] 收到 done 后，完整消息（文本 + 工具卡片）一次性出现
- [ ] Bot 子代理的嵌套工具调用排版不混乱
- [ ] 刷新页面后 hydrate 的完整消息与流式结束时的视觉一致
