export interface ServerEvent {
  event: string;
  data: Record<string, any>;
}

export interface Session {
  id: string;
  model: string;
  created_at: string;
}

export class ApiClient {
  private baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl;
  }

  async createSession(): Promise<Session> {
    const res = await fetch(`${this.baseUrl}/sessions`, { method: "POST" });
    if (!res.ok) {
      const data = await res.json().catch(() => ({}));
      throw new Error(
        `创建会话失败 (${res.status}): ${data?.error?.message || res.statusText}`
      );
    }
    return res.json();
  }

  async *sendMessage(
    sessionId: string,
    content: string,
    signal?: AbortSignal
  ): AsyncGenerator<ServerEvent, void> {
    const res = await fetch(`${this.baseUrl}/sessions/${sessionId}/messages`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ content }),
      signal,
    });
    if (!res.ok || !res.body) {
      const data = await res.json().catch(() => ({}));
      throw new Error(
        `请求失败 (${res.status}): ${data?.error?.message || res.statusText}`
      );
    }
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let currentEvent = "";
    while (true) {
      const { done, value } = await reader.read();
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

  async clearSession(sessionId: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/sessions/${sessionId}/clear`, {
      method: "POST",
    });
    if (!res.ok) {
      throw new Error(`清空会话失败: ${res.status}`);
    }
  }

  async deleteSession(sessionId: string): Promise<void> {
    const res = await fetch(`${this.baseUrl}/sessions/${sessionId}`, {
      method: "DELETE",
    });
    if (!res.ok) {
      throw new Error(`删除会话失败: ${res.status}`);
    }
  }
}
