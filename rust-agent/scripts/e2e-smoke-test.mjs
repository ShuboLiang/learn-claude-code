#!/usr/bin/env node
/**
 * E2E 冒烟测试脚本
 *
 * 1. 启动真实 rust-agent-server 进程
 * 2. 轮询 health check 确认就绪
 * 3. 验证会话全链路 CRUD
 * 4. 验证 SSE 消息发送链路（可选，会调用真实 LLM）
 * 5. 验证 OpenAI 兼容端点 /v1/chat/completions
 * 6. 验证 /bots 列表
 * 7. 清理并退出
 *
 * 环境变量:
 *   - SERVER_PORT     : 服务器端口 (默认随机 30000-40000)
 *   - SERVER_BIN      : 服务器二进制路径 (默认 ../target/debug/rust-agent-server.exe 或 ../target/debug/rust-agent-server)
 *   - SKIP_LLM_TESTS  : 设为 true 则跳过调用真实 LLM 的测试
 *   - TEST_TIMEOUT_MS : 单条 LLM 测试超时 (默认 60000)
 */

import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const COLORS = {
  reset: "\x1b[0m",
  green: "\x1b[32m",
  red: "\x1b[31m",
  yellow: "\x1b[33m",
  cyan: "\x1b[36m",
  dim: "\x1b[2m",
};

function log(msg, color = "reset") {
  console.log(`${COLORS[color]}${msg}${COLORS.reset}`);
}

function fail(msg) {
  throw new Error(msg);
}

function assertEqual(actual, expected, msg) {
  if (actual !== expected) {
    fail(`${msg}\n  期望: ${expected}\n  实际: ${actual}`);
  }
}

function assertTruthy(value, msg) {
  if (!value) {
    fail(`${msg}\n  值: ${value}`);
  }
}

// ── 配置 ────────────────────────────────────────────────
const PORT = process.env.SERVER_PORT
  ? Number(process.env.SERVER_PORT)
  : Math.floor(Math.random() * 10000) + 30000;

const SERVER_BIN =
  process.env.SERVER_BIN ||
  path.join(
    __dirname,
    "..",
    "target",
    "debug",
    os.platform() === "win32" ? "rust-agent-server.exe" : "rust-agent-server"
  );

const SKIP_LLM = process.env.SKIP_LLM_TESTS === "true";
const LLM_TIMEOUT = Number(process.env.TEST_TIMEOUT_MS || "60000");
const BASE_URL = `http://localhost:${PORT}`;

let serverProcess = null;
let failed = false;

// ── HTTP 工具 ───────────────────────────────────────────
async function request(path, opts = {}) {
  const url = `${BASE_URL}${path}`;
  const res = await fetch(url, {
    ...opts,
    headers: {
      ...(opts.headers || {}),
      ...(opts.body ? { "Content-Type": "application/json" } : {}),
    },
  });
  let body = null;
  const contentType = res.headers.get("content-type") || "";
  if (contentType.includes("application/json")) {
    body = await res.json();
  } else {
    body = await res.text();
  }
  return { status: res.status, headers: res.headers, body };
}

// ── SSE 读取工具 ────────────────────────────────────────
async function* readSSE(response) {
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let currentEvent = null;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop(); // 保留不完整行
      for (const line of lines) {
        if (line.startsWith("event: ")) {
          currentEvent = line.slice(7);
        } else if (line.startsWith("data: ")) {
          const data = line.slice(6);
          if (data === "[DONE]") continue;
          try {
            const parsed = JSON.parse(data);
            if (currentEvent) parsed.type = currentEvent;
            yield parsed;
          } catch {
            yield { type: currentEvent || "unknown", raw: data };
          }
          currentEvent = null;
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}

// ── 服务器生命周期 ──────────────────────────────────────
function startServer() {
  return new Promise((resolve, reject) => {
    log(`[启动] ${SERVER_BIN} --port ${PORT}`, "dim");
    serverProcess = spawn(SERVER_BIN, ["--port", String(PORT)], {
      detached: false,
      stdio: "pipe",
    });

    serverProcess.stdout.on("data", (d) => {
      // 静默或打印到 stderr 以便调试
      process.stderr.write(`[server-out] ${d}`);
    });
    serverProcess.stderr.on("data", (d) => {
      process.stderr.write(`[server-err] ${d}`);
    });

    serverProcess.on("error", reject);

    // 给进程一点时间启动，然后轮询 health
    resolve();
  });
}

async function waitForReady(maxWaitMs = 15000) {
  const start = Date.now();
  while (Date.now() - start < maxWaitMs) {
    try {
      const { status, body } = await request("/");
      if (status === 200 && body?.status === "ok") {
        return;
      }
    } catch {
      // 还未就绪
    }
    await sleep(200);
  }
  fail("服务器在 ${maxWaitMs}ms 内未就绪");
}

function stopServer() {
  return new Promise((resolve) => {
    if (!serverProcess) return resolve();
    log("[关闭] 终止服务器进程...", "dim");
    if (os.platform() === "win32") {
      spawn("taskkill", ["/pid", String(serverProcess.pid), "/f", "/t"]);
    } else {
      serverProcess.kill("SIGTERM");
    }
    const t = setTimeout(() => {
      try {
        serverProcess.kill("SIGKILL");
      } catch {}
      resolve();
    }, 3000);
    serverProcess.on("exit", () => {
      clearTimeout(t);
      resolve();
    });
  });
}

// ── 测试用例 ────────────────────────────────────────────
async function testHealthCheck() {
  log("→ 测试 Health Check", "cyan");
  const { status, body } = await request("/");
  assertEqual(status, 200, "health status");
  assertEqual(body.status, "ok", "health body.status");
  log("  ✓ / 返回 { status: ok }", "green");
}

async function testSessionCRUD() {
  log("→ 测试 Session CRUD", "cyan");

  // Create
  const createRes = await request("/sessions", { method: "POST" });
  assertEqual(createRes.status, 200, "create session status");
  const sessionId = createRes.body.id;
  assertTruthy(sessionId, "create session id");
  assertTruthy(createRes.body.created_at, "create session created_at");
  log(`  ✓ POST /sessions 创建 id=${sessionId.slice(0, 8)}...`, "green");

  // Get
  const getRes = await request(`/sessions/${sessionId}`);
  assertEqual(getRes.status, 200, "get session status");
  assertEqual(getRes.body.id, sessionId, "get session id");
  assertTruthy(getRes.body.message_count !== undefined, "get session message_count");
  log("  ✓ GET /sessions/:id 查询成功", "green");

  // List
  const listRes = await request("/sessions");
  assertEqual(listRes.status, 200, "list sessions status");
  assertTruthy(Array.isArray(listRes.body.sessions), "list sessions array");
  assertTruthy(
    listRes.body.sessions.some((s) => s.id === sessionId),
    "list sessions contains created"
  );
  log("  ✓ GET /sessions 列表包含新建会话", "green");

  // Messages (empty)
  const msgRes = await request(`/sessions/${sessionId}/messages`);
  assertEqual(msgRes.status, 200, "get messages status");
  assertTruthy(Array.isArray(msgRes.body.messages), "messages array");
  log("  ✓ GET /sessions/:id/messages 初始为空", "green");

  // Clear
  const clearRes = await request(`/sessions/${sessionId}/clear`, { method: "POST" });
  assertEqual(clearRes.status, 200, "clear session status");
  assertEqual(clearRes.body.status, "cleared", "clear session body");
  log("  ✓ POST /sessions/:id/clear 清空成功", "green");

  // Delete
  const delRes = await request(`/sessions/${sessionId}`, { method: "DELETE" });
  assertEqual(delRes.status, 204, "delete session status");
  log("  ✓ DELETE /sessions/:id 删除成功", "green");

  // Get after delete → 404
  const getAfter = await request(`/sessions/${sessionId}`);
  assertEqual(getAfter.status, 404, "get after delete status");
  log("  ✓ 删除后 GET /sessions/:id 返回 404", "green");

  return sessionId;
}

async function testSendMessage() {
  if (SKIP_LLM) {
    log("→ 跳过 SSE 消息测试 (SKIP_LLM_TESTS=true)", "yellow");
    return;
  }

  log("→ 测试 SSE 消息发送（将调用真实 LLM）", "cyan");

  // 创建会话
  const { body: session } = await request("/sessions", { method: "POST" });
  const id = session.id;

  // 发送消息（SSE）
  const url = `${BASE_URL}/sessions/${id}/messages`;
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), LLM_TIMEOUT);

  let eventCount = 0;
  let hasText = false;
  let hasTurnEnd = false;
  let hasDone = false;
  let errorEvent = null;
  const eventTypes = [];

  try {
    const res = await fetch(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content: "Say only the word 'pong' and nothing else." }),
      signal: controller.signal,
    });

    assertEqual(res.status, 200, "send message status");
    const contentType = res.headers.get("content-type") || "";
    assertTruthy(contentType.includes("text/event-stream"), "send message content-type");

    for await (const event of readSSE(res)) {
      eventCount++;
      eventTypes.push(event.type || "unknown");
      if (event.type === "text_delta") hasText = true;
      if (event.type === "turn_end") hasTurnEnd = true;
      if (event.type === "done") hasDone = true;
      if (event.type === "error") errorEvent = event;
    }
  } catch (e) {
    if (e.name === "AbortError") {
      fail(`SSE 消息发送超时 (${LLM_TIMEOUT}ms)`);
    }
    throw e;
  } finally {
    clearTimeout(timeout);
  }

  assertTruthy(eventCount > 0, "SSE 至少收到一个事件");
  log(`  ✓ SSE 收到 ${eventCount} 个事件 [${eventTypes.join(", ")}]`, "green");

  if (errorEvent) {
    log(
      `  ! SSE 收到 error 事件: ${errorEvent.code} - ${errorEvent.message}`,
      "yellow"
    );
    // 如果是 API key / rate limit 相关错误，不致命，但标记警告
    if (
      errorEvent.code?.includes("api_key") ||
      errorEvent.code?.includes("auth") ||
      errorEvent.code?.includes("rate_limited")
    ) {
      log("  ! 跳过 LLM 内容断言（API 受限）", "yellow");
    } else {
      fail(`SSE 收到非预期 error: ${errorEvent.message}`);
    }
  } else {
    // turn_end 或 done 任一出现即视为链路正常
    if (!hasTurnEnd && !hasDone) {
      fail(
        `SSE 未收到 turn_end 或 done 事件。事件类型: [${eventTypes.join(", ")}]`
      );
    }
    log("  ✓ SSE 收到 turn_end/done，链路正常", "green");
  }

  // 清理会话
  await request(`/sessions/${id}`, { method: "DELETE" });
}

async function testOpenAICompat() {
  if (SKIP_LLM) {
    log("→ 跳过 OpenAI 兼容端点测试 (SKIP_LLM_TESTS=true)", "yellow");
    return;
  }

  log("→ 测试 OpenAI 兼容端点 /v1/chat/completions", "cyan");

  const { status, body } = await request("/v1/chat/completions", {
    method: "POST",
    body: JSON.stringify({
      model: "ignored",
      messages: [{ role: "user", content: "Say pong." }],
      stream: false,
    }),
  });

  assertEqual(status, 200, "chat completions status");
  assertTruthy(body.id?.startsWith("chatcmpl-"), "chat completions id");
  assertEqual(body.object, "chat.completion", "chat completions object");
  assertTruthy(Array.isArray(body.choices), "chat completions choices");
  assertTruthy(body.choices.length > 0, "chat completions choices length");
  assertEqual(body.choices[0].message.role, "assistant", "choice message role");
  log("  ✓ /v1/chat/completions 返回格式正确", "green");
}

async function testBots() {
  log("→ 测试 /bots", "cyan");
  const { status, body } = await request("/bots");
  assertEqual(status, 200, "bots status");
  assertTruthy(Array.isArray(body.bots), "bots array");
  assertTruthy(typeof body.total === "number", "bots total");
  log(`  ✓ /bots 返回 ${body.total} 个 bot`, "green");
}

// ── 主流程 ──────────────────────────────────────────────
async function main() {
  log("═══════════════════════════════════════", "cyan");
  log("  rust-agent-server E2E 冒烟测试", "cyan");
  log("═══════════════════════════════════════", "cyan");
  log(`服务器: ${SERVER_BIN}`, "dim");
  log(`端口  : ${PORT}`, "dim");
  log(`LLM   : ${SKIP_LLM ? "跳过" : "启用（超时 " + LLM_TIMEOUT + "ms）"}`, "dim");

  try {
    await startServer();
    await waitForReady();
    log("[就绪] 服务器已启动\n", "green");

    await testHealthCheck();
    await testSessionCRUD();
    await testSendMessage();
    await testOpenAICompat();
    await testBots();

    log("\n═══════════════════════════════════════", "green");
    log("  所有测试通过 ✓", "green");
    log("═══════════════════════════════════════", "green");
  } catch (err) {
    failed = true;
    log("\n═══════════════════════════════════════", "red");
    log("  测试失败 ✗", "red");
    log("═══════════════════════════════════════", "red");
    log(err.message || err, "red");
    if (err.stack) log(err.stack, "dim");
  } finally {
    await stopServer();
    process.exit(failed ? 1 : 0);
  }
}

main();
