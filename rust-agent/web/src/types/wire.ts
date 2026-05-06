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

// ── 事件来源标识 ──
export type EventSource =
  | { role: 'main' }
  | { role: 'bot'; name: string; call_id: string }

// ── SSE Events (8 variants, matches AgentEvent + agent_event_to_sse) ──

export type SSEEvent =
  | { event: 'text_delta'; data: { content: string; source: EventSource } }
  | { event: 'thinking_delta'; data: { content: string; source: EventSource } }
  | {
      event: 'tool_call'
      data: {
        name: string
        input: unknown
        id: string | null
        parallel_index?: ParallelIndex
        source: EventSource
      }
    }
  | {
      event: 'tool_result'
      data: {
        name: string
        output: string
        id: string | null
        parallel_index?: ParallelIndex
        source: EventSource
      }
    }
  | {
      event: 'turn_end'
      data: { api_calls: number; token_usage?: TokenUsage; source: EventSource }
    }
  | { event: 'done'; data: { source: EventSource } }
  | { event: 'error'; data: { code: string; message: string; source: EventSource } }
  | {
      event: 'retrying'
      data: {
        attempt: number
        max_retries: number
        wait_seconds: number
        detail?: string
        source: EventSource
      }
    }

export type SSEEventType = SSEEvent['event']

// ── Watch (file system events via SSE) ──

export interface WatchEvent {
  event: 'file_created' | 'file_modified' | 'file_removed'
  data: {
    path: string
    kind: 'file' | 'directory'
  }
}

// ── Config ──

export interface ProfileInfo {
  name: string
  provider: string
  models: string[]
}

export interface ConfigResponse {
  default_profile: string
  current_profile: string
  current_model: string
  profiles: ProfileInfo[]
}

// ── Session ──

export interface SessionSummary {
  id: string
  created_at: string
  last_active: string
  message_count: number
  preview: string | null
  working_dir: string
  profile_name: string
  model: string
}
