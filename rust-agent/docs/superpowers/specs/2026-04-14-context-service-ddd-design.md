# 设计文档：轻量 DDD 重构 — 上下文域试点

## 背景

当前 `crates/core` 按技术层组织（api/、tools/、skills/、infra/），随着功能增长，职责边界模糊：
- 对话历史（`Vec<ApiMessage>`）散落在 `agent.rs` 和 `cli/main.rs` 中
- 压缩策略（`infra/compact.rs`）与消息管理分离，但逻辑上高度耦合
- `/clear` 等命令逻辑不存在，需要新增
- CLI 和 Server 各自实现命令解析，重复且不一致

## 目标

采用**轻量 DDD** 模式，以**上下文域**为试点进行渐进式重构：

1. 对话历史、压缩策略归入 `context/` 域，统一管理
2. 命令解析和执行归入 `command/` 域，CLI/Server 共用
3. `agent.rs` 简化为编排层，委托给各 service
4. 为后续域（tool、skill 等）的重构建立模式

## 设计方案

### 目录结构

```
crates/core/src/
├── context/              # 上下文域
│   ├── mod.rs            # ContextService — 对外统一入口
│   ├── history.rs        # Conversation — 对话历史管理
│   ├── compact.rs        # CompactStrategy — 压缩策略
│   └── types.rs          # 域内类型（ContextStats、CompactionResult）
├── command/              # 命令域
│   ├── mod.rs            # CommandDispatcher — 指令分发
│   └── handlers.rs       # 各指令处理器
├── tool/                 # 工具域（tools/ 重命名）
│   ├── mod.rs
│   ├── bash.rs
│   ├── file_ops.rs
│   ├── search.rs
│   ├── skill_ops.rs
│   └── schemas.rs
├── skill/                # 技能域（skills/ 重命名）
│   ├── mod.rs
│   └── hub.rs
├── api/                  # 基础设施：LLM API（不变）
├── infra/                # 基础设施（缩减）
│   ├── config.rs
│   ├── logging.rs
│   ├── storage.rs
│   ├── usage.rs
│   ├── todo.rs
│   └── utils.rs
├── agent.rs              # 编排层（简化）
└── lib.rs
```

### 上下文域（context/）

#### Conversation（history.rs）

封装对话历史的所有操作，是上下文域的核心数据结构。

```rust
pub struct Conversation {
    messages: Vec<ApiMessage>,
}

impl Conversation {
    pub fn new() -> Self;
    pub fn push(&mut self, msg: ApiMessage);
    pub fn clear(&mut self);
    pub fn truncate(&mut self, keep_last: usize);
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn estimate_tokens(&self) -> usize;
    pub fn messages(&self) -> &[ApiMessage];
    pub fn messages_mut(&mut self) -> &mut Vec<ApiMessage>;
}
```

#### CompactStrategy（compact.rs）

将压缩策略抽象为 trait，便于测试和扩展。

```rust
pub trait CompactStrategy {
    /// 判断是否需要执行压缩
    fn should_compact(&self, conv: &Conversation) -> bool;
    /// 执行压缩，返回压缩结果
    fn compact(
        &self,
        conv: &mut Conversation,
        logger: &mut ConversationLogger,
    ) -> AgentResult<CompactionResult>;
}

pub struct ThreeTierCompact {
    micro: MicroCompact,
    auto: AutoCompact,
    manual: ManualCompact,
}

impl ThreeTierCompact {
    /// 依次检查并执行三层压缩
    pub fn run(&self, conv: &mut Conversation, logger: &mut ConversationLogger) -> AgentResult<bool>;
}
```

#### ContextService（mod.rs）

上下文域的统一入口，对外暴露简洁 API。

```rust
pub struct ContextService {
    conversation: Conversation,
    compactor: ThreeTierCompact,
}

impl ContextService {
    pub fn new(compactor: ThreeTierCompact) -> Self;

    // 读取
    pub fn messages(&self) -> &[ApiMessage];
    pub fn messages_mut(&mut self) -> &mut Vec<ApiMessage>;
    pub fn stats(&self) -> ContextStats;

    // 写入
    pub fn push(&mut self, msg: ApiMessage);
    pub fn clear(&mut self) -> ContextStats;

    // 压缩
    pub fn maybe_compact(&mut self, logger: &mut ConversationLogger) -> AgentResult<bool>;
}
```

#### 域内类型（types.rs）

```rust
/// 上下文统计信息
pub struct ContextStats {
    pub message_count: usize,
    pub estimated_tokens: usize,
    pub cleared_count: usize,  // 清空时的消息数
}

/// 压缩结果
pub struct CompactionResult {
    pub compacted: bool,
    pub strategy: String,       // "micro" | "auto" | "manual"
    pub messages_before: usize,
    pub messages_after: usize,
    pub saved_tokens: usize,
}
```

### 命令域（command/）

#### CommandDispatcher（mod.rs）

```rust
/// 用户指令枚举
pub enum UserCommand {
    Clear,
    Compact,
    Help,
    Stats,
    Quit,
}

/// 命令分发器
pub struct CommandDispatcher<'a> {
    context: &'a mut ContextService,
}

impl<'a> CommandDispatcher<'a> {
    /// 解析用户输入，匹配 /command 格式
    pub fn parse(input: &str) -> Option<UserCommand>;

    /// 执行命令
    pub fn execute(
        &mut self,
        cmd: UserCommand,
        logger: &mut ConversationLogger,
    ) -> CommandResult;
}

/// 命令执行结果
pub struct CommandResult {
    pub output: String,       // 给用户的反馈文本
    pub should_quit: bool,    // 是否退出
}
```

#### 指令处理器（handlers.rs）

```rust
impl<'a> CommandDispatcher<'a> {
    fn handle_clear(&mut self) -> CommandResult {
        let stats = self.context.clear();
        CommandResult {
            output: format!("上下文已清空（清除 {} 条消息）", stats.cleared_count),
            should_quit: false,
        }
    }

    fn handle_compact(&mut self, logger: &mut ConversationLogger) -> CommandResult {
        match self.context.maybe_compact(logger) {
            Ok(true) => CommandResult { output: "压缩完成".into(), should_quit: false },
            Ok(false) => CommandResult { output: "当前不需要压缩".into(), should_quit: false },
            Err(e) => CommandResult { output: format!("压缩失败: {e}"), should_quit: false },
        }
    }

    fn handle_stats(&self) -> CommandResult {
        let stats = self.context.stats();
        CommandResult {
            output: format!("消息数: {} | 预估 token: {}", stats.message_count, stats.estimated_tokens),
            should_quit: false,
        }
    }
}
```

### 编排层变化（agent.rs）

`AgentApp` 不再直接持有 `Vec<ApiMessage>`，改为持有 `ContextService`：

```rust
pub struct AgentApp {
    client: LlmProvider,
    workspace_root: PathBuf,
    context: ContextService,        // 替代原来的 messages 参数
    skills: Arc<RwLock<SkillLoader>>,
    // ...
}

impl AgentApp {
    pub async fn handle_user_turn(&self, user_input: &str, ...) -> AgentResult<String> {
        // 1. 压缩检查
        self.context.maybe_compact(&mut logger)?;

        // 2. 添加用户消息
        self.context.push(ApiMessage::user(user_input));

        // 3. 构建请求（从 context.messages() 获取历史）
        let request = self.build_request(self.context.messages(), ...);

        // 4. 调用 LLM + 工具循环
        // ...
    }
}
```

### 入口端变化

CLI 和 Server 都变成薄壳：

```rust
// cli/src/main.rs
let mut app = AgentApp::new(...)?;
let mut logger = ConversationLogger::new(...);

loop {
    let input = readline()?;

    match CommandDispatcher::parse(&input) {
        Some(UserCommand::Quit) => break,
        Some(cmd) => {
            let mut dispatcher = CommandDispatcher::new(&mut app.context);
            let result = dispatcher.execute(cmd, &mut logger);
            println!("{}", result.output);
            if result.should_quit { break; }
        }
        None => {
            // 普通对话
            let response = app.handle_user_turn(&input, ...).await?;
            println!("{response}");
        }
    }
}
```

## 实施步骤

1. **创建 context/ 模块**：将 `infra/compact.rs` 的压缩逻辑迁移到 `context/compact.rs`，新建 `history.rs` 和 `types.rs`
2. **实现 ContextService**：组合 Conversation + ThreeTierCompact
3. **创建 command/ 模块**：实现 CommandDispatcher 和各指令处理器
4. **重构 agent.rs**：将 `Vec<ApiMessage>` 替换为 `ContextService`
5. **重构 CLI 入口**：使用 CommandDispatcher 处理指令
6. **重构 Server 入口**：同步适配
7. **清理**：移除旧的散落逻辑，更新 mod.rs 导出

## 不做的事

- 不引入完整的 Aggregate/Entity/ValueObject 战术模式
- 不引入 Repository 层（对话历史暂无持久化需求）
- 不拆分 domain/application/infrastructure 三层目录
- 不做 tool/ 和 skill/ 的重构（留给后续迭代）
