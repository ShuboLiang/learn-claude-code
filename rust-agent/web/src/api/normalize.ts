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
  const toolCallMap = new Map<string, UIToolCall>()
  // 用 "消息索引:块索引" 作为 key，避免对象引用不匹配问题
  const blockIdMap = new Map<string, string>()

  // Pass 1: gather tool_use blocks from assistant messages
  for (let mi = 0; mi < apiMessages.length; mi++) {
    const msg = apiMessages[mi]
    if (msg.role !== 'assistant' || typeof msg.content === 'string') continue
    if (!Array.isArray(msg.content)) continue
    for (let bi = 0; bi < msg.content.length; bi++) {
      const block = msg.content[bi]
      if (block.type !== 'tool_use') continue
      const id = block.id ?? generateId()
      blockIdMap.set(`${mi}:${bi}`, id)
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

  for (let mi = 0; mi < apiMessages.length; mi++) {
    const msg = apiMessages[mi]
    if (msg.role === 'assistant') {
      const blocks = assistantBlocks(msg.content, toolCallMap, blockIdMap, mi)
      if (blocks.length > 0) {
        result.push({
          id: generateId(),
          role: 'assistant',
          content: '',
          blocks,
        })
      }
    } else {
      // User message: skip if it's purely tool_result blocks
      if (isPureToolResult(msg.content)) continue
      const text = userText(msg.content)
      result.push({
        id: generateId(),
        role: 'user',
        content: text,
        blocks: [],
      })
    }
  }

  return result
}

function assistantBlocks(
  content: string | ApiContentBlock[],
  toolCallMap: Map<string, UIToolCall>,
  blockIdMap: Map<string, string>,
  msgIndex: number,
): UIBlock[] {
  if (typeof content === 'string') {
    return content ? [{ kind: 'text', content }] : []
  }

  const blocks: UIBlock[] = []
  for (let bi = 0; bi < content.length; bi++) {
    const block = content[bi]
    switch (block.type) {
      case 'text':
        blocks.push({ kind: 'text', content: block.text })
        break
      case 'thinking':
        blocks.push({ kind: 'thinking', content: block.thinking })
        break
      case 'tool_use': {
        const id = block.id ?? blockIdMap.get(`${msgIndex}:${bi}`) ?? ''
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
