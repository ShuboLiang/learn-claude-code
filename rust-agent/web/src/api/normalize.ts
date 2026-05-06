import type { ApiMessage, ApiContentBlock } from '@/types/wire'
import type { UIMessage, UIToolCall, UIBlock } from '@/types/ui'

/**
 * Normalize raw ApiMessage[] into UIMessage[] for rendering.
 *
 * Three-pass approach:
 * 1. Collect tool_use blocks from assistant messages into a map keyed by tool_use_id
 * 2. Attach tool_result output from user messages back to matching tool calls
 * 3. Output UIMessage[], skipping user messages that are pure tool_result carriers
 */
export function normalizeApiMessages(
  apiMessages: ApiMessage[],
  generateId: () => string,
): UIMessage[] {
  console.log('[normalizeApiMessages] 输入消息数量:', apiMessages.length)
  apiMessages.forEach((m, i) => {
    console.log(`[normalizeApiMessages] msg[${i}] role=${m.role}, contentType=${typeof m.content}, isArray=${Array.isArray(m.content)}`)
  })
  const toolCallMap = new Map<string, UIToolCall>()

  // Pass 1: gather tool_use blocks from assistant messages
  for (const msg of apiMessages) {
    if (msg.role !== 'assistant' || typeof msg.content === 'string') continue
    for (const block of msg.content) {
      if (block.type !== 'tool_use') continue
      const id = block.id ?? generateId()
      toolCallMap.set(id, {
        id,
        name: block.name,
        input: block.input,
        output: null,
        status: 'done',
        parallelIndex: null,
      })
    }
  }

  // Pass 2: attach tool_result content to tool calls
  for (const msg of apiMessages) {
    if (msg.role !== 'user' || typeof msg.content === 'string') continue
    for (const block of msg.content) {
      if (block.type !== 'tool_result') continue
      const toolUseId = block.tool_use_id
      if (toolUseId && toolCallMap.has(toolUseId)) {
        const tc = toolCallMap.get(toolUseId)!
        tc.output = block.content
        if (block.is_error) {
          tc.status = 'error'
          tc.isError = true
        }
      }
    }
  }

  // Pass 3: build UIMessage[]
  const result: UIMessage[] = []

  for (const msg of apiMessages) {
    if (msg.role === 'assistant') {
      const blocks = assistantBlocks(msg.content, toolCallMap)
      console.log(`[normalizeApiMessages] assistant msg blocks=${blocks.length}, contentType=${typeof msg.content}`)
      if (blocks.length > 0) {
        result.push({
          id: generateId(),
          role: 'assistant',
          content: '',
          blocks,
        })
      } else {
        console.log('[normalizeApiMessages] ⚠️ assistant msg 被跳过（blocks 为空）')
      }
    } else {
      // User message: skip if it's purely tool_result blocks
      if (isPureToolResult(msg.content)) {
        console.log('[normalizeApiMessages] user msg 被跳过（纯 tool_result）')
        continue
      }
      const text = userText(msg.content)
      result.push({
        id: generateId(),
        role: 'user',
        content: text,
        blocks: [],
      })
    }
  }

  console.log('[normalizeApiMessages] 输出消息数量:', result.length)
  return result
}

function assistantBlocks(
  content: string | ApiContentBlock[],
  toolCallMap: Map<string, UIToolCall>,
): UIBlock[] {
  if (typeof content === 'string') {
    return content ? [{ kind: 'text', content }] : []
  }

  const blocks: UIBlock[] = []
  for (const block of content) {
    switch (block.type) {
      case 'text':
        blocks.push({ kind: 'text', content: block.text })
        break
      case 'thinking':
        blocks.push({ kind: 'thinking', content: block.thinking })
        break
      case 'tool_use': {
        const id = block.id ?? ''
        const tc = toolCallMap.get(id)
        if (tc) {
          blocks.push({ kind: 'toolCall', toolCall: tc })
        }
        break
      }
      case 'tool_result':
        // Tool results are handled in pass 2; skip here
        break
    }
  }
  return blocks
}

function userText(content: string | ApiContentBlock[]): string {
  if (typeof content === 'string') return content
  // User messages with mixed blocks: collect text blocks as content
  return content
    .filter((b): b is Extract<ApiContentBlock, { type: 'text' }> => b.type === 'text')
    .map((b) => b.text)
    .join('\n')
}

function isPureToolResult(content: string | ApiContentBlock[]): boolean {
  if (typeof content === 'string') return false
  if (content.length === 0) return false
  return content.every((b) => b.type === 'tool_result')
}
