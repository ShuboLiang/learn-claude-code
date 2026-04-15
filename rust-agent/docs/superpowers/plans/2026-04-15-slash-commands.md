# Slash 命令实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 在 Node.js CLI 中添加 `/help`、`/clear`、`/skills`、`/compact` slash 命令

**Architecture:** CLI 拦截 `/` 开头的输入，本地命令直接处理，server 命令通过 API 调用。命令输出作为 system 消息显示在聊天中。

**Tech Stack:** TypeScript, Ink (React for CLI), Axum (Rust server)

---

### Task 1: 新增 server 端点（skills 和 compact）

**Files:**
- Modify: `crates/server/src/routes.rs`
- Modify: `crates/server/src/session.rs`

- [ ] **Step 1: 在 Session 中暴露 skills 列表和 context**

修改 `crates/server/src/session.rs`，给 `Session` 添加从 agent 获取 skills 的方法：

```rust
// 在 Session impl 中添加
pub fn list_skills(&self) -> Vec<rust_agent_core::skills::SkillSummary> {
    self.agent.skills.read().unwrap().list_skills()
}

pub fn compact_context(&mut self) {
    self.context.micro_compact();
}
```

- [ ] **Step 2: 添加 GET /sessions/{id}/skills 端点**

修改 `crates/server/src/routes.rs`：

```rust
.route("/sessions/{id}/skills", get(list_skills))
.route("/sessions/{id}/compact", post(compact_session))
```

```rust
/// GET /sessions/:id/skills — 列出可用技能
async fn list_skills(
    State(store): State<SessionStore>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match store.get(&id) {
        Some(session) => {
            let skills = session.list_skills();
            Json(serde_json::json!({ "skills": skills })).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": { "message": "会话不存在" } })),
        ).into_response(),
    }
}

/// POST /sessions/:id/compact — 手动触发上下文压缩
async fn compact_session(
    State(store): State<SessionStore>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match store.get_mut(&id) {
        Some(mut session) => {
            session.compact_context();
            Json(serde_json::json!({ "status": "ok", "message": "上下文已压缩" })).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": { "message": "会话不存在" } })),
        ).into_response(),
    }
}
```

注意：需要在 `SessionStore` 中添加 `get_mut` 方法。

- [ ] **Step 3: 在 SessionStore 中添加 get_mut 方法**

```rust
pub fn get_mut(&self, id: &str) -> Option<dashmap::mapref::one::RefMut<String, Session>> {
    self.sessions.get_mut(id)
}
```

- [ ] **Step 4: 编译验证**

Run: `cargo build -p rust-agent-server`

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/routes.rs crates/server/src/session.rs
git commit -m "feat(server): 添加 /skills 和 /compact 端点"
```

---

### Task 2: CLI 添加 API 函数和 slash 命令处理

**Files:**
- Modify: `cli/src/api.ts`
- Modify: `cli/src/app.tsx`
- Modify: `cli/src/chat.tsx`

- [ ] **Step 1: 在 api.ts 中添加 getSkills 和 triggerCompact**

```ts
// 获取技能列表
export async function getSkills(): Promise<any[]> {
  const { baseUrl, sessionId } = getConfig();
  const res = await fetch(`${baseUrl}/sessions/${sessionId}/skills`);
  if (!res.ok) throw new Error(`获取技能失败: ${res.status}`);
  const data = await res.json();
  return data.skills;
}

// 触发上下文压缩
export async function triggerCompact(): Promise<string> {
  const { baseUrl, sessionId } = getConfig();
  const res = await fetch(`${baseUrl}/sessions/${sessionId}/compact`, { method: 'POST' });
  if (!res.ok) throw new Error(`压缩失败: ${res.status}`);
  const data = await res.json();
  return data.message;
}
```

- [ ] **Step 2: 在 app.tsx 中添加 handleCommand**

在 `handleSubmit` 之前拦截 `/` 开头的输入：

```tsx
const handleCommand = useCallback(async (input: string) => {
  const cmd = input.trim().toLowerCase();
  const addSystemMsg = (text: string) => {
    setMessages(prev => [...prev, { role: 'system', content: text }]);
  };

  switch (cmd) {
    case '/help':
      addSystemMsg(
        '可用命令:\n  /help    - 显示帮助\n  /clear   - 清空对话\n  /skills  - 列出可用技能\n  /compact - 压缩上下文'
      );
      break;
    case '/clear':
      setMessages([]);
      setCurrentReply('');
      break;
    case '/skills':
      try {
        const skills = await getSkills();
        if (skills.length === 0) {
          addSystemMsg('没有可用技能');
        } else {
          const list = skills.map((s: any) => `  ${s.name} - ${s.description || '无描述'}`).join('\n');
          addSystemMsg(`可用技能 (${skills.length}):\n${list}`);
        }
      } catch (err) {
        addSystemMsg(`获取技能失败: ${err}`);
      }
      break;
    case '/compact':
      try {
        const msg = await triggerCompact();
        addSystemMsg(msg);
      } catch (err) {
        addSystemMsg(`压缩失败: ${err}`);
      }
      break;
    default:
      addSystemMsg(`未知命令: ${input}\n输入 /help 查看可用命令`);
  }
}, [sessionId]);
```

修改 `handleSubmit` 入口处添加命令判断：

```tsx
const handleSubmit = useCallback(async (input: string) => {
  if (!input.trim() || isLoading || !sessionId) return;
  // 拦截 slash 命令
  if (input.trim().startsWith('/')) {
    handleCommand(input);
    return;
  }
  // ... 原有发送消息逻辑
}, [sessionId, isLoading, handleCommand]);
```

- [ ] **Step 3: 在 chat.tsx 中添加 system 消息渲染**

在 `renderMessage` 函数中添加：

```tsx
case 'system':
  return (
    <Box key={`msg-${index}`}>
      <Text dimColor>{msg.content}</Text>
    </Box>
  );
```

- [ ] **Step 4: TypeScript 类型检查**

Run: `cd cli && npx tsc --noEmit`

- [ ] **Step 5: Commit**

```bash
git add cli/src/api.ts cli/src/app.tsx cli/src/chat.tsx
git commit -m "feat(cli): 添加 /help /clear /skills /compact slash 命令"
```

---

### Task 3: 集成测试

- [ ] **Step 1: 编译 server 并运行测试**

Run: `cargo test`
Run: `cargo build -p rust-agent-server`

- [ ] **Step 2: 手动测试 CLI**

Run: `cd cli && npm start`

验证：
- 输入 `/help` → 显示命令列表
- 输入 `/clear` → 清空对话
- 输入 `/skills` → 显示技能列表
- 输入 `/compact` → 显示压缩成功
- 输入 `/unknown` → 显示未知命令提示
- 输入普通消息 → 正常发送（不受影响）
