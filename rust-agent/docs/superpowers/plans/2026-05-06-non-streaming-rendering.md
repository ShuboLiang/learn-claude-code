# 前端非流式渲染改进实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将前端 SSE 流式输出改为：文本实时流式显示，工具调用折叠为单行状态条，完成后统一渲染完整消息，消除视觉跳动。

**Architecture:** 修改 `chat.ts` 的 `runSSELoop` 事件处理逻辑：流式期只追加文本和迷你工具状态条，不展开工具卡片；收到 `done` 后从累积的事件构建 `UIMessage` 一次性推入列表。同步修复 `MessageBubble` 的重复 footer。

**Tech Stack:** TypeScript, React, Zustand, SSE

---

## File Structure

| 文件 | 职责 |
|------|------|
| `web/src/store/chat.ts` | SSE 事件累积与非流式渲染逻辑 |
| `web/src/components/MessageBubble.tsx` | 助手消息渲染（删除重复 footer） |

---

### Task 1: 修改 chat.ts runSSELoop 为非流式事件累积

**Files:**
- Modify: `web/src/store/chat.ts:39-310`

- [ ] **Step 1: 在 runSSELoop 顶部引入事件累积器**

在 `runSSELoop` 函数开始处（`try` 之前），添加事件累积数组和辅助状态：

```typescript
// 事件累积器：流式期间累积工具调用事件，完成后统一渲染
const toolCallEvents: Array<
  | { kind: 'tool_call'; name: string; input: unknown; parallelIndex: any; isBot: boolean }
  | { kind: 'tool_result'; name: string; output: string; isBot: boolean }
> = [];
let pendingToolCount = 0;
let completedToolCount = 0;
```

- [ ] **Step 2: 修改 text_delta / thinking_delta 处理**

保持现有逻辑（实时更新到 `target.assistantText` / `target.thinking`），确保文本继续实时流式显示。

- [ ] **Step 3: 修改 tool_call 处理**

将 `tool_call` 事件处理改为：
1. 仍创建工具调用对象但不推入 `target.tools`（或仍推入但 UI 不展开）
2. 计数器 `pendingToolCount++`
3. 在 `set` 更新中，设置一个迷你状态文本到 `target`（如 `target.toolStatus = '运行中: bash...'`）
4. 将事件信息推入 `toolCallEvents` 数组

```typescript
case 'tool_call': {
  pendingToolCount++;
  const isBot = evt.data.name === 'call_bot' || target.activeBotName != null;
  toolCallEvents.push({
    kind: 'tool_call',
    name: evt.data.name,
    input: evt.data.input,
    parallelIndex: evt.data.parallel_index ?? null,
    isBot,
  });
  // 更新迷你状态（供流式期 UI 底部显示）
  target.toolStatus = `正在运行 ${pendingToolCount} 个工具...`;
  break;
}
```

- [ ] **Step 4: 修改 tool_result 处理**

将 `tool_result` 事件处理改为：
1. `completedToolCount++`
2. 查找匹配的 `tool_call` 事件并配对
3. 更新迷你状态

```typescript
case 'tool_result': {
  completedToolCount++;
  // 倒序查找最近一个未配对的同类型 tool_call
  for (let i = toolCallEvents.length - 1; i >= 0; i--) {
    const ev = toolCallEvents[i];
    if (ev.kind === 'tool_call' && ev.name === evt.data.name) {
      // 找到匹配的 call，在其后插入 result
      toolCallEvents.splice(i + 1, 0, {
        kind: 'tool_result',
        name: evt.data.name,
        output: evt.data.output,
        isBot: ev.isBot,
      });
      break;
    }
  }
  target.toolStatus =
    completedToolCount >= pendingToolCount
      ? null
      : `已完成 ${completedToolCount}/${pendingToolCount} 个工具`;
  break;
}
```

- [ ] **Step 5: 在 StreamingState 类型中添加 toolStatus 字段**

修改 `web/src/types/ui.ts:43-62`，在 `StreamingState` 中添加：

```typescript
export interface StreamingState {
  // ... 现有字段
  /** 流式期间显示的迷你工具状态（如"运行中 3 个工具"） */
  toolStatus: string | null
}
```

- [ ] **Step 6: 修改 done / finalize 逻辑**

在收到 `done` 或流结束后，从 `toolCallEvents` 构建完整的 `UIBlock[]` 和 `UIMessage`。

替换 `finalize` 函数为：

```typescript
const finalize = () => {
  set((s) => {
    const target = s.streamingBySession[sid];
    if (!target || target.abort !== abortController) return;

    const blocks: UIBlock[] = [];

    // 1. thinking
    if (target.thinking) {
      blocks.push({ kind: 'thinking', content: target.thinking });
    }
    // 2. text
    if (target.assistantText) {
      blocks.push({ kind: 'text', content: target.assistantText });
    }
    // 3. 从累积的工具事件构建工具调用卡片
    for (const ev of toolCallEvents) {
      if (ev.kind === 'tool_call') {
        // 查找对应的 result
        const resultIdx = toolCallEvents.findIndex(
          (r) => r.kind === 'tool_result' && r.name === ev.name
        );
        const result = resultIdx >= 0 ? (toolCallEvents[resultIdx] as any) : null;
        blocks.push({
          kind: 'toolCall',
          toolCall: {
            id: nanoid(),
            name: ev.name,
            input: ev.input,
            output: result?.output ?? null,
            status: result ? 'done' : 'running',
            parallelIndex: ev.parallelIndex,
          },
        });
      }
    }
    // 4. error
    if (target.error) {
      blocks.push({
        kind: 'error',
        code: target.error.code,
        message: target.error.message,
      });
    }

    if (blocks.length > 0) {
      const assistantMsg = {
        id: nanoid(),
        role: 'assistant' as const,
        content: '',
        blocks,
        apiCalls: target.apiCalls,
        tokenUsage: target.tokenUsage ?? undefined,
      };
      s.messages.push(assistantMsg);
      if (!s.messagesBySession[sid]) s.messagesBySession[sid] = [];
      s.messagesBySession[sid].push(assistantMsg);
    }
    delete s.streamingBySession[sid];
    if (s.currentSessionId === sid) s.streaming = null;
  });
};
```

注意：上面的工具配对逻辑需要更精确（使用 ID 而非 name）。实际上更好的做法是在 tool_call 事件到达时直接创建 UIToolCall 并存入 map，tool_result 时更新 map，最后遍历 map 输出。这样既保留了现有数据结构，又避免了复杂的配对逻辑。

修正后的更简洁方案：

```typescript
// 在 runSSELoop 顶部
const toolMap = new Map<string, UIToolCall>();

// tool_call 时
const tcId = nanoid();
toolMap.set(tcId, {
  id: tcId,
  name: evt.data.name,
  input: evt.data.input,
  output: null,
  status: 'running',
  parallelIndex: evt.data.parallel_index ?? null,
});

// tool_result 时（需要服务端在 tool_result 中携带 toolId）
// 但当前服务端不携带 id，所以暂时仍用 name + output === null 匹配
// 保持现有匹配逻辑，但只在 finalize 时渲染
```

为了最小化改动，保持现有的 `target.tools` 数组累积逻辑不变，只是在流式期不通过 `buildStreamingBlocks` 渲染它们，而是等 finalize 时统一渲染。

更简化的实现：

```typescript
// runSSELoop 中保持现有 set 逻辑不变（继续累积到 target）
// 修改 buildStreamingBlocks：流式期只返回文本块 + 迷你状态条

// 新增 isFinalized 参数或区分调用场景
export function buildStreamingBlocks(st: ..., finalized: boolean = false): UIBlock[] {
  const blocks: UIBlock[] = [];
  // thinking 和 text 照常
  if (st.thinking) blocks.push({ kind: 'thinking', content: st.thinking });
  if (st.assistantText) blocks.push({ kind: 'text', content: st.assistantText });

  if (finalized) {
    // 完成时：输出所有工具调用
    for (const tc of st.tools) {
      blocks.push({ kind: 'toolCall', toolCall: tc });
    }
  } else {
    // 流式期：只输出迷你状态条（如果有工具在运行）
    if (st.toolStatus) {
      blocks.push({
        kind: 'text',
        content: `\n\n[${st.toolStatus}]`,
      });
    }
  }

  if (st.error) {
    blocks.push({ kind: 'error', code: st.error.code, message: st.error.message });
  }
  return blocks;
}
```

实际上，更简洁的做法是：

1. `runSSELoop` 中的 set 逻辑**保持几乎不变**，继续累积到 `target.tools`
2. 但 `buildStreamingBlocks` 在流式期**不输出工具调用**，只输出文本 + 迷你状态
3. `finalize` 时，从 `target.tools` 构建完整 blocks（而非从 blockOrder）

```typescript
function buildStreamingBlocks(st: StreamingState, finalized: boolean): UIBlock[] {
  const blocks: UIBlock[] = [];
  if (st.thinking) blocks.push({ kind: 'thinking', content: st.thinking });
  if (st.assistantText) blocks.push({ kind: 'text', content: st.assistantText });

  if (finalized) {
    for (const tc of st.tools) {
      blocks.push({ kind: 'toolCall', toolCall: tc });
    }
  } else if (st.tools.length > 0) {
    const running = st.tools.filter((t) => t.status === 'running').length;
    const done = st.tools.filter((t) => t.status === 'done').length;
    if (running > 0 || done > 0) {
      blocks.push({
        kind: 'text',
        content: `\n\n[工具调用中: ${done}/${st.tools.length} 完成]`,
      });
    }
  }

  if (st.error) {
    blocks.push({ kind: 'error', code: st.error.code, message: st.error.message });
  }
  return blocks;
}
```

然后在 `runSSELoop` 中，set 的同步到 `s.streaming` 保持不变（因为流式期仍需要展示文本），但 `buildStreamingBlocks` 调用时传入 `finalized=false`。

`finalize` 时：`buildStreamingBlocks(target, true)`。

这就是最简洁的方案。保持现有 SSE 事件处理逻辑不变，只改 `buildStreamingBlocks` 和一个新的 `finalized` 参数。

- [ ] **Step 7: 修改 stream 组件中 buildStreamingBlocks 的使用**

在 `runSSELoop` 内部，找到所有调用 `buildStreamingBlocks` 的地方：
1. `finalize` 函数中：`const blocks = buildStreamingBlocks(target, true);`
2. catch 块中：`const blocks = buildStreamingBlocks(target, true);`

**验证：**
发送一条触发工具调用的消息，观察：
1. 文本实时流式显示
2. 工具调用期间底部显示"[工具调用中: 0/1 完成]"
3. 完成后完整工具卡片一次性出现
4. 无页面跳动

---

### Task 2: 修复 MessageBubble 重复 footer

**Files:**
- Modify: `web/src/components/MessageBubble.tsx:96-110`

- [ ] **Step 1: 删除 AssistantBlocks 中的重复 footer**

删除以下代码：

```tsx
{(apiCalls != null && apiCalls > 0) || tokenUsage ? (
  <div className="flex items-center gap-3 text-[10px] text-muted-foreground border-t border-border pt-2">
    {apiCalls != null && apiCalls > 0 && (
      <span className="inline-flex items-center gap-1">
        <Zap className="h-3 w-3" />
        {apiCalls} API call{apiCalls > 1 ? 's' : ''}
      </span>
    )}
    {tokenUsage && (
      <span>
        {tokenUsage.input} in / {tokenUsage.output} out tokens
      </span>
    )}
  </div>
) : null}
```

**验证：**
1. 助手消息底部只显示一个 footer
2. API calls 和 token usage 仍然正确显示

---

## Commit 建议

```bash
git add web/src/store/chat.ts web/src/components/MessageBubble.tsx web/src/types/ui.ts
git commit -m "refactor(web): 非流式工具渲染，消除视觉跳动

- 流式期只显示文本和迷你工具状态条
- 完成后统一渲染完整工具卡片
- 修复 MessageBubble 重复 footer"
```
