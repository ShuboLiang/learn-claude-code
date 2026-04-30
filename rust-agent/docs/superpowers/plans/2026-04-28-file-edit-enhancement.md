# File Edit 工具增强实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将现有 `edit_file` 工具增强至与 Python `file_edit_tool.py` 功能对齐，支持 replace_all、unified diff 输出、CRLF 行尾保留、重复检测。

**Architecture:** 修改 3 个文件——schema 定义 (`schemas.rs`)、核心逻辑 (`file_ops.rs`)、工具调度 (`mod.rs`)。核心逻辑直接在 `AgentToolbox::edit_file` 方法中实现，不拆分到 infra。

**Tech Stack:** Rust, std::fs (二进制读写), 手写 unified diff (无外部依赖)

---

### Task 1: 更新 edit_file 的 JSON Schema

**Files:**
- Modify: `crates/core/src/tools/schemas.rs:72-84`

- [ ] **Step 1: 在 schemas.rs 中更新 edit_file 的 schema 定义**

将参数名 `path`/`old_text`/`new_text` 改为 `file_path`/`old_string`/`new_string`，新增 `replace_all` boolean 参数，更新描述：

```rust
json!({
    "name": "edit_file",
    "description": "对文件执行精确字符串替换。old_string 在文件中必须唯一（出现一次），否则需设置 replace_all: true 替换所有匹配项，或提供更多上下文使匹配唯一。",
    "input_schema": {
        "type": "object",
        "properties": {
            "file_path": { "type": "string", "description": "要修改的文件的绝对路径" },
            "old_string": { "type": "string", "description": "要被替换的文本" },
            "new_string": { "type": "string", "description": "替换后的新文本（必须与 old_string 不同）" },
            "replace_all": { "type": "boolean", "description": "替换所有匹配项（可选，默认 false）" }
        },
        "required": ["file_path", "old_string", "new_string"]
    }
}),
```

- [ ] **Step 2: Commit schema change**

```bash
git add crates/core/src/tools/schemas.rs
git commit -m "feat(tools): update edit_file schema with replace_all and renamed params"
```

---

### Task 2: 增强 edit_file 核心逻辑

**Files:**
- Modify: `crates/core/src/tools/file_ops.rs:41-58`

- [ ] **Step 1: 重写 edit_file 方法**

新签名：`fn edit_file(&self, file_path: &str, old_string: &str, new_string: &str, replace_all: bool) -> AgentResult<String>`

实现逻辑：

```rust
/// 在文件中精确替换文本，返回 unified diff 格式的变更摘要
///
/// 使用二进制读写保留原始行尾（CRLF/LF），与 Python file_edit_tool.py 行为一致。
pub(crate) fn edit_file(
    &self,
    file_path: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> AgentResult<String> {
    let resolved = resolve_workspace_path(&self.workspace_root, file_path)?;

    // 1. 校验文件存在
    if !resolved.is_file() {
        return Ok(format!("错误：文件不存在：{}", resolved.display()));
    }

    // 2. 校验 old_string != new_string
    if old_string == new_string {
        return Ok("错误：old_string 与 new_string 必须不同".to_string());
    }

    // 3. 二进制读取保留原始行尾
    let raw_bytes = std::fs::read(&resolved)
        .with_context(|| format!("无法读取文件：{}", resolved.display()))?;
    let content = String::from_utf8(raw_bytes)
        .with_context(|| format!("文件不是有效的 UTF-8：{}", resolved.display()))?;

    // 4. 检查 old_string 是否存在
    let count = content.matches(old_string).count();
    if count == 0 {
        return Ok(format!("错误：在 {} 中未找到 old_string", file_path));
    }

    // 5. 重复检测
    if count > 1 && !replace_all {
        return Ok(format!(
            "错误：old_string 在 {} 中出现了 {} 次。请设置 replace_all: true 替换全部，或提供更多上下文使匹配唯一。",
            file_path, count
        ));
    }

    // 6. 执行替换
    let updated = if replace_all {
        content.replace(old_string, new_string)
    } else {
        content.replacen(old_string, new_string, 1)
    };

    // 7. 二进制写回
    std::fs::write(&resolved, updated.as_bytes())
        .with_context(|| format!("无法写入文件：{}", resolved.display()))?;

    // 8. 生成 unified diff
    let diff = generate_unified_diff(&content, &updated, file_path);

    Ok(truncate_text(&diff, 50_000))
}

/// 手写 unified diff 格式（类似 Python difflib.unified_diff）
fn generate_unified_diff(old: &str, new: &str, filename: &str) -> String {
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();

    // 简单的逐行比较 diff
    // 对于精确字符串替换场景，只需找出变化的起始和结束行
    let mut diff = format!("--- {filename}\n+++ {filename}\n");

    // 找到第一个不同的行
    let mut start = 0usize;
    while start < old_lines.len() && start < new_lines.len() && old_lines[start] == new_lines[start] {
        start += 1;
    }

    // 找到最后一个不同的行
    let mut old_end = old_lines.len();
    let mut new_end = new_lines.len();
    while old_end > start && new_end > start && old_lines[old_end - 1] == new_lines[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }

    // 上下文行数
    let context = 3usize;
    let ctx_start = start.saturating_sub(context);
    let old_ctx_end = (old_end + context).min(old_lines.len());
    let new_ctx_end = (new_end + context).min(new_lines.len());

    diff.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        ctx_start + 1,
        old_ctx_end - ctx_start,
        ctx_start + 1,
        new_ctx_end - ctx_start,
    ));

    // 输出上下文 + 删除行
    for i in ctx_start..old_ctx_end {
        if i >= start && i < old_end {
            diff.push_str(&format!("-{}", old_lines[i]));
        } else {
            diff.push_str(&format!(" {}", old_lines[i]));
        }
    }
    // 输出新增行（不在 old 范围内的部分）
    for i in old_ctx_end..new_ctx_end {
        diff.push_str(&format!("+{}", new_lines[i]));
    }

    diff
}
```

- [ ] **Step 2: 运行 cargo check 确认编译通过**

```bash
cargo check -p rust-agent-core
```
期望：编译通过（可能 task 2 还没改 dispatch，先确认 file_ops.rs 本身无语法错误）

- [ ] **Step 3: Commit**

```bash
git add crates/core/src/tools/file_ops.rs
git commit -m "feat(tools): enhance edit_file with replace_all, CRLF preservation, unified diff"
```

---

### Task 3: 更新工具调度层

**Files:**
- Modify: `crates/core/src/tools/mod.rs:170-174`

- [ ] **Step 1: 新增 optional_bool 辅助函数**

在 `mod.rs` 的辅助函数区域添加：

```rust
/// 从 JSON 对象中提取可选的 bool 字段值
pub(crate) fn optional_bool(input: &Value, key: &str) -> AgentResult<bool> {
    match input.get(key) {
        Some(value) => value
            .as_bool()
            .ok_or_else(|| anyhow!("字段 '{key}' 必须是布尔值")),
        None => Ok(false),
    }
}
```

- [ ] **Step 2: 更新 dispatch 中 edit_file 的调用点**

在 `mod.rs` 第 170-174 行，将：

```rust
"edit_file" => self.edit_file(
    required_string(input, "path")?,
    required_string(input, "old_text")?,
    required_string(input, "new_text")?,
)?,
```

改为：

```rust
"edit_file" => self.edit_file(
    required_string(input, "file_path")?,
    required_string(input, "old_string")?,
    required_string(input, "new_string")?,
    optional_bool(input, "replace_all")?,
)?,
```

- [ ] **Step 3: 运行 cargo check 确认整个项目编译通过**

```bash
cargo check -p rust-agent-core
```
期望：编译通过

- [ ] **Step 4: Commit**

```bash
git add crates/core/src/tools/mod.rs
git commit -m "feat(tools): update edit_file dispatch with new params and optional_bool helper"
```

---

### 验证清单

- [ ] `cargo build -p rust-agent-core` 编译通过
- [ ] `cargo test -p rust-agent-core` 现有测试仍通过
- [ ] Schema 中参数名与 dispatch 中提取的参数名一致
- [ ] 替换逻辑与 Python 实现行为对齐：唯一替换、全量替换、重复检测报错、diff 输出
