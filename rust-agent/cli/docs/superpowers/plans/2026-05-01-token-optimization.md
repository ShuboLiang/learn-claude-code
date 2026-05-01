# Token 优化 CLI 增强 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 rust-agent CLI 客户端中实现 Token 用量遥测统计面板与预算告警系统，落地 token-optimization-report.md 中的 P0/P1 监控策略。

**Architecture:** 引入两个纯 TypeScript 业务模块 `TokenStats`（累计统计）和 `BudgetManager`（预算检查），通过 React ref 注入 App 组件生命周期，在每次 SSE `turn_end` 事件时自动记录用量；新增 `/tokens`（查看统计）、`/budget`（设置预算）、`/budget clear`（清除预算）三条斜杠命令；输入框占位文本同步更新以提示用户新命令。

**Tech Stack:** TypeScript, React (Ink), Node.js native test runner

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/token-stats.ts` | Create | 累计记录每次 API 调用的 input/output/cache_read token，提供会话级汇总 |
| `tests/token-stats.test.ts` | Create | TokenStats 单元测试：单条记录、累计、含缓存、reset |
| `src/budget.ts` | Create | 根据 maxTotalTokens / maxInputTokens / maxOutputTokens 检查是否超限 |
| `tests/budget.test.ts` | Create | BudgetManager 单元测试：未设置预算通过、总预算超限、输入/输出单项超限、清除预算 |
| `src/app.tsx` | Modify | 集成 TokenStats 与 BudgetManager；在 `turn_end` 中记录用量并检查预算；添加 `/tokens`、`/budget`、`/budget clear` 命令处理；`handleClear` 中重置统计 |
| `src/input.tsx` | Modify | 更新 placeholder 提示 `/tokens` 与 `/budget` 命令 |

---

### Task 1: Token 统计跟踪器 (`src/token-stats.ts`)

**Files:**
- Create: `src/token-stats.ts`
- Test: `tests/token-stats.test.ts`

- [ ] **Step 1: Write the failing test**

```typescript
// tests/token-stats.test.ts
import { test } from "node:test";
import assert from "node:assert";
import { TokenStats } from "../src/token-stats.js";

test("records single usage", () => {
  const stats = new TokenStats();
  stats.record({ input_tokens: 100, output_tokens: 50 });
  const s = stats.getSummary();
  assert.strictEqual(s.totalInput, 100);
  assert.strictEqual(s.totalOutput, 50);
  assert.strictEqual(s.totalCacheRead, 0);
  assert.strictEqual(s.totalTokens, 150);
  assert.strictEqual(s.callCount, 1);
});

test("records usage with cache_read_tokens", () => {
  const stats = new TokenStats();
  stats.record({ input_tokens: 200, output_tokens: 100, cache_read_tokens: 50 });
  const s = stats.getSummary();
  assert.strictEqual(s.totalInput, 200);
  assert.strictEqual(s.totalOutput, 100);
  assert.strictEqual(s.totalCacheRead, 50);
});

test("accumulates multiple records", () => {
  const stats = new TokenStats();
  stats.record({ input_tokens: 100, output_tokens: 50 });
  stats.record({ input_tokens: 200, output_tokens: 100, cache_read_tokens: 50 });
  const s = stats.getSummary();
  assert.strictEqual(s.totalInput, 300);
  assert.strictEqual(s.totalOutput, 150);
  assert.strictEqual(s.totalCacheRead, 50);
  assert.strictEqual(s.totalTokens, 450);
  assert.strictEqual(s.callCount, 2);
});

test("reset clears all data", () => {
  const stats = new TokenStats();
  stats.record({ input_tokens: 100, output_tokens: 50 });
  stats.reset();
  const s = stats.getSummary();
  assert.strictEqual(s.totalTokens, 0);
  assert.strictEqual(s.callCount, 0);
  assert.strictEqual(s.totalCacheRead, 0);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx tsx --test tests/token-stats.test.ts`
Expected: FAIL with `ReferenceError: TokenStats is not defined` or `Cannot find module`

- [ ] **Step 3: Write minimal implementation**

```typescript
// src/token-stats.ts
export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens?: number;
}

export interface TokenSummary {
  totalInput: number;
  totalOutput: number;
  totalCacheRead: number;
  totalTokens: number;
  callCount: number;
}

export class TokenStats {
  private totalInput = 0;
  private totalOutput = 0;
  private totalCacheRead = 0;
  private callCount = 0;

  record(usage: TokenUsage): void {
    this.totalInput += usage.input_tokens || 0;
    this.totalOutput += usage.output_tokens || 0;
    this.totalCacheRead += usage.cache_read_tokens || 0;
    this.callCount++;
  }

  getSummary(): TokenSummary {
    return {
      totalInput: this.totalInput,
      totalOutput: this.totalOutput,
      totalCacheRead: this.totalCacheRead,
      totalTokens: this.totalInput + this.totalOutput,
      callCount: this.callCount,
    };
  }

  reset(): void {
    this.totalInput = 0;
    this.totalOutput = 0;
    this.totalCacheRead = 0;
    this.callCount = 0;
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx tsx --test tests/token-stats.test.ts`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
git add tests/token-stats.test.ts src/token-stats.ts
git commit -m "feat: add TokenStats tracker for session-level token telemetry"
```

---

### Task 2: Token 预算管理器 (`src/budget.ts`)

**Files:**
- Create: `src/budget.ts`
- Test: `tests/budget.test.ts`

- [ ] **Step 1: Write the failing test**

```typescript
// tests/budget.test.ts
import { test } from "node:test";
import assert from "node:assert";
import { BudgetManager } from "../src/budget.js";

test("checkBudget passes when no budget set", () => {
  const bm = new BudgetManager();
  const result = bm.checkBudget(1000, 500);
  assert.strictEqual(result.exceeded, false);
  assert.strictEqual(result.reason, undefined);
});

test("checkBudget fails when total exceeds maxTotalTokens", () => {
  const bm = new BudgetManager();
  bm.setBudget({ maxTotalTokens: 1000 });
  const result = bm.checkBudget(700, 400);
  assert.strictEqual(result.exceeded, true);
  assert.ok(result.reason?.includes("总 Token 预算超限"));
});

test("checkBudget fails when input exceeds maxInputTokens", () => {
  const bm = new BudgetManager();
  bm.setBudget({ maxInputTokens: 500 });
  const result = bm.checkBudget(600, 100);
  assert.strictEqual(result.exceeded, true);
  assert.ok(result.reason?.includes("输入 Token 预算超限"));
});

test("checkBudget fails when output exceeds maxOutputTokens", () => {
  const bm = new BudgetManager();
  bm.setBudget({ maxOutputTokens: 200 });
  const result = bm.checkBudget(100, 250);
  assert.strictEqual(result.exceeded, true);
  assert.ok(result.reason?.includes("输出 Token 预算超限"));
});

test("getBudget returns current config", () => {
  const bm = new BudgetManager();
  bm.setBudget({ maxTotalTokens: 5000, maxInputTokens: 3000 });
  const config = bm.getBudget();
  assert.strictEqual(config.maxTotalTokens, 5000);
  assert.strictEqual(config.maxInputTokens, 3000);
});

test("clearBudget removes limits", () => {
  const bm = new BudgetManager();
  bm.setBudget({ maxTotalTokens: 1000 });
  bm.clearBudget();
  const result = bm.checkBudget(2000, 2000);
  assert.strictEqual(result.exceeded, false);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx tsx --test tests/budget.test.ts`
Expected: FAIL with `ReferenceError: BudgetManager is not defined`

- [ ] **Step 3: Write minimal implementation**

```typescript
// src/budget.ts
export interface BudgetConfig {
  maxInputTokens?: number;
  maxOutputTokens?: number;
  maxTotalTokens?: number;
}

export interface BudgetCheckResult {
  exceeded: boolean;
  reason?: string;
}

export class BudgetManager {
  private config: BudgetConfig = {};

  setBudget(config: BudgetConfig): void {
    this.config = { ...config };
  }

  getBudget(): BudgetConfig {
    return { ...this.config };
  }

  clearBudget(): void {
    this.config = {};
  }

  checkBudget(inputTokens: number, outputTokens: number): BudgetCheckResult {
    const total = inputTokens + outputTokens;

    if (this.config.maxTotalTokens && total > this.config.maxTotalTokens) {
      return {
        exceeded: true,
        reason: `总 Token 预算超限: ${total} / ${this.config.maxTotalTokens}`,
      };
    }
    if (this.config.maxInputTokens && inputTokens > this.config.maxInputTokens) {
      return {
        exceeded: true,
        reason: `输入 Token 预算超限: ${inputTokens} / ${this.config.maxInputTokens}`,
      };
    }
    if (this.config.maxOutputTokens && outputTokens > this.config.maxOutputTokens) {
      return {
        exceeded: true,
        reason: `输出 Token 预算超限: ${outputTokens} / ${this.config.maxOutputTokens}`,
      };
    }
    return { exceeded: false };
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx tsx --test tests/budget.test.ts`
Expected: PASS (6 tests)

- [ ] **Step 5: Commit**

```bash
git add tests/budget.test.ts src/budget.ts
git commit -m "feat: add BudgetManager for token budget limits and alerts"
```

---

### Task 3: CLI 集成 (`/tokens` 与 `/budget` 命令)

**Files:**
- Modify: `src/app.tsx`
- Modify: `src/input.tsx`

- [ ] **Step 1: Add imports and refs in `src/app.tsx`**

在 `src/app.tsx` 顶部现有 imports 之后追加：

```typescript
import { TokenStats } from "./token-stats";
import { BudgetManager } from "./budget";
```

在 `App` 组件内部，`retryStatus` state 之后添加：

```typescript
  // Token 统计与预算管理
  const tokenStatsRef = useRef(new TokenStats());
  const budgetManagerRef = useRef(new BudgetManager());
```

- [ ] **Step 2: Update `handleClear` to reset token stats**

将 `handleClear` 替换为：

```typescript
  const handleClear = useCallback(() => {
    setMessages((prev) => [
      ...prev,
      { role: "system", content: "═══ 以上对话已被清除 ═══" },
    ]);
    setCurrentReply("");
    tokenStatsRef.current.reset();
    // 仅在已有会话时才请求清除
    if (getConfig().sessionId) {
      clearSession().catch(() => {});
    }
  }, []);
```

- [ ] **Step 3: Record token usage and check budget in main `turn_end`**

在 `handleSubmit` 的 `sendMessage` 循环中，定位到 `case "turn_end":` 块。将其中 `if (tokenUsage) { ... }` 部分替换为：

```typescript
              if (tokenUsage) {
                info += ` │ Token: ${formatTokens(tokenUsage.input_tokens)}入/${formatTokens(tokenUsage.output_tokens)}出`;
                if (tokenUsage.cache_read_tokens > 0) {
                  info += ` (缓存命中 ${formatTokens(tokenUsage.cache_read_tokens)})`;
                }
                tokenStatsRef.current.record(tokenUsage);
                const check = budgetManagerRef.current.checkBudget(
                  tokenUsage.input_tokens,
                  tokenUsage.output_tokens,
                );
                if (check.exceeded) {
                  setMessages((prev) => [
                    ...prev,
                    { role: "system", content: `⚠️ ${check.reason}` },
                  ]);
                }
              }
```

- [ ] **Step 4: Record token usage and check budget in bot `turn_end`**

在 `handleBotTask` 的 `sendBotTask` 循环中，定位到 `case "turn_end":` 块。将其中 `if (tokenUsage) { ... }` 部分替换为：

```typescript
              if (tokenUsage) {
                info += ` │ Token: ${formatTokens(tokenUsage.input_tokens)}入/${formatTokens(tokenUsage.output_tokens)}出`;
                tokenStatsRef.current.record(tokenUsage);
                const check = budgetManagerRef.current.checkBudget(
                  tokenUsage.input_tokens,
                  tokenUsage.output_tokens,
                );
                if (check.exceeded) {
                  setMessages((prev) => [
                    ...prev,
                    { role: "system", content: `⚠️ ${check.reason}` },
                  ]);
                }
              }
```

- [ ] **Step 5: Add `/tokens`, `/budget`, `/budget clear` command handlers**

在 `handleSubmit` 中，在 `/load` 命令处理之后、`// 懒创建会话` 注释之前，插入以下命令处理：

```typescript
      // /tokens command: show token statistics
      if (input.trim().toLowerCase() === "/tokens") {
        const s = tokenStatsRef.current.getSummary();
        const lines = [
          "═══ Token 统计 ═══",
          `API 调用次数: ${s.callCount}`,
          `输入 Token: ${formatTokens(s.totalInput)}`,
          `输出 Token: ${formatTokens(s.totalOutput)}`,
          `缓存命中: ${formatTokens(s.totalCacheRead)}`,
          `总计: ${formatTokens(s.totalTokens)}`,
        ];
        const budget = budgetManagerRef.current.getBudget();
        if (budget.maxTotalTokens) {
          lines.push(`预算上限: ${formatTokens(budget.maxTotalTokens)}`);
          const pct = (s.totalTokens / budget.maxTotalTokens) * 100;
          lines.push(`使用率: ${pct.toFixed(1)}%`);
        }
        lines.push("════════════════");
        setMessages((prev) => [
          ...prev,
          { role: "system", content: lines.join("\n") },
        ]);
        return;
      }

      // /budget command: set token budget
      const budgetMatch = input.trim().match(/^\/budget\s+(\d+)(?:\s+(\d+))?(?:\s+(\d+))?$/);
      if (budgetMatch) {
        const maxTotal = parseInt(budgetMatch[1], 10);
        const maxInput = budgetMatch[2] ? parseInt(budgetMatch[2], 10) : undefined;
        const maxOutput = budgetMatch[3] ? parseInt(budgetMatch[3], 10) : undefined;
        budgetManagerRef.current.setBudget({
          maxTotalTokens: maxTotal,
          maxInputTokens: maxInput,
          maxOutputTokens: maxOutput,
        });
        const parts = [`总预算: ${formatTokens(maxTotal)}`];
        if (maxInput) parts.push(`输入上限: ${formatTokens(maxInput)}`);
        if (maxOutput) parts.push(`输出上限: ${formatTokens(maxOutput)}`);
        setMessages((prev) => [
          ...prev,
          {
            role: "system",
            content: `✅ 已设置 Token 预算\n${parts.join("\n")}`,
          },
        ]);
        return;
      }

      // /budget clear command: clear token budget
      if (input.trim().toLowerCase() === "/budget clear") {
        budgetManagerRef.current.clearBudget();
        setMessages((prev) => [
          ...prev,
          { role: "system", content: "✅ 已清除 Token 预算" },
        ]);
        return;
      }
```

- [ ] **Step 6: Update placeholder in `src/input.tsx`**

将 `placeholder` 的定义替换为：

```typescript
  const placeholder = isLoading
    ? "(等待响应中...)"
    : isMultiline
      ? "多行模式: 输入 /send 提交, ESC 或 /cancel 取消..."
      : model
        ? `[${model}] 输入消息, Enter 提交, /tokens 统计, /budget 预算, /@bot 委派, /m 多行...`
        : "输入消息, Enter 提交, /tokens 统计, /budget 预算, /@bot 委派, /m 多行...";
```

- [ ] **Step 7: Type-check and run all tests**

Run: `npx tsc --noEmit`
Expected: 无编译错误

Run: `npx tsx --test tests/*.test.ts`
Expected: PASS (4 token-stats + 6 budget + 5 session-utils = 15 tests)

- [ ] **Step 8: Commit**

```bash
git add src/app.tsx src/input.tsx
git commit -m "feat: integrate /tokens stats and /budget alerts into CLI"
```

---

## Self-Review

**1. Spec coverage:**
- token-optimization-report.md P0 "监控与分析 (Token 遥测)" → Task 1 + Task 3 `/tokens`
- token-optimization-report.md P0 "上下文预算" → Task 2 + Task 3 `/budget`
- token-optimization-report.md P1 "工具结果裁剪" → 已有 `chat.tsx:68` maxShow=5000，本计划不重复

**2. Placeholder scan:** 无 TBD、TODO、implement later、fill in details。

**3. Type consistency:** `TokenUsage` 字段名（input_tokens / output_tokens / cache_read_tokens）与 `app.tsx` 中 `event.data?.token_usage` 的现有字段一致。`formatTokens` 在 Task 3 中复用现有函数。

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-05-01-token-optimization.md`. Two execution options:**

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints for review

**Which approach?**
