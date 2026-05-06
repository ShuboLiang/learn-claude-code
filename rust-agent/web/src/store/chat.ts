import { create } from 'zustand'
import { immer } from 'zustand/middleware/immer'
import { nanoid } from 'nanoid'
import type { ProfileInfo, SessionSummary } from '@/types/wire'
import type { UIMessage, StreamingState } from '@/types/ui'
import * as api from '@/api/client'
import { streamSendMessage } from '@/api/sse'
import { normalizeApiMessages } from '@/api/normalize'
import { pushToolCall, attachToolResult } from '@/api/match'

// ── State shape ──

interface ChatState {
  sessions: SessionSummary[]
  currentSessionId: string | null
  messages: UIMessage[]
  streaming: StreamingState | null
  loadError: string | null
  profiles: ProfileInfo[]
  selectedProfile: string
  selectedModel: string
}

interface ChatActions {
  loadSessions: () => Promise<void>
  loadConfig: () => Promise<void>
  createSession: (workingDir?: string) => Promise<void>
  selectSession: (id: string) => Promise<void>
  deleteSession: (id: string) => Promise<void>
  clearCurrent: () => Promise<void>
  sendMessage: (content: string) => Promise<void>
  cancelStream: () => void
  handleCommand: (cmd: string) => Promise<void>
  setSelectedProfile: (profile: string) => void
  setSelectedModel: (model: string) => void
}

function finalizeStreamingPreview(state: ChatState) {
  const st = state.streaming
  if (!st) return
  const blocks: UIMessage['blocks'] = []
  if (st.thinking) blocks.push({ kind: 'thinking', content: st.thinking })
  if (st.assistantText) blocks.push({ kind: 'text', content: st.assistantText })
  for (const tc of st.tools) blocks.push({ kind: 'toolCall', toolCall: tc })
  if (st.error) blocks.push({ kind: 'error', code: st.error.code, message: st.error.message })
  if (blocks.length > 0) {
    state.messages.push({
      id: nanoid(),
      role: 'assistant',
      content: '',
      blocks,
      apiCalls: st.apiCalls,
      tokenUsage: st.tokenUsage ?? undefined,
    })
  }
}

export const useChatStore = create<ChatState & ChatActions>()(
  immer((set, get) => ({
    // ── Initial state ──
    sessions: [],
    currentSessionId: null,
    messages: [],
    streaming: null,
    loadError: null,
    profiles: [],
    selectedProfile: '',
    selectedModel: '',

    // ── Config action ──

    async loadConfig() {
      try {
        const config = await api.getConfig()
        const p = config.profiles.find((p) => p.name === config.current_profile)
        set((s) => {
          s.profiles = config.profiles
          s.selectedProfile = config.current_profile || config.default_profile
          s.selectedModel = config.current_model || p?.models[0] || ''
        })
      } catch (err) {
        console.error('Failed to load config:', err)
      }
    },

    // ── Session actions ──

    async loadSessions() {
      try {
        const sessions = await api.listSessions()
        set((s) => {
          s.sessions = sessions
          s.loadError = null
        })
      } catch (err) {
        set((s) => {
          s.loadError = err instanceof Error ? err.message : 'Failed to load sessions'
        })
      }
    },

    async createSession(workingDir?: string) {
      const { selectedProfile, selectedModel } = get()
      const { id, working_dir } = await api.createSession(workingDir, selectedProfile, selectedModel)
      set((s) => {
        s.sessions.unshift({
          id,
          created_at: new Date().toISOString(),
          last_active: new Date().toISOString(),
          message_count: 0,
          preview: null,
          working_dir,
        })
        s.currentSessionId = id
        s.messages = []
        s.streaming = null
      })
    },

    async selectSession(id: string) {
      set((s) => {
        s.currentSessionId = id
        s.messages = []
        s.streaming = null
      })
      try {
        const msgs = await api.getMessages(id)
        set((s) => {
          s.messages = normalizeApiMessages(msgs, nanoid)
        })
      } catch {
        // session may not exist — leave messages empty
      }
    },

    async deleteSession(id: string) {
      await api.deleteSession(id)
      set((s) => {
        s.sessions = s.sessions.filter((ss) => ss.id !== id)
        if (s.currentSessionId === id) {
          s.currentSessionId = null
          s.messages = []
          s.streaming = null
        }
      })
    },

    async clearCurrent() {
      const id = get().currentSessionId
      if (!id) return
      await api.clearSession(id)
      set((s) => {
        s.messages = []
        s.streaming = null
      })
    },

    // ── Streaming ──

    async sendMessage(content: string) {
      const sid = get().currentSessionId
      if (!sid) return

      get().cancelStream()
      const abortController = new AbortController()

      // Add user message immediately
      set((s) => {
        s.messages.push({
          id: nanoid(),
          role: 'user',
          content,
          blocks: [],
        })
        s.streaming = {
          active: true,
          assistantText: '',
          thinking: '',
          tools: [],
          error: null,
          retrying: null,
          apiCalls: 0,
          tokenUsage: null,
          abort: abortController,
        }
      })

      const isCurrent = () => get().streaming?.abort === abortController

      try {
        const stream = streamSendMessage(sid, content, abortController)

        for await (const evt of stream) {
          if (!isCurrent()) break

          set((s) => {
            if (!s.streaming || s.streaming.abort !== abortController) return

            switch (evt.event) {
              case 'text_delta':
                s.streaming.assistantText += evt.data.content
                break
              case 'thinking_delta':
                s.streaming.thinking += evt.data.content
                break
              case 'tool_call':
                pushToolCall(s.streaming.tools, evt, nanoid)
                break
              case 'tool_result':
                attachToolResult(s.streaming.tools, evt)
                break
              case 'turn_end':
                s.streaming.apiCalls = evt.data.api_calls
                if (evt.data.token_usage) {
                  s.streaming.tokenUsage = {
                    input: evt.data.token_usage.input_tokens,
                    output: evt.data.token_usage.output_tokens,
                  }
                }
                break
              case 'error':
                s.streaming.error = {
                  code: evt.data.code,
                  message: evt.data.message,
                }
                break
              case 'retrying':
                s.streaming.retrying = {
                  attempt: evt.data.attempt,
                  maxRetries: evt.data.max_retries,
                  waitSeconds: evt.data.wait_seconds,
                  detail: evt.data.detail,
                }
                break
              case 'done':
                s.streaming.active = false
                break
            }
          })
        }

        // Hydrate full messages from server
        if (!isCurrent()) return
        try {
          const msgs = await api.getMessages(sid)
          set((s) => {
            if (s.streaming?.abort === abortController) {
              const hydrated = normalizeApiMessages(msgs, nanoid)
              // Only append new assistant messages to preserve existing keys
              const prevLen = s.messages.length
              if (hydrated.length > prevLen) {
                for (let i = prevLen; i < hydrated.length; i++) {
                  s.messages.push(hydrated[i])
                }
              } else {
                s.messages = hydrated
              }
              s.streaming = null
            }
          })
        } catch {
          set((s) => {
            if (s.streaming?.abort === abortController) {
              finalizeStreamingPreview(s)
              s.streaming = null
            }
          })
        }
      } catch (err: unknown) {
        if (err instanceof DOMException && err.name === 'AbortError') {
          set((s) => {
            if (s.streaming?.abort === abortController) {
              finalizeStreamingPreview(s)
              s.streaming = null
            }
          })
        } else {
          set((s) => {
            if (s.streaming?.abort === abortController) {
              s.streaming.error = {
                code: 'NETWORK',
                message: err instanceof Error ? err.message : 'Unknown error',
              }
              s.streaming.active = false
            }
          })
        }
      }
    },

    async handleCommand(cmd: string) {
      if (cmd === '/bots') {
        try {
          const bots = await api.listBots()
          set((s) => {
            if (bots.length === 0) {
              s.messages.push({
                id: nanoid(),
                role: 'user',
                content: '/bots',
                blocks: [],
              })
              s.messages.push({
                id: nanoid(),
                role: 'assistant',
                content: '',
                blocks: [
                  { kind: 'text', content: '暂无已配置的 Bot。将 Bot 定义文件放入 skills/ 或 ~/.rust-agent/skills/ 目录即可。' },
                ],
              })
            } else {
              const lines = bots.map(
                (b) => `- **${b.nickname || b.name}** (/${b.name}) — ${b.role || 'No description'}`,
              )
              s.messages.push({
                id: nanoid(),
                role: 'user',
                content: '/bots',
                blocks: [],
              })
              s.messages.push({
                id: nanoid(),
                role: 'assistant',
                content: '',
                blocks: [{ kind: 'text', content: lines.join('\n') }],
              })
            }
          })
        } catch {
          set((s) => {
            s.messages.push({
              id: nanoid(),
              role: 'assistant',
              content: '',
              blocks: [{ kind: 'error', code: 'BOTS_ERROR', message: '获取 Bot 列表失败' }],
            })
          })
        }
      }
    },

    setSelectedProfile(profile: string) {
      set((s) => {
        s.selectedProfile = profile
        const p = s.profiles.find((p) => p.name === profile)
        s.selectedModel = p?.models[0] || ''
      })
    },

    setSelectedModel(model: string) {
      set((s) => {
        s.selectedModel = model
      })
    },

    cancelStream() {
      const streaming = get().streaming
      if (streaming) {
        streaming.abort.abort()
        set((s) => {
          s.streaming = null
        })
      }
    },
  })),
)
