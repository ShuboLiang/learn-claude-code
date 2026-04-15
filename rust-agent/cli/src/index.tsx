#!/usr/bin/env npx tsx
import React from 'react';
import { render } from 'ink';
import { spawn } from 'child_process';
import net from 'net';
import App from './app';

const SERVER_BINARY = process.platform === 'win32'
  ? '../target/debug/rust-agent-server.exe'
  : '../target/debug/rust-agent-server';

// 查找空闲端口
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

// 启动 server 子进程并等待就绪
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

  const instance = render(<App serverUrl={`http://127.0.0.1:${port}`} />);

  // 退出时清理子进程
  const cleanup = () => {
    serverProcess.kill();
    instance.unmount();
    process.exit(0);
  };
  process.on('SIGINT', cleanup);
  process.on('SIGTERM', cleanup);

  // 等待 Ink 退出
  await instance.waitUntilExit();
}

main().catch(err => {
  console.error('启动失败:', err.message);
  process.exit(1);
});
