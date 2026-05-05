import type { ApiMessage, SessionSummary } from '@/types/wire'

export class ApiError extends Error {
  status: number
  code: string

  constructor(status: number, code: string, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
  }
}

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, init)
  if (res.status === 204) return undefined as T
  if (!res.ok) {
    let code = 'UNKNOWN'
    let message = `HTTP ${res.status}`
    try {
      const body = await res.json()
      code = body.code ?? code
      message = body.message ?? message
    } catch {
      /* body not JSON */
    }
    throw new ApiError(res.status, code, message)
  }
  return res.json()
}

export function listSessions(): Promise<SessionSummary[]> {
  return request<{ sessions: SessionSummary[] }>('/sessions').then((r) => r.sessions)
}

export function createSession(workingDir?: string): Promise<{ id: string; working_dir: string }> {
  return request('/sessions', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ working_dir: workingDir || null }),
  })
}

export function getSession(id: string): Promise<SessionSummary> {
  return request(`/sessions/${encodeURIComponent(id)}`)
}

export function deleteSession(id: string): Promise<void> {
  return request(`/sessions/${encodeURIComponent(id)}`, { method: 'DELETE' })
}

export function getMessages(id: string): Promise<ApiMessage[]> {
  return request<{ messages: ApiMessage[] }>(
    `/sessions/${encodeURIComponent(id)}/messages`,
  ).then((r) => r.messages)
}

export function clearSession(id: string): Promise<void> {
  return request(`/sessions/${encodeURIComponent(id)}/clear`, { method: 'POST' })
}

export interface BotInfo {
  name: string
  nickname: string
  role: string
  description: string
  skills: string[]
}

export function listBots(): Promise<BotInfo[]> {
  return request<{ bots: BotInfo[] }>('/bots').then((r) => r.bots)
}
