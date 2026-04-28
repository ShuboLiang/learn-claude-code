# Bot 会话管理设计

> 状态：已批准 | 日期：2026-04-27

## 问题

`run_bot()` 每次调用创建全新 `ContextService`，执行一次 agent loop 后销毁。Bot 无法与用户多轮交互。典型案例：resume-screener Bot 需要在步骤 4 反问用户选择评分方案，但基础设施不支持"bot 问 → 用户答 → bot 继续"的闭环。

## 目标

为 Bot 子代理增加会话持久化能力，支持多轮交互：Bot 反问用户 → 用户回复 → Bot 从断点继续执行。

## 设计

### 核心结构

```rust
// bots/mod.rs — BotSessionManager
pub struct BotSession {
    pub ctx: ContextService,      // 对话历史（含简历解析+打分等全部中间结果）
    pub created_at: Instant,      // 创建时间，用于过期清理
}

// BotRegistry 新增字段
pub struct BotRegistry {
    bots: BTreeMap<String, BotDefinition>,
    sessions: BTreeMap<String, BotSession>,  // ← 新增
}
```

### 调用流程

```
首次: call_bot("resume-screener", "请筛选这些简历...")
  → run_bot() → sessions.get("resume-screener") == None
  → 创建新 ContextService → 注入 BOT.md system prompt
  → run_agent_loop() 多轮工具调用 → Bot 反问用户
  → agent_loop 返回文本（问题）
  → 保存 BotSession { ctx, Instant::now() }
  → 返回问题文本给主 Agent

恢复: call_bot("resume-screener", "选C")
  → run_bot() → sessions.get("resume-screener") == Some(session)
  → 恢复 session.ctx
  → ctx.push_user_text("选C")
  → system prompt 追加"这是恢复执行的会话，直接从上次中断处继续"
  → run_agent_loop() 继续 → 应用权重 → 排序 → 返回结果
  → 清除会话
```

### 判断标准

Bot agent_loop 返回 `Ok(text)`（非错误）且非超时上限 → 自动保存会话。Bot 不需要额外标记。因为：
- 如果 Bot 完成了任务，返回结果文本 → 主 Agent 处理结果 → 下次同名 Bot 调用覆盖旧会话
- 如果 Bot 反问用户，返回问题文本 → 会话保存 → 等用户回复后恢复

### 过期策略

会话超过 30 分钟自动清理。每次 `run_bot` 时检查。

### 改动清单

| 文件 | 改动 |
|------|------|
| `bots/mod.rs` | `BotSession` 结构体 + `BotRegistry` 新增 `sessions` 字段和 3 个方法 |
| `agent.rs:run_bot()` | 检测活跃会话 → 恢复/新建 → 执行后保存/清理 |
| `agent.rs:system_prompt` | 恢复会话时追加"从上次中断处继续执行"指令 |
| `bots/mod.rs:tests` | Bot 会话存取测试 |
| `agent.rs:tests` | 首次调用/恢复调用场景测试 |

### 不变

- `run_agent_loop` 签名不变（仍然返回 `AgentResult<String>`）
- `call_bot` 工具 schema 不变
- 现有 Bot 定义文件（BOT.md）格式不变
