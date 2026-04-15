# 并行 Subagent 执行 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 agent 支持并行执行多个 subagent，当 LLM 一次返回多个 `task` 工具调用时自动并行执行，并通过 system prompt 引导 LLM 自主判断何时并行、何时串行。

**Architecture:** 在 `agent.rs` 的工具执行循环中，检测一轮内所有 `task` 类型的 tool_calls，将它们从串行执行改为 `tokio::spawn` 并行执行。`event_tx` 用 `Arc` 包装以支持跨 spawned task 共享。并行结果按原始顺序合并为 `tool_result` 数组返回给 LLM。`handle_user_turn` 的公开 API 签名保持不变（`mpsc::Sender<AgentEvent>`），Arc 包装仅在内部使用，因此 `routes.rs` 和 `cli/main.rs` 的调用方无需修改。System prompt 中加入并行/串行路由规则。事件流增加并行标识信息。

**Tech Stack:** Rust, tokio (已有), async-recursion (已有)

---

## 文件变更清单

| 文件 | 操作 | 职责 |
|------|------|------|
| `crates/core/src/agent.rs` | 修改 | 核心变更：并行 task 检测与执行、system prompt 路由规则、事件流并行标识、Arc event_tx |
| `crates/cli/src/main.rs` | 修改 | CLI 渲染：显示并行 subagent 的 `[并行 1/3]` 标识 |
| `crates/server/src/sse.rs` | 修改 | SSE：传递并行标识信息给前端 |
| `crates/server/src/openai_compat.rs` | 修改 | OpenAI 兼容接口：适配 AgentEvent 新增的 parallel_index 字段 |

注意：`routes.rs` 无需修改，因为 `handle_user_turn` 的公开签名保持不变。

---

### Task 1: 添加并行 subagent 最大数量常量和事件变体

**Files:**
- Modify: `crates/core/src/agent.rs:39-40` (常量区域)
- Modify: `crates/core/src/agent.rs:22-37` (AgentEvent 枚举)
- Modify: `crates/server/src/sse.rs:11-16` (模式匹配)
- Modify: `crates/server/src/openai_compat.rs:204` (模式匹配)
- Modify: `crates/cli/src/main.rs:86-113` (模式匹配)

- [ ] **Step 1: 在 `MAX_TOOL_ROUNDS` 旁添加并行最大数量常量**

在 `agent.rs` 第 40 行 `MAX_TOOL_ROUNDS` 之后添加：

```rust
/// 单轮中允许并行执行的 subagent 最大数量，防止 API 过载
const MAX_PARALLEL_TASKS: usize = 5;
```

- [ ] **Step 2: 在 `AgentEvent` 变体中添加并行标识字段**

将 `AgentEvent` 枚举修改为：

```rust
#[derive(Clone, Debug)]
pub enum AgentEvent {
    /// AI 回复的文本片段
    TextDelta(String),
    /// 即将调用工具
    ToolCall {
        name: String,
        input: serde_json::Value,
        /// 并行执行时的序号标识，如 Some((1, 3)) 表示"并行第 1 个，共 3 个"
        parallel_index: Option<(usize, usize)>,
    },
    /// 工具执行结果
    ToolResult {
        name: String,
        output: String,
        /// 并行执行时的序号标识
        parallel_index: Option<(usize, usize)>,
    },
    /// 本轮对话结束
    TurnEnd,
    /// Agent 完成全部任务
    Done,
}
```

- [ ] **Step 3: 修复 `agent.rs` 中所有 `AgentEvent` 构造点**

在 `agent.rs` 中搜索所有 `AgentEvent::ToolCall { name, input }` 和 `AgentEvent::ToolResult { name, output }`，添加 `parallel_index: None`。

将所有 `{ name: ..., input: ... }` 改为 `{ name: ..., input: ..., parallel_index: None }`。
将所有 `{ name: ..., output: ... }` 改为 `{ name: ..., output: ..., parallel_index: None }`。

- [ ] **Step 4: 修复 `sse.rs` 中的模式匹配，传递并行标识给前端**

`sse.rs` 第 11-16 行修改为：
```rust
AgentEvent::ToolCall { name, input, parallel_index } => {
    let mut data = json!({ "name": name, "input": input });
    if let Some((idx, total)) = parallel_index {
        data["parallel_index"] = json!({ "index": idx, "total": total });
    }
    Event::default().event("tool_call").data(data.to_string())
}
AgentEvent::ToolResult { name, output, parallel_index } => {
    let mut data = json!({ "name": name, "output": output });
    if let Some((idx, total)) = parallel_index {
        data["parallel_index"] = json!({ "index": idx, "total": total });
    }
    Event::default().event("tool_result").data(data.to_string())
}
```

- [ ] **Step 5: 修复 `openai_compat.rs` 中的模式匹配**

`openai_compat.rs` 第 204 行修改为：
```rust
rust_agent_core::agent::AgentEvent::ToolCall { name, input, parallel_index: _ } => {
```

- [ ] **Step 6: 修复 `cli/main.rs` 中的模式匹配，显示并行标识**

`cli/main.rs` 第 86-113 行修改为：
```rust
AgentEvent::ToolCall { name, input, parallel_index } => {
    // 提取关键参数显示
    let detail = match name.as_str() {
        "bash" => input.get("command").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
        "read_file" => input.get("path").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
        "write_file" => format!("{} ({} 字节)", input.get("path").and_then(|v| v.as_str()).unwrap_or(""), input.get("content").map(|v| v.as_str().map(|s| s.len()).unwrap_or(0)).unwrap_or(0)),
        "edit_file" => input.get("path").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
        "glob" => input.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
        "grep" => {
            let mut parts = vec![input.get("pattern").and_then(|v| v.as_str()).unwrap_or("").to_owned()];
            if let Some(p) = input.get("path").and_then(|v| v.as_str()) { parts.push(p.to_owned()); }
            parts.join(" in ")
        }
        "todo" => {
            let items = input.get("items").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            format!("{items} 项")
        }
        "task" => input.get("description").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
        _ => input.to_string(),
    };
    let tag = match parallel_index {
        Some((idx, total)) => format!("[并行 {idx}/{total}] "),
        None => String::new(),
    };
    println!("┌─ {tag}{name}: `{detail}`");
}
AgentEvent::ToolResult { name: _, output, parallel_index } => {
    let tag = match parallel_index {
        Some((idx, total)) => format!("[并行 {idx}/{total}] "),
        None => String::new(),
    };
    for line in output.lines() {
        println!("│  {tag}{line}");
    }
    println!("└─");
}
```

- [ ] **Step 7: 编译验证**

Run: `cargo check 2>&1 | head -50`
Expected: 编译通过，无错误

- [ ] **Step 8: Commit**

```bash
git add crates/core/src/agent.rs crates/server/src/sse.rs crates/server/src/openai_compat.rs crates/cli/src/main.rs
git commit -m "refactor: AgentEvent 添加并行标识字段，为并行 subagent 做准备"
```

---

### Task 2: 实现并行 task 检测与执行逻辑

**Files:**
- Modify: `crates/core/src/agent.rs:134-167` (handle_user_turn — 内部 Arc 包装)
- Modify: `crates/core/src/agent.rs:170-362` (run_agent_loop — 核心循环重写)

这是核心变更。当前逻辑是逐个遍历 `response.content` 中的 tool_calls 串行执行。需要改为：
1. 先遍历所有 tool_calls，将 `task` 类型的和非 `task` 类型的分离
2. 非 task 工具串行执行（保持现有行为）
3. task 工具使用 `tokio::spawn` 并行执行（每个 subagent 独立 logger）
4. 结果按原始顺序合并

关键设计决策：
- `event_tx` 需要用 `Arc<mpsc::Sender<AgentEvent>>` 包装，因为 `tokio::spawn` 需要 `'static` 生命周期
- `handle_user_turn` 的公开签名不变，Arc 包装仅在内部使用
- 每个 spawned subagent 创建独立的 `ConversationLogger`，不存在跨线程共享问题

- [ ] **Step 1: 修改 `handle_user_turn` 内部使用 Arc 包装 event_tx**

将 `handle_user_turn` 方法内部修改为：

```rust
pub async fn handle_user_turn(
    &self,
    history: &mut Vec<ApiMessage>,
    user_input: &str,
    event_tx: mpsc::Sender<AgentEvent>,
) -> AgentResult<String> {
    let mut logger = ConversationLogger::create();

    history.push(ApiMessage::user_text(user_input));
    logger.log(&format!("=== 用户 ===\n{user_input}"));

    let system_prompt = build_system_prompt(
        &self.workspace_root,
        &self.skills.read().unwrap().descriptions_for_system_prompt(),
    );
    logger.log(&format!("=== 系统提示词 ===\n{system_prompt}"));

    // 内部用 Arc 包装 event_tx，使 run_agent_loop 可跨 spawned task 共享
    let event_tx = Arc::new(event_tx);

    let result = self
        .run_agent_loop(
            history,
            system_prompt,
            AgentRunConfig::parent(),
            &mut logger,
            &event_tx,
        )
        .await;

    match &result {
        Ok(text) => logger.log(&format!("=== 助手 ===\n{text}")),
        Err(e) => logger.log(&format!("=== 错误 ===\n{e}")),
    }

    result
}
```

- [ ] **Step 2: 修改 `run_agent_loop` 和 `run_subagent` 签名**

`run_agent_loop` 签名改为（注意 `event_tx` 类型变化）：

```rust
#[async_recursion]
async fn run_agent_loop(
    &self,
    messages: &mut Vec<ApiMessage>,
    system_prompt: String,
    config: AgentRunConfig,
    logger: &mut ConversationLogger,
    event_tx: &Arc<mpsc::Sender<AgentEvent>>,
) -> AgentResult<String> {
```

`run_subagent` 签名同步修改：

```rust
async fn run_subagent(
    &self,
    prompt: String,
    logger: &mut ConversationLogger,
    event_tx: &Arc<mpsc::Sender<AgentEvent>>,
) -> AgentResult<String> {
```

同时需要在文件顶部 import 中添加 `use std::sync::Arc;`（如果尚未导入）。

- [ ] **Step 3: 重写工具执行循环，分离 task 和非 task 调用**

将 `run_agent_loop` 中 `for _ in 0..MAX_TOOL_ROUNDS` 循环内的工具执行部分（从 `let mut results = Vec::new();` 到 `messages.push(ApiMessage::user_blocks(results));`）替换为以下完整逻辑：

```rust
let mut results = Vec::new();
let mut used_todo = false;
let mut manual_compact = false;

// 第一遍：收集所有 tool_calls
struct ToolCallInfo {
    id: String,
    name: String,
    input: Value,
}

let tool_calls: Vec<ToolCallInfo> = response
    .content
    .iter()
    .filter_map(|block| {
        if let ResponseContentBlock::ToolUse { id, name, input } = block {
            Some(ToolCallInfo {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            })
        } else {
            None
        }
    })
    .collect();

// 分离 task 和非 task 调用，保持原始顺序
let mut task_calls: Vec<ToolCallInfo> = Vec::new();
let mut other_calls: Vec<ToolCallInfo> = Vec::new();
for tc in tool_calls {
    if tc.name == "task" {
        task_calls.push(tc);
    } else {
        other_calls.push(tc);
    }
}

// === 串行执行非 task 工具 ===
for tc in &other_calls {
    let input_preview = preview_text(&tc.input.to_string(), 200);
    logger.log(&format!("=== 工具调用: {} ===\n输入: {input_preview}", tc.name));

    let output = if tc.name == "compact" {
        manual_compact = true;
        let _ = event_tx
            .send(AgentEvent::ToolCall {
                name: tc.name.clone(),
                input: tc.input.clone(),
                parallel_index: None,
            })
            .await;
        "正在压缩...".to_owned()
    } else {
        match toolbox.dispatch(&tc.name, &tc.input).await {
            Ok(dispatch) => {
                used_todo |= dispatch.used_todo;
                let _ = event_tx
                    .send(AgentEvent::ToolCall {
                        name: tc.name.clone(),
                        input: tc.input.clone(),
                        parallel_index: None,
                    })
                    .await;
                let _ = event_tx
                    .send(AgentEvent::ToolResult {
                        name: tc.name.clone(),
                        output: preview_text(&dispatch.output, 200),
                        parallel_index: None,
                    })
                    .await;
                dispatch.output
            }
            Err(e) => {
                let msg = format!("Error: {e}");
                let _ = event_tx
                    .send(AgentEvent::ToolResult {
                        name: tc.name.clone(),
                        output: msg.clone(),
                        parallel_index: None,
                    })
                    .await;
                msg
            }
        }
    };

    logger.log(&format!("=== 工具结果: {} ===\n{output}", tc.name));
    let processed_output = storage::maybe_persist(&tc.id, &output);
    results.push(tool_result_block(&tc.id, processed_output));
}

// === 并行执行 task 工具（subagent） ===
if !task_calls.is_empty() {
    if !config.allow_task {
        // 子代理不允许使用 task
        for tc in &task_calls {
            results.push(tool_result_block(
                &tc.id,
                "错误：task 工具在子代理中不可用".to_owned(),
            ));
        }
    } else {
        let total = task_calls.len().min(MAX_PARALLEL_TASKS);
        let actual_calls: Vec<_> = task_calls.into_iter().take(total).collect();
        let is_parallel = actual_calls.len() > 1;

        // 发送 ToolCall 事件
        for (idx, tc) in actual_calls.iter().enumerate() {
            let input_preview = preview_text(&tc.input.to_string(), 200);
            logger.log(&format!(
                "=== 工具调用: task (并行 {}/{}) ===\n输入: {input_preview}",
                idx + 1,
                actual_calls.len()
            ));
            let _ = event_tx
                .send(AgentEvent::ToolCall {
                    name: "task".to_owned(),
                    input: tc.input.clone(),
                    parallel_index: if is_parallel {
                        Some((idx + 1, actual_calls.len()))
                    } else {
                        None
                    },
                })
                .await;
        }

        // 为每个 subagent 创建独立的 logger 并行运行
        let mut handles = Vec::new();
        for tc in &actual_calls {
            let prompt = tc
                .input
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let app = self.clone();
            let event_tx = Arc::clone(event_tx);
            handles.push(tokio::spawn(async move {
                let mut sub_logger = ConversationLogger::create();
                let result = app.run_subagent(prompt, &mut sub_logger, &event_tx).await;
                (result, sub_logger)
            }));
        }

        // 收集结果（按 spawn 顺序，tokio 保证 JoinHandle await 顺序）
        let mut sub_results: Vec<(String, ConversationLogger)> = Vec::new();
        for handle in handles {
            match handle.await {
                Ok((Ok(output), sub_logger)) => {
                    sub_results.push((output, sub_logger));
                }
                Ok((Err(e), sub_logger)) => {
                    let msg = format!("子代理执行失败: {e}");
                    sub_results.push((msg, sub_logger));
                }
                Err(e) => {
                    let msg = format!("子代理任务异常: {e}");
                    sub_results.push((msg, ConversationLogger::create()));
                }
            }
        }

        // 发送 ToolResult 事件并构建 tool_result 块
        for (idx, (output, _sub_logger)) in sub_results.iter().enumerate() {
            let _ = event_tx
                .send(AgentEvent::ToolResult {
                    name: "task".to_owned(),
                    output: preview_text(output, 200),
                    parallel_index: if is_parallel {
                        Some((idx + 1, actual_calls.len()))
                    } else {
                        None
                    },
                })
                .await;
            logger.log(&format!(
                "=== 工具结果: task (并行 {}/{}) ===\n{output}",
                idx + 1,
                actual_calls.len()
            ));
            let tc_id = &actual_calls[idx].id;
            let processed = storage::maybe_persist(tc_id, output);
            results.push(tool_result_block(tc_id, processed));
        }
    }
}
```

- [ ] **Step 4: 编译验证**

Run: `cargo check 2>&1 | head -50`
Expected: 编译通过

- [ ] **Step 5: 运行现有测试**

Run: `cargo test 2>&1 | tail -20`
Expected: 所有测试通过

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/agent.rs
git commit -m "feat: 支持并行执行多个 subagent（多 task tool_calls 自动并行）"
```

---

### Task 3: 更新 System Prompt 添加并行/串行路由规则

**Files:**
- Modify: `crates/core/src/agent.rs` (build_system_prompt 函数)

- [ ] **Step 1: 在 system prompt 中加入并行路由规则**

修改 `build_system_prompt` 函数，在"输出规则"和"其他工具"之间添加并行路由指导：

```rust
fn build_system_prompt(workspace_root: &std::path::Path, skills_desc: &str) -> String {
    let platform = if cfg!(windows) {
        "Windows (PowerShell)。使用 PowerShell 语法：用 Get-ChildItem 代替 ls，Get-Content 代替 cat，-Command 代替 -lc，; 代替 &&"
    } else {
        "Unix (bash)"
    };
    format!(
        "你是一个编程助手，工作目录：{}。\n平台：{platform}\n优先使用工具解决问题，避免冗长解释。\n\n\
        任务执行流程 — 每个任务必须按以下顺序执行：\n\
        0. 先了解项目：读取目录结构和关键文件，理解项目上下文。\n\
        1. 检查已安装的技能是否覆盖当前任务。如果有，调用 load_skill。\n\
        2. 如果没有匹配的已安装技能，必须调用 search_skillhub 搜索。\n\
        3. 如果 search_skillhub 返回了相关技能，调用 install_skill 安装它。\n\
        4. 只有在步骤 0-3 完成（且未找到技能）后，才能使用 bash 或其他工具执行具体操作。\n\
        5. 绝对不能跳过技能检查直接使用 bash/curl 等工具。\n\
        6. 在完成技能流程之前，绝对不能声称无法完成任务。\n\n\
        输出规则：\n\
        - 研究类任务：收集完资料后必须输出完整内容，不能只说整理完毕。\n\
        - 长篇内容（>500字）应写入文件并告知用户文件路径。\n\
        - 如果工具结果被持久化到磁盘（包含 <persisted-output> 标签），可以随时用 read_file 读取完整内容。\n\n\
        子代理并行执行规则：\n\
        - 你可以在一次响应中返回多个 task 工具调用来并行执行多个子代理。\n\
        - **并行执行条件**（需全部满足）：2+ 个独立任务、任务间无依赖、无共享文件冲突。\n\
        - **串行执行条件**（任一触发）：任务间有依赖、共享文件/状态、范围不明确需先了解。\n\
        - 典型并行场景：同时研究多个不相关主题、同时探索不同模块、同时分析多个文件。\n\
        - 典型串行场景：先调研再实现、先写 schema 再写 API、需要前一步结果才能决定下一步。\n\
        - 并行上限为 {MAX_PARALLEL_TASKS} 个子代理，超出部分将被忽略。\n\n\
        其他工具：\n\
        - 使用 todo 工具规划多步骤工作。\n\
        - 使用 task 工具委派子任务（子代理拥有独立上下文，支持并行）。\n\n\
        可用技能：\n{}",
        workspace_root.display(),
        skills_desc
    )
}
```

注意：并行阈值设为 "2+"，与代码中 `actual_calls.len() > 1` 一致。

- [ ] **Step 2: 编译验证**

Run: `cargo check 2>&1 | head -20`
Expected: 编译通过

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/agent.rs
git commit -m "feat: system prompt 添加并行/串行 subagent 路由规则"
```

---

### Task 4: 端到端验证

- [ ] **Step 1: 构建项目**

Run: `cargo build --release 2>&1 | tail -10`
Expected: 构建成功

- [ ] **Step 2: 运行所有测试**

Run: `cargo test 2>&1 | tail -20`
Expected: 所有测试通过

- [ ] **Step 3: 手动测试并行 subagent**

启动 agent，输入一个明确需要并行处理的请求，例如：

```
请同时帮我做以下 3 件事：
1. 列出当前目录下所有 .rs 文件
2. 列出当前目录下所有 .toml 文件
3. 列出当前目录下所有 .md 文件
```

Expected: CLI 显示 `[并行 1/3]`、`[并行 2/3]`、`[并行 3/3]` 标识，三个 subagent 并行执行。

- [ ] **Step 4: 手动测试串行 subagent**

输入一个需要串行处理的请求：

```
先读取 Cargo.toml 了解项目依赖，然后根据依赖信息搜索是否有相关技能可以安装。
```

Expected: 只有一个 task 调用，无并行标识，串行执行。

- [ ] **Step 5: Final commit（如有修复）**

```bash
git add -A
git commit -m "fix: 并行 subagent 端到端验证修复"
```
