export interface ServerConfig {
  baseUrl: string;
  sessionId: string;
}

let config: ServerConfig | null = null;

export function getConfig(): ServerConfig {
  if (!config) throw new Error("API 未初始化");
  return config;
}

export function init(baseUrl: string, sessionId: string) {
  config = { baseUrl, sessionId };
}

export async function createSession(): Promise<{ id: string; model: string }> {
  const res = await fetch(`${getConfig().baseUrl}/sessions`, {
    method: "POST",
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(
      `创建会话失败 (${res.status}): ${data?.error?.message || res.statusText}`,
    );
  }
  const data = await res.json();
  if (!data.id) throw new Error("创建会话失败: 服务器未返回会话 ID");
  // 更新 config 中的 sessionId，供后续 sendMessage 使用
  config!.sessionId = data.id;
  return { id: data.id, model: data.model || "unknown" };
}

// SSE 事件类型
export interface ServerEvent {
  event: string;
  data: Record<string, any>;
}

// 流式发送消息，逐行解析 SSE 事件
export async function* sendMessage(
  content: string,
  signal?: AbortSignal,
): AsyncGenerator<ServerEvent, void> {
  const { baseUrl, sessionId } = getConfig();
  const res = await fetch(`${baseUrl}/sessions/${sessionId}/messages`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ content }),
    signal,
  });
  if (!res.ok || !res.body) {
    const data = await res.json().catch(() => ({}));
    throw new Error(
      `请求失败 (${res.status}): ${data?.error?.message || res.statusText}`,
    );
  }
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let currentEvent = "";
  while (true) {
    let done: boolean | undefined;
    let value: Uint8Array | undefined;
    try {
      ({ done, value } = await reader.read());
    } catch (err) {
      // Node.js fetch 在服务器意外关闭连接时抛出 TypeError: terminated
      // 当作流正常结束处理，避免显示错误
      if (
        err instanceof TypeError &&
        (err.message === "terminated" || err.message.includes("terminated"))
      ) {
        break;
      }
      throw err;
    }
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() || "";
    for (const line of lines) {
      // 兼容 "event: type" 和 "event:type" 两种格式
      if (line.startsWith("event:")) {
        currentEvent = line.slice(line.charAt(6) === " " ? 7 : 6);
      } else if (line.startsWith("data:")) {
        const data = line.slice(line.charAt(5) === " " ? 6 : 5);
        if (data === "[DONE]") return;
        try {
          yield { event: currentEvent, data: JSON.parse(data) };
        } catch {}
        currentEvent = "";
      }
    }
    if (done) break;
  }
}

export async function clearSession(): Promise<void> {
  const { baseUrl, sessionId } = getConfig();
  const res = await fetch(`${baseUrl}/sessions/${sessionId}/clear`, {
    method: "POST",
  });
  if (!res.ok) {
    throw new Error(`清空会话失败: ${res.status}`);
  }
}

// ── Subagent / Bot API ──

export interface BotInfo {
  name: string;
  nickname: string;
  role: string;
  description: string;
}

export async function fetchBots(): Promise<BotInfo[]> {
  const { baseUrl } = getConfig();
  const res = await fetch(`${baseUrl}/bots`);
  if (!res.ok) throw new Error(`获取 Bot 列表失败: ${res.status}`);
  const data = await res.json();
  return data.bots || [];
}

/** 向指定 Bot 委派任务，返回 SSE 流 */
export async function* sendBotTask(
  botName: string,
  content: string,
  signal?: AbortSignal,
): AsyncGenerator<ServerEvent, void> {
  const { baseUrl } = getConfig();
  const res = await fetch(
    `${baseUrl}/bots/${encodeURIComponent(botName)}/task`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content }),
      signal,
    },
  );
  if (!res.ok || !res.body) {
    const data = await res.json().catch(() => ({}));
    throw new Error(
      `Bot 请求失败 (${res.status}): ${data?.error?.message || res.statusText}`,
    );
  }
  // 复用与 sendMessage 相同的 SSE 解析逻辑
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let currentEvent = "";
  while (true) {
    let done: boolean | undefined;
    let value: Uint8Array | undefined;
    try {
      ({ done, value } = await reader.read());
    } catch (err) {
      if (
        err instanceof TypeError &&
        (err.message === "terminated" || err.message.includes("terminated"))
      ) {
        break;
      }
      throw err;
    }
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() || "";
    for (const line of lines) {
      if (line.startsWith("event:")) {
        currentEvent = line.slice(line.charAt(6) === " " ? 7 : 6);
      } else if (line.startsWith("data:")) {
        const data = line.slice(line.charAt(5) === " " ? 6 : 5);
        if (data === "[DONE]") return;
        try {
          yield { event: currentEvent, data: JSON.parse(data) };
        } catch {}
        currentEvent = "";
      }
    }
    if (done) break;
  }
}
