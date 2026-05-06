import type { ApiMessage, ConfigResponse, SessionSummary } from '@/types/wire'

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

async function request<T>(url: string, init?: RequestInit & { signal?: AbortSignal }): Promise<T> {
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

export function listSessions(signal?: AbortSignal): Promise<SessionSummary[]> {
  return request<{ sessions: SessionSummary[] }>('/sessions', { signal }).then((r) => r.sessions)
}

export function getConfig(signal?: AbortSignal): Promise<ConfigResponse> {
  return request<ConfigResponse>('/config', { signal })
}

export function createSession(
  workingDir?: string,
  profile?: string,
  model?: string,
  signal?: AbortSignal,
): Promise<{ id: string; working_dir: string; model: string; profile: string }> {
  return request('/sessions', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      working_dir: workingDir || null,
      profile: profile || null,
      model: model || null,
    }),
    signal,
  })
}

export function getSession(id: string, signal?: AbortSignal): Promise<SessionSummary & { profile?: string; model?: string }> {
  return request(`/sessions/${encodeURIComponent(id)}`, { signal })
}

export function deleteSession(id: string, signal?: AbortSignal): Promise<void> {
  return request(`/sessions/${encodeURIComponent(id)}`, { method: 'DELETE', signal })
}

export function getMessages(id: string, signal?: AbortSignal): Promise<ApiMessage[]> {
  return request<{ messages: ApiMessage[] }>(
    `/sessions/${encodeURIComponent(id)}/messages`,
    { signal },
  ).then((r) => r.messages)
}

export function clearSession(id: string, signal?: AbortSignal): Promise<void> {
  return request(`/sessions/${encodeURIComponent(id)}/clear`, { method: 'POST', signal })
}

export function updateSessionConfig(
  id: string,
  profile?: string,
  model?: string,
  signal?: AbortSignal,
): Promise<{ status: string }> {
  return request(`/sessions/${encodeURIComponent(id)}/config`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ profile: profile || null, model: model || null }),
    signal,
  })
}

export interface BotInfo {
  name: string
  nickname: string
  role: string
  description: string
  skills: string[]
}

export interface BrowseEntry {
  name: string
  path: string
  kind: 'file' | 'directory'
  size?: number
  modified?: string
}

export interface BrowseResult {
  path: string
  parent: string | null
  entries: BrowseEntry[]
}

export function browseDirectory(dirPath?: string): Promise<BrowseResult> {
  const params = dirPath ? `?path=${encodeURIComponent(dirPath)}` : ''
  return request(`/browse${params}`)
}

export interface SkillInfo {
  name: string
  description: string
  tags: string
  path: string
}

export function listBots(): Promise<BotInfo[]> {
  return request<{ bots: BotInfo[] }>('/bots').then((r) => r.bots)
}

export function listSkills(): Promise<SkillInfo[]> {
  return request<{ skills: SkillInfo[] }>('/skills').then((r) => r.skills)
}
