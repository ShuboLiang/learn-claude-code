// ── Backend wire types (matches crates/core/src/api/types.rs) ──

export interface TokenUsage {
  input_tokens: number
  output_tokens: number
}

export interface ParallelIndex {
  index: number
  total: number
}

// ── API content blocks (serde tag="type") ──

export type TextBlock = { type: 'text'; text: string }
export type ThinkingBlock = { type: 'thinking'; thinking: string }
export type ToolUseBlock = {
  type: 'tool_use'
  id: string | null
  name: string
  input: unknown
}
export type ToolResultBlock = {
  type: 'tool_result'
  tool_use_id: string | null
  content: string // 200-char preview
  name?: string
  is_error?: boolean
}

export type ApiContentBlock =
  | TextBlock
  | ThinkingBlock
  | ToolUseBlock
  | ToolResultBlock

// ── ApiMessage (matches the backend) ──

export interface ApiMessage {
  role: 'user' | 'assistant'
  content: string | ApiContentBlock[]
}

// ── SSE Events (8 variants, matches AgentEvent + agent_event_to_sse) ──

export type SSEEvent =
  | { event: 'text_delta'; data: { content: string } }
  | { event: 'thinking_delta'; data: { content: string } }
  | {
      event: 'tool_call'
      data: {
        name: string
        input: unknown
        id: string | null
        parallel_index?: ParallelIndex
      }
    }
  | {
      event: 'tool_result'
      data: {
        name: string
        output: string
        id: string | null
        parallel_index?: ParallelIndex
      }
    }
  | {
      event: 'turn_end'
      data: { api_calls: number; token_usage?: TokenUsage }
    }
  | { event: 'done'; data: Record<string, never> }
  | { event: 'error'; data: { code: string; message: string } }
  | {
      event: 'retrying'
      data: {
        attempt: number
        max_retries: number
        wait_seconds: number
        detail?: string
      }
    }

export type SSEEventType = SSEEvent['event']

// ── Session ──

export interface SessionSummary {
  id: string
  created_at: string
  last_active: string
  message_count: number
  preview: string | null
  working_dir: string
}
