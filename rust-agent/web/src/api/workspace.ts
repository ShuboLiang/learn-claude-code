import type { BrowseEntry, BrowseResult } from '@/api/client'
import { browseDirectory } from '@/api/client'

export type { BrowseEntry, BrowseResult }
export { browseDirectory }

export interface WatchEvent {
  event: 'file_created' | 'file_modified' | 'file_removed'
  data: {
    path: string
    kind: 'file' | 'directory'
  }
}

export async function streamWatchEvents(
  sessionId: string,
  onEvent: (evt: WatchEvent) => void,
  signal: AbortSignal,
): Promise<void> {
  const url = `/watch?session_id=${encodeURIComponent(sessionId)}`
  const res = await fetch(url, { signal })

  if (!res.ok || !res.body) {
    throw new Error(`Watch stream failed: ${res.status}`)
  }

  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  try {
    while (true) {
      const { done, value } = await reader.read()
      if (done) break

      buffer += decoder.decode(value, { stream: true })
      const lines = buffer.split('\n')
      buffer = lines.pop() || ''

      let currentEvent = ''
      let currentData = ''

      for (const line of lines) {
        if (line.startsWith('event: ')) {
          currentEvent = line.slice(7).trim()
        } else if (line.startsWith('data: ')) {
          currentData = line.slice(6).trim()
        } else if (line === '') {
          if (currentEvent && currentData) {
            try {
              const parsed = JSON.parse(currentData)
              onEvent({
                event: currentEvent as WatchEvent['event'],
                data: parsed,
              })
            } catch {
              // skip unparseable events
            }
          }
          currentEvent = ''
          currentData = ''
        }
      }
    }
  } finally {
    reader.releaseLock()
  }
}

export async function readFile(
  sessionId: string,
  filePath: string,
): Promise<string> {
  const params = `session_id=${encodeURIComponent(sessionId)}&path=${encodeURIComponent(filePath)}`
  const res = await fetch(`/file?${params}`)
  if (!res.ok) {
    const body = await res.json().catch(() => ({}))
    throw new Error(body?.error?.message || `Failed to read file: ${res.status}`)
  }
  const data = await res.json()
  return data.content as string
}

export async function writeFile(
  sessionId: string,
  filePath: string,
  content: string,
): Promise<void> {
  const params = `session_id=${encodeURIComponent(sessionId)}&path=${encodeURIComponent(filePath)}`
  const res = await fetch(`/file?${params}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content }),
  })
  if (!res.ok) {
    const body = await res.json().catch(() => ({}))
    throw new Error(body?.error?.message || `Failed to write file: ${res.status}`)
  }
}
