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

// SSE 事件类型
export interface ServerEvent {
  event: string;
  data: Record<string, any>;
}

// 流式发送消息，逐行解析 SSE 事件
export async function* sendMessage(content: string): AsyncGenerator<ServerEvent, void> {
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
