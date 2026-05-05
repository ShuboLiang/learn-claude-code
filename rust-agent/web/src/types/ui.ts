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
