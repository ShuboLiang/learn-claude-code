// ── UI domain types (decoupled from wire format) ──

export type ToolStatus = 'running' | 'done' | 'error'

export interface UIToolCall {
  id: string // guaranteed non-null in UI (generated locally for streams)
  name: string
  input: unknown
  output: string | null // 200-char preview during stream; full after hydrate
  status: ToolStatus
  parallelIndex: { index: number; total: number } | null
  isError?: boolean
  /** Bot 子代理内部的嵌套工具调用（仅 call_bot 有） */
  children?: UIToolCall[]
}

// ── UI content blocks for message rendering ──

export type UITextBlock = { kind: 'text'; content: string }
export type UIThinkingBlock = { kind: 'thinking'; content: string }
export type UIToolCallBlock = { kind: 'toolCall'; toolCall: UIToolCall }
export type UIErrorBlock = { kind: 'error'; code: string; message: string }

export type UIBlock =
  | UITextBlock
  | UIThinkingBlock
  | UIToolCallBlock
  | UIErrorBlock

// ── UIMessage (normalized from ApiMessage[]) ──

export interface UIMessage {
  id: string
  role: 'user' | 'assistant'
  content: string // plain text for user; empty for assistant (renders via blocks)
  blocks: UIBlock[]
  tokenUsage?: { input: number; output: number }
  apiCalls?: number
}

// ── Streaming state (updated per SSE event, cleared on done) ──

export interface StreamingState {
  active: boolean
  assistantText: string
  thinking: string
  tools: UIToolCall[]
  /** 记录 blocks 首次出现的顺序，用于流式渲染时保持与 API 一致的顺序 */
  blockOrder: ('thinking' | 'text' | `tool:${string}`)[]
  /** 当前活跃的 Bot 名称（call_bot 内部的工具调用作为子项） */
  activeBotName: string | null
  error: { code: string; message: string } | null
  retrying: {
    attempt: number
    maxRetries: number
    waitSeconds: number
    detail?: string
  } | null
  apiCalls: number
  tokenUsage: { input: number; output: number } | null
  abort: AbortController
}

// ── Session list item ──

export interface SessionItem {
  id: string
  createdAt: string
  lastActive: string
  messageCount: number
  preview: string | null
}
