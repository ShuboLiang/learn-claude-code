import type { UIToolCall } from '@/types/ui'
import type { SSEEvent } from '@/types/wire'

/**
 * Push a new tool call from a streaming 'tool_call' event onto the tools array.
 * Handles parallel grouping via parallel_index.
 */
export function pushToolCall(
  tools: UIToolCall[],
  evt: Extract<SSEEvent, { event: 'tool_call' }>,
  generateId: () => string,
): UIToolCall {
  const tc: UIToolCall = {
    id: evt.data.id ?? generateId(),
    name: evt.data.name,
    input: evt.data.input,
    output: null,
    status: 'running',
    parallelIndex: evt.data.parallel_index ?? null,
  }
  tools.push(tc)
  return tc
}

/**
 * Attach a tool_result to the earliest unmatched tool call with the same name.
 *
 * Matching strategy (in priority order):
 * 1. If both have parallel_index, match by (name, index) within the group
 * 2. If the result has an id, match by id
 * 3. Fallback: FIFO — find the earliest unfinished tool call with the same name
 */
export function attachToolResult(
  tools: UIToolCall[],
  evt: Extract<SSEEvent, { event: 'tool_result' }>,
): UIToolCall | null {
  const { name, output, id, parallel_index } = evt.data

  // Strategy 1: match by parallel_index
  if (parallel_index) {
    const match = tools.find(
      (t) =>
        t.name === name &&
        t.status === 'running' &&
        t.parallelIndex?.index === parallel_index.index,
    )
    if (match) {
      match.output = output
      match.status = 'done'
      return match
    }
  }

  // Strategy 2: match by id
  if (id) {
    const match = tools.find((t) => t.id === id && t.status === 'running')
    if (match) {
      match.output = output
      match.status = 'done'
      return match
    }
  }

  // Strategy 3: FIFO — earliest unfinished with same name
  const match = tools.find((t) => t.name === name && t.status === 'running')
  if (match) {
    match.output = output
    match.status = 'done'
    return match
  }

  return null
}
