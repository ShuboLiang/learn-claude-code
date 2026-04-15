# Node.js CLI 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 用 Ink (React for CLI) 替代 Rust CLI，通过 HTTP 调用现有 Rust server

**Architecture:** Node.js CLI 自动 spawn rust-agent-server 子进程，通过 fetch + SSE 通信

**Spec:** `docs/superpowers/specs/2026-04-15-nodejs-cli-design.md`

---

### Task 1: 初始化 Node.js 项目

**Files:**
- Create: `cli/package.json`
- Create: `cli/tsconfig.json`
- Create: `cli/src/index.tsx`

- [ ] **Step 1: 创建 package.json**

```json
{
  "name": "rust-agent-cli",
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "start": "tsx src/index.tsx"
  },
  "dependencies": {
    "ink": "^5.1.0",
    "react": "^18.3.1",
    "ink-text-input": "^6.0.0",
    "node-fetch": "^3.3.2"
  },
  "devDependencies": {
    "tsx": "^4.19.0",
    "@types/react": "^18.3.0",
    "typescript": "^5.6.0"
  }
}
```

- [ ] **Step 2: 创建 tsconfig.json**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true
  },
  "include": ["src/**/*"]
}
```

- [ ] **Step 3: 创建 src/index.tsx（入口，spawn server + 启动 Ink）**

```tsx
#!/usr/bin/env npx tsx
import React from 'react';
import { render } from 'ink';
import { spawn } from 'child_process';
import net from 'net';
import App from './app';

const SERVER_BINARY = process.platform === 'win32'
  ? '../target/debug/rust-agent-server.exe'
  : '../target/debug/rust-agent-server';

function findFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, '127.0.0.1', () => {
      const port = (server.address() as net.AddressInfo).port;
      server.close(() => resolve(port));
    });
    server.on('error', reject);
  });
}

async function startServer(): Promise<{ port: number; process: import('child_process').ChildProcess }> {
  const port = await findFreePort();
  const child = spawn(SERVER_BINARY, ['--port', String(port)], {
    stdio: ['inherit', 'pipe', 'pipe'],
  });

  // 等待 server 就绪（最多 10 秒）
  for (let i = 0; i < 100; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/sessions`, { method: 'GET' });
      if (res.ok) {
        console.error(`[server] 运行在端口 ${port}`);
        return { port, process: child };
      }
    } catch {
      // server 还没准备好
    }
    await new Promise(r => setTimeout(r, 100));
  }

  throw new Error('server 启动超时');
}

async function main() {
  const { port, process: serverProcess } = await startServer();

  const { exit } = render(<App serverUrl={`http://127.0.0.1:${port}`} />);

  // 退出时清理子进程
  const cleanup = () => {
    serverProcess.kill();
    process.exit(0);
  };
  process.on('SIGINT', cleanup);
  process.on('SIGTERM', cleanup);

  await new Promise(r => setTimeout(r, 100));
}

main().catch(err => {
  console.error('启动失败:', err.message);
  process.exit(1);
});
```

- [ ] **Step 4: 安装依赖**

Run: `cd cli && npm install`

- [ ] **Step 5: Commit**

```bash
git add cli/
git commit -m "feat: 初始化 Node.js CLI 项目（Ink + React）"
```

---

### Task 2: 实现 API 模块

**Files:**
- Create: `cli/src/api.ts`

- [ ] **Step 1: 创建 src/api.ts**

```ts
export interface ServerConfig {
  baseUrl: string;
  sessionId: string;
}

let config: ServerConfig | null = null;

export function getConfig(): ServerConfig {
  if (!config) throw new Error('API 未初始化');
  return config;
}

export function init(baseUrl: string, sessionId: string) {
  config = { baseUrl, sessionId };
}

export async function createSession(): Promise<string> {
  const res = await fetch(`${getConfig().baseUrl}/sessions`, { method: 'POST' });
  const data = await res.json();
  return data.id;
}

export async function* sendMessage(content: string): AsyncGenerator<string, ServerEvent> {
  const { baseUrl, sessionId } = getConfig();
  const res = await fetch(`${baseUrl}/sessions/${sessionId}/messages`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content }),
  });
  if (!res.ok || !res.body) throw new Error(`请求失败: ${res.status}`);
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  while (true) {
    const { done, value } = await reader.read();
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() || '';
    for (const line of lines) {
      if (line.startsWith('data: ')) {
        const data = line.slice(6);
        if (data === '[DONE]') return;
        try { yield JSON.parse(data); } catch {}
      }
    }
    if (done) break;
  }
}

export interface ServerEvent { event: string; data: Record<string, any>; }
```

- [ ] **Step 2: Commit**

```bash
git add cli/src/api.ts
git commit -m "feat(cli): 实现 API 模块（SSE 流式通信）"
```

---

### Task 3: 实现主应用组件

**Files:**
- Create: `cli/src/app.tsx`

- [ ] **Step 1: 创建 src/app.tsx**

```tsx
import React, { useState, useCallback, useEffect } from 'react';
import { Box, Text, useApp, Static } from 'ink';
import TextInput from 'ink-text-input';
import { sendMessage, init, createSession } from './api';
import Chat from './chat';
import Input from './input';

export default function App({ serverUrl }: { serverUrl: string }) {
  const { exit } = useApp();
  const [sessionId, setSessionId] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [messages, setMessages] = useState<Array<{ role: string; content: string }>>([]);
  const [currentReply, setCurrentReply] = useState('');

  useEffect(() => {
    (async () => { init(serverUrl, ''); const id = await createSession(); setSessionId(id); })();
  }, [serverUrl]);

  const handleSubmit = useCallback(async (input: string) => {
    if (!input.trim() || isLoading || !sessionId) return;
    setError(null);
    setMessages(prev => [...prev, { role: 'user', content: input }]);
    setIsLoading(true);
    setCurrentReply('');
    try {
      for await (const event of sendMessage(input)) {
        switch (event.event) {
          case 'text_delta': setCurrentReply(prev => prev + event.data.content); break;
          case 'tool_call':
            if (currentReply) { setMessages(prev => [...prev, { role: 'assistant', content: currentReply }]); setCurrentReply(''); }
            setMessages(prev => [...prev, { role: 'tool_call', content: event.data }]); break;
          case 'tool_result': setMessages(prev => [...prev, { role: 'tool_result', content: event.data.output }]); break;
          case 'done':
            if (currentReply) { setMessages(prev => [...prev, { role: 'assistant', content: currentReply }]); setCurrentReply(''); }
            break;
        }
      }
    } catch (err) { setError(String(err)); }
    finally { setIsLoading(false); }
  }, [sessionId, isLoading, currentReply]);

  const handleQuit = useCallback((input: string) => {
    if (matches(input, 'q', 'quit', 'exit')) exit();
  }, [exit]);

  return (
    <Box flexDirection="column" height="100%">
      <Chat messages={messages} currentReply={currentReply} isLoading={isLoading} />
      <Input onSubmit={handleSubmit} onQuit={handleQuit} isLoading={isLoading} />
      {error && <Box><Text color="red">Error: {error}</Text></Box>}
    </Box>
  );
}
```

- [ ] **Step 2: Commit**

```bash
git add cli/src/app.tsx
git commit -m "feat(cli): 实现主应用组件"
```

---

### Task 4: 实现聊天记录和输入框组件

**Files:**
- Create: `cli/src/chat.tsx`
- Create: `cli/src/input.tsx`

- [ ] **Step 1: 创建 chat.tsx**

```tsx
import React, { useRef, useEffect } from 'react';
import { Box, Text, Static } from 'ink';

interface Message { role: string; content: string; }
interface ChatProps { messages: Message[]; currentReply: string; isLoading: boolean; }

export default function Chat({ messages, currentReply, isLoading }: ChatProps) {
  const scrollRef = useRef(0);
  useEffect(() => { scrollRef.current = messages.length + (currentReply ? 1 : 0) + (isLoading ? 1 : 0); }, [messages.length, currentReply, isLoading]);

  const allMessages = [...messages];
  if (currentReply) allMessages.push({ role: 'assistant', content: currentReply });
  if (isLoading && !currentReply) allMessages.push({ role: 'assistant', content: '...' });

  return (
    <Box flexDirection="column" flexGrow={1} overflowY="hidden">
      <Static items={allMessages.map((msg, i) => {
        switch (msg.role) {
          case 'user': return <Box key={i}><Text color="cyan" bold>你: {msg.content}</Text></Box>;
          case 'assistant': return <Box key={i}><Text wrap="wrap">{msg.content}</Text></Box>;
          case 'tool_call': {
            const d = typeof msg.content === 'string' ? msg.content : JSON.stringify(msg.content);
            return <Box key={i}><Text color="yellow">┌─ {d}</Text></Box>;
          }
          case 'tool_result': return <Box key={i}><Text dimColor="gray">│  {msg.content}</Text></Box>;
          default: return null;
        }
      })} />
    </Box>
  );
}
```

- [ ] **Step 2: 创建 input.tsx**

```tsx
import React, { useState } from 'react';
import { Box, Text } from 'ink';
import TextInput from 'ink-text-input';

interface InputProps { onSubmit: (text: string) => void; onQuit: (text: string) => void; isLoading: boolean; }

export default function Input({ onSubmit, onQuit, isLoading }: InputProps) {
  const [value, setValue] = useState('');
  const handleSubmit = (text: string) => {
    if (!text.trim()) return;
    if (['q','quit','exit'].includes(text.trim().toLowerCase())) { onQuit(text); return; }
    setValue('');
    onSubmit(text);
  };
  return (
    <Box borderStyle="gray" paddingLeft={1}>
      <TextInput value={value} onChange={setValue} onSubmit={handleSubmit}
        placeholder={isLoading ? '(等待响应中...)' : '输入消息，Enter 提交...'} />
    </Box>
  );
}
```

- [ ] **Step 3: Commit**

```bash
git add cli/src/chat.tsx cli/src/input.tsx
git commit -m "feat(cli): 实现聊天记录和输入框组件"
```

---

### Task 5: 构建测试

- [ ] **Step 1:** `cargo build -p rust-agent-server` 确保 server 已编译

- [ ] **Step 2:** `cd cli && npm start` 测试完整流程
