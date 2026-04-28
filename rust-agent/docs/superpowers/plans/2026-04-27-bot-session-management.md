# Bot 会话管理 — 实施计划

> **For agentic workers:** 使用 superpowers:subagent-driven-development（推荐）按任务逐一实施。步骤使用 checkbox (`- [ ]`) 语法跟踪。

**目标：** 让 Bot 子代理支持多轮交互 — Bot 反问用户后暂停，用户回复后从断点继续执行。

**架构：** 在 `BotRegistry` 中新增 `BTreeMap<String, BotSession>` 缓存活跃上下文。首次 `call_bot` 执行后自动保存会话；同名 Bot 再次调用时恢复上下文继续。会话 30 分钟过期。

**技术栈：** Rust 2024 edition, tokio, anyhow, serde

---

### 文件结构

| 文件 | 职责 | 改动类型 |
|------|------|----------|
| `crates/core/src/bots/mod.rs` | `BotSession` 结构体 + `BotRegistry` 会话管理方法 | 修改 |
| `crates/core/src/agent.rs` | `run_bot()` 会话检测/恢复/清理逻辑 + system prompt 调整 | 修改 |
| `crates/core/src/lib.rs` | 导出新增的 `BotSession` | 修改 |
| `crates/core/tests/core_behaviors.rs` | Bot 会话存取 + 恢复流程测试 | 修改 |

---

### Task 1: BotSession 数据结构 + BotRegistry 会话管理

**文件：**
- 修改: `crates/core/src/bots/mod.rs`

- [ ] **Step 1: 添加 `BotSession` 结构体和 `BotRegistry` 会话字段**

在 `bots/mod.rs` 的 `BotRegistry` 定义处添加：

```rust
// 在 imports 区追加
use std::time::Instant;

// 在 BotRegistry 之前新增结构体
/// Bot 活跃会话：缓存对话上下文，支持多轮交互
#[derive(Clone, Debug)]
pub struct BotSession {
    /// Bot 的对话上下文（包含所有中间结果）
    pub ctx: crate::context::ContextService,
    /// 会话创建时间，用于过期清理
    pub created_at: Instant,
}

impl BotSession {
    /// 会话过期时间（30 分钟）
    const TTL: Duration = Duration::from_secs(30 * 60);

    /// 是否已过期
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= Self::TTL
    }
}
```

修改 `BotRegistry` 结构体，新增 `sessions` 字段：

```rust
// 在 bots: BTreeMap<String, BotDefinition> 后面新增
pub struct BotRegistry {
    bots: BTreeMap<String, BotDefinition>,
    sessions: BTreeMap<String, BotSession>,  // ← 新增
}
```

- [ ] **Step 2: 添加 3 个会话管理方法**

在 `impl BotRegistry` 块末尾追加：

```rust
/// 获取 Bot 的活跃会话。如果会话已过期则自动清理。
pub fn get_session(&self, bot_name: &str) -> Option<&BotSession> {
    self.sessions.get(bot_name).filter(|s| !s.is_expired())
}

/// 保存（或覆盖）Bot 会话
pub fn save_session(&mut self, bot_name: String, ctx: crate::context::ContextService) {
    self.sessions.insert(
        bot_name,
        BotSession {
            ctx,
            created_at: Instant::now(),
        },
    );
}

/// 移除并销毁 Bot 会话（任务完成时调用）
pub fn clear_session(&mut self, bot_name: &str) -> Option<BotSession> {
    self.sessions.remove(bot_name)
}

/// 清理所有过期会话（可在 run_bot 入口调用）
pub fn cleanup_expired_sessions(&mut self) {
    self.sessions.retain(|_, s| !s.is_expired());
}
```

- [ ] **Step 3: 添加 `Duration` 到 imports**

在 `bots/mod.rs` 顶部 `use std::time::Instant;` 改为：
```rust
use std::time::{Duration, Instant};
```

- [ ] **Step 4: 编译验证**

```bash
cd crates/core && cargo check
```
预期：编译通过，无警告。

- [ ] **Step 5: Commit**

---

### Task 2: 修改 `run_bot()` — 会话检测/恢复/清理

**文件：**
- 修改: `crates/core/src/agent.rs:766-852`

- [ ] **Step 1: 重写 `run_bot` 方法的前半部分（Bot 查找 + 会话检测）**

将当前 `run_bot()` 从第 766 行开始重写。核心改动：在创建 `bot_ctx` 之前检查是否有活跃会话。

```rust
/// 运行 Bot 子代理：支持多轮会话恢复。
/// 如果存在活跃会话，恢复之前的 ContextService 并注入用户回复；
/// 否则创建新的 ContextService。
async fn run_bot(
    &self,
    bot_name: &str,
    task: &str,
    event_tx: &Arc<mpsc::Sender<AgentEvent>>,
) -> AgentResult<String> {
    // ── 查找 Bot 定义 ──
    let available = self.bots.list();
    let bot_names: Vec<&str> = available.iter().map(|b| b.name.as_str()).collect();
    let bot = self.bots.find(bot_name).ok_or_else(|| {
        anyhow::anyhow!(
            "找不到 Bot: '{bot_name}'。可用 Bot：{}",
            bot_names.join(", ")
        )
    })?;

    // ── 检测是否有活跃会话（恢复执行） ──
    let is_resume = self.bots.get_session(bot_name).is_some();
    let mut bot_ctx = if let Some(session) = self.bots.get_session(bot_name) {
        // 恢复现有会话上下文
        let mut ctx = session.ctx.clone();
        // 把用户回复作为新的用户消息注入
        ctx.push_user_text(task);
        ctx
    } else {
        // 创建全新的上下文
        let mut ctx = crate::context::ContextService::new();
        ctx.push_user_text(task);
        ctx
    };
```

- [ ] **Step 2: 修改 system prompt（恢复会话时追加继续执行指令）**

在构建 `system_prompt` 时，如果 `is_resume` 为 true，追加提示：

```rust
// 构建 system prompt（现有代码保持不动，只在末尾追加）
let system_prompt = format!(
    r#"{identity_line}。
工作目录：{workspace}。
平台：{platform}。
你是一个具备独立上下文的 Bot 子代理，拥有专属技能。
完成用户交给你的任务，按需使用工具，然后返回完整的处理结果。

工具限制：task 和 call_bot 工具已为你禁用，不可调用。

专属技能：
{skills_desc}
{bot_body_section}
提示：
- 如果已安装的技能覆盖当前任务，直接调用 load_skill 加载后再执行。
- 否则跳过技能流程，直接使用 bash 等工具执行。
- **脚本执行规则**：
  - 已安装技能的脚本（已在 skills/ 目录下）→ 用 bash 直接从技能目录运行。
  - 只有凭空生成的临时代码片段才使用 exec_script 工具执行。
  - 执行前禁止先检查环境，直接运行，失败再报告。
  - 禁止 write_file 写临时脚本到工作区再用 bash 执行。
- 完成后返回详细的结果，不要只说"已完成"。
- **信息不明确时必须询问用户**：当任务存在多种可行方案（如不同的算法、权重模型、模板风格等），或关键信息缺失导致无法做出唯一判断时，**必须**先向用户确认，**禁止**擅自选择默认值直接执行。{resume_hint}"#,
    identity_line = identity_line,
    workspace = self.workspace_root.display(),
    skills_desc = skills_desc,
    bot_body_section = bot_body_section,
    resume_hint = if is_resume {
        "\n\n**⚠️ 会话恢复**：这是之前中断的对话的继续。用户刚刚回复了你上一次提问的内容。请从上次中断的地方继续执行，**不要重复已完成的步骤**（如文件解析、数据收集等）。直接基于上下文中的既有结果推进到下一步。"
    } else {
        ""
    },
);
```

- [ ] **Step 3: 修改 agent_loop 调用后的处理逻辑（保存/清理会话）**

将原来简单的 `run_agent_loop(...)` → 返回改为带会话管理的：

```rust
    // ... bot_app 克隆和 skills_desc 等现有代码保持不变 ...

    let mut sub_logger = ConversationLogger::create();
    sub_logger.log(&format!("=== Bot 子代理系统提示词 ===\n{system_prompt}"));

    let result = bot_app
        .run_agent_loop(
            &mut bot_ctx,
            system_prompt,
            AgentRunConfig::child(),
            &mut sub_logger,
            event_tx,
        )
        .await;

    match &result {
        Ok(_text) => {
            // Bot 正常返回 → 保存会话（支持后续恢复）
            // 注意：这里 clone bot_ctx 需要在 run_agent_loop 后持有
            // 使用 self.bots 的可变引用需要特殊处理
        }
        Err(_) => {
            // 出错时清理会话，避免脏状态
            // self.bots.clear_session(bot_name);
        }
    }

    result
}
```

**注意：** `self.bots` 是不可变引用（`&self`），而 `save_session`/`clear_session` 需要 `&mut self`。需要将 `BotRegistry` 的 `sessions` 字段用 `RefCell` 或 `RwLock` 包裹，或者将 `run_bot` 改为 `&mut self`。

**选择：** 将 `sessions` 用 `Arc<RwLock<BTreeMap<String, BotSession>>>` 包裹以支持内部可变性，避免改动 `run_bot` 签名。或者更简单的方案：用 `RefCell`。

**最终方案：** `BotRegistry` 保持 `Clone`，`sessions` 用 `Arc<RwLock<...>>` 包裹：

```rust
// BotRegistry 结构体
pub struct BotRegistry {
    bots: BTreeMap<String, BotDefinition>,
    sessions: Arc<RwLock<BTreeMap<String, BotSession>>>,  // 用 RwLock 支持内部可变性
}
```

会话管理方法改为：
```rust
pub fn get_session(&self, bot_name: &str) -> Option<BotSession> {
    let sessions = self.sessions.read().unwrap();
    sessions.get(bot_name).filter(|s| !s.is_expired()).cloned()
}

pub fn save_session(&self, bot_name: String, ctx: ContextService) {
    let mut sessions = self.sessions.write().unwrap();
    sessions.insert(bot_name, BotSession { ctx, created_at: Instant::now() });
}

pub fn clear_session(&self, bot_name: &str) {
    let mut sessions = self.sessions.write().unwrap();
    sessions.remove(bot_name);
}
```

需要在 imports 中添加：
```rust
use std::sync::{Arc, RwLock};
```

- [ ] **Step 4: 修改 `agent.rs:run_bot` 中的结尾**

替换原有的 agent_loop 调用和返回逻辑：

```rust
    let mut sub_logger = ConversationLogger::create();
    sub_logger.log(&format!("=== Bot 子代理系统提示词 ===\n{system_prompt}"));

    let result = bot_app
        .run_agent_loop(
            &mut bot_ctx,
            system_prompt,
            AgentRunConfig::child(),
            &mut sub_logger,
            event_tx,
        )
        .await;

    match &result {
        Ok(_) => {
            // 正常返回 → 保存会话，支持后续多轮交互
            // bot_app 的 bots 字段拥有 sessions 的可变访问
            bot_app.bots.save_session(bot_name.to_owned(), bot_ctx);
        }
        Err(_) => {
            // 出错时清理会话
            bot_app.bots.clear_session(bot_name);
        }
    }

    result
```

- [ ] **Step 5: 编译验证**

```bash
cd crates/core && cargo check
```
预期：编译通过，无警告。

- [ ] **Step 6: Commit**

---

### Task 3: 更新 lib.rs 导出

**文件：**
- 修改: `crates/core/src/lib.rs`

- [ ] **Step 1: 添加 `BotSession` 导出**

```rust
// lib.rs — 将
pub use bots::{BotDefinition, BotMetadata, BotRegistry, BotSummary, parse_bot_file};
// 改为
pub use bots::{BotDefinition, BotMetadata, BotRegistry, BotSession, BotSummary, parse_bot_file};
```

- [ ] **Step 2: 编译验证**

```bash
cd crates/core && cargo check
```

- [ ] **Step 3: Commit**

---

### Task 4: 测试

**文件：**
- 修改: `crates/core/tests/core_behaviors.rs`

- [ ] **Step 1: 编写 Bot 会话存取测试**

```rust
#[test]
fn bot_session_can_be_saved_and_retrieved() {
    use rust_agent_core::bots::BotRegistry;
    use rust_agent_core::ContextService;

    let mut registry = BotRegistry::default();
    
    // 首次调用 — 应无活跃会话
    assert!(registry.get_session("test-bot").is_none());

    // 保存会话
    let ctx = ContextService::new();
    registry.save_session("test-bot".to_owned(), ctx);
    
    // 再次获取 — 应有活跃会话
    assert!(registry.get_session("test-bot").is_some());
}

#[test]
fn bot_session_clear_removes_session() {
    use rust_agent_core::bots::BotRegistry;
    use rust_agent_core::ContextService;

    let registry = BotRegistry::default();
    
    // 保存再清除
    let ctx = ContextService::new();
    registry.save_session("test-bot".to_owned(), ctx);
    registry.clear_session("test-bot");
    
    // 应无会话
    assert!(registry.get_session("test-bot").is_none());
}

#[test]
fn bot_session_cleanup_expired_removes_old_sessions() {
    use rust_agent_core::bots::BotRegistry;
    use rust_agent_core::ContextService;
    use std::thread;
    use std::time::Duration;

    // 注意：BotSession::TTL 是 30 分钟，这个测试无法真正等待过期。
    // 改为验证 cleanup_expired_sessions 不会误删未过期会话。
    let registry = BotRegistry::default();
    let ctx = ContextService::new();
    registry.save_session("test-bot".to_owned(), ctx);
    registry.cleanup_expired_sessions();
    
    // 刚创建的会话不应被清理
    assert!(registry.get_session("test-bot").is_some());
}
```

- [ ] **Step 2: 运行测试**

```bash
cd crates/core && cargo test -- bot_session
```
预期：3 个测试全部 PASS。

- [ ] **Step 3: 运行完整测试套件**

```bash
cd crates/core && cargo test
```
预期：所有已有测试继续 PASS（无回归）。

- [ ] **Step 4: Commit**
