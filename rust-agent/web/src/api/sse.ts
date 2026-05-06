import type { SSEEvent, SSEEventType } from '@/types/wire'

interface BufferedEvent {
  event: string
  dataLines: string[]
}

/**
 * Parse a raw SSE ReadableStream into typed SSEEvent objects.
 *
 * Uses \n\n frame splitting (not per-line) so multi-line data: fields
 * are correctly aggregated. Skips comment frames (lines starting with :).
 * Stops yielding when the 'done' event is received.
 */
export async function* parseSSEStream(
  response: Response,
  signal?: AbortSignal,
): AsyncGenerator<SSEEvent, void, undefined> {
  if (!response.body) throw new Error('Response has no body')

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  try {
    while (true) {
      if (signal?.aborted) break

      const { done, value } = await reader.read()
      if (done) break

      buffer += decoder.decode(value, { stream: true })

      // Split on double-newline (SSE frame boundary)
      let idx: number
      while ((idx = buffer.search(/\r?\n\r?\n/)) !== -1) {
        const frame = buffer.slice(0, idx)
        // Remove the frame (including the \n\n delimiter) from buffer
        buffer = buffer
          .slice(idx)
          .replace(/^\r?\n\r?\n/, '')

        if (!frame.trim()) continue

        const parsed = parseFrame(frame)
        if (!parsed) continue

        const { event, dataLines } = parsed
        if (event === 'done') {
          return
        }

        try {
          const data = JSON.parse(dataLines.join('\n'))
          yield { event: event as SSEEventType, data } as SSEEvent
        } catch {
          // malformed JSON — skip this frame
        }
      }
    }
  } finally {
    reader.releaseLock()
  }
}

function parseFrame(frame: string): BufferedEvent | null {
  let event = ''
  const dataLines: string[] = []

  for (const rawLine of frame.split(/\r?\n/)) {
    const line = rawLine.trim()
    // Skip empty lines and comment lines (keep-alive)
    if (!line || line.startsWith(':')) continue

    const colon = line.indexOf(':')
    const field = colon === -1 ? line : line.slice(0, colon)
    // Skip the leading space after colon per SSE spec
    const val = colon === -1 ? '' : line.slice(colon + 1).replace(/^ /, '')

    if (field === 'event') {
      event = val
    } else if (field === 'data') {
      dataLines.push(val)
    }
  }

  // Must have at least an event type
  if (!event) return null
  return { event, dataLines }
}

/**
 * POST a user message to a session.
 * 返回 { status: "started" }，SSE 流需通过 subscribeSessionStream 订阅。
 */
export async function sendMessageOnly(
  sessionId: string,
  content: string,
  signal: AbortSignal,
): Promise<{ status: string }> {
  const res = await fetch(`/sessions/${encodeURIComponent(sessionId)}/messages`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content }),
    signal,
  })
  if (!res.ok) {
    let msg = `HTTP ${res.status}`
    try {
      const body = await res.json()
      msg = body.message ?? msg
    } catch {
      /* ignore */
    }
    throw new Error(msg)
  }
  return res.json()
}

/**
 * 订阅会话的 SSE 实时流（支持刷新后重连）。
 */
export function subscribeSessionStream(
  sessionId: string,
  signal: AbortController,
): AsyncGenerator<SSEEvent, void, undefined> {
  return parseSSEStream(
    new Response(
      new ReadableStream({
        start: async (controller) => {
          try {
            const res = await fetch(
              `/sessions/${encodeURIComponent(sessionId)}/stream`,
              { signal: signal.signal },
            )

            if (!res.ok) {
              if (res.status === 404) {
                // 会话无活跃流 — 静默关闭，不抛错
                controller.close()
                return
              }
              let msg = `HTTP ${res.status}`
              try {
                const body = await res.json()
                msg = body.message ?? msg
              } catch {
                /* ignore */
              }
              controller.error(new Error(msg))
              return
            }

            if (!res.body) {
              controller.error(new Error('Response has no body'))
              return
            }

            const reader = res.body.getReader()
            try {
              while (true) {
                const { done, value } = await reader.read()
                if (done) break
                controller.enqueue(value)
              }
              controller.close()
            } finally {
              reader.releaseLock()
            }
          } catch (err: unknown) {
            if (err instanceof DOMException && err.name === 'AbortError') {
              controller.close()
            } else {
              controller.error(err)
            }
          }
        },
      }),
      {
        headers: { 'Content-Type': 'text/event-stream' },
      },
    ),
    signal.signal,
  )
}
