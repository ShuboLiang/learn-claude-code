#!/usr/bin/env npx tsx
import React from "react";
import { render } from "ink";
import { spawn } from "child_process";
import net from "net";
import App from "./app";

import { existsSync } from "fs";
import { dirname, resolve } from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
// 优先从环境变量获取项目根目录，支持全局安装场景
const projectRoot = process.env.RUST_AGENT_ROOT || resolve(__dirname, "../..");

function resolveServerBinary(): string {
  // 优先使用 release 版本，回退到 debug
  const candidates = process.platform === "win32"
    ? [
        resolve(projectRoot, "target/release/rust-agent-server.exe"),
        resolve(projectRoot, "target/debug/rust-agent-server.exe"),
      ]
    : [
        resolve(projectRoot, "target/release/rust-agent-server"),
        resolve(projectRoot, "target/debug/rust-agent-server"),
      ];
  for (const p of candidates) {
    if (existsSync(p)) return p;
  }
  // 都不存在时返回第一个，让后续 spawn 报错提示用户
  return candidates[0];
}

const SERVER_BINARY = resolveServerBinary();

// 查找空闲端口
function findFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, "127.0.0.1", () => {
      const port = (server.address() as net.AddressInfo).port;
      server.close(() => resolve(port));
    });
    server.on("error", reject);
  });
}

// 启动 server 子进程并等待就绪
async function startServer(): Promise<{
  port: number;
  process: import("child_process").ChildProcess;
}> {
  const port = await findFreePort();
  const child = spawn(SERVER_BINARY, ["--port", String(port)], {
    stdio: ["inherit", "pipe", "pipe"],
  });

  // 等待 server 就绪（最多 10 秒）
  for (let i = 0; i < 100; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/`, { method: "GET" });
      if (res.ok) {
        // console.error(`[server] 运行在端口 ${port}`);
        return { port, process: child };
      }
    } catch {
      // server 还没准备好
    }
    await new Promise((r) => setTimeout(r, 100));
  }

  throw new Error("server 启动超时");
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
  process.on("SIGINT", cleanup);
  process.on("SIGTERM", cleanup);

  // 等待 Ink 退出
  await instance.waitUntilExit();

  // Ink 退出后（如用户输入 /exit），清理 server 子进程
  cleanup();
}

main().catch((err) => {
  console.error("启动失败:", err.message);
  process.exit(1);
});
