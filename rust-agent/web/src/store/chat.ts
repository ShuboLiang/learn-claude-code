import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import { nanoid } from "nanoid";
import type { ProfileInfo, SessionSummary } from "@/types/wire";
import type { UIMessage, StreamingState } from "@/types/ui";
import * as api from "@/api/client";
import { streamSendMessage } from "@/api/sse";
import { normalizeApiMessages } from "@/api/normalize";
import { pushToolCall, attachToolResult } from "@/api/match";

// ── State shape ──

interface ChatState {
  sessions: SessionSummary[];
  currentSessionId: string | null;
  messages: UIMessage[];
  streaming: StreamingState | null;
  /** 按会话存储的后台流式状态，切会话时不中断，切回时恢复 */
  streamingBySession: Record<string, StreamingState>;
  loadError: string | null;
  profiles: ProfileInfo[];
  selectedProfile: string;
  selectedModel: string;
}

interface ChatActions {
  loadSessions: () => Promise<void>;
  loadConfig: () => Promise<void>;
  createSession: (workingDir?: string) => Promise<void>;
  selectSession: (id: string) => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  clearCurrent: () => Promise<void>;
  sendMessage: (content: string) => Promise<void>;
  cancelStream: () => void;
  handleCommand: (cmd: string) => Promise<void>;
  setSelectedProfile: (profile: string) => void;
  setSelectedModel: (model: string) => void;
}

// function finalizeStreamingPreview(state: ChatState) {
//   const st = state.streaming
//   if (!st) return
//   const blocks: UIMessage['blocks'] = []
//   if (st.thinking) blocks.push({ kind: 'thinking', content: st.thinking })
//   if (st.assistantText) blocks.push({ kind: 'text', content: st.assistantText })
//   for (const tc of st.tools) blocks.push({ kind: 'toolCall', toolCall: tc })
//   if (st.error) blocks.push({ kind: 'error', code: st.error.code, message: st.error.message })
//   if (blocks.length > 0) {
//     state.messages.push({
//       id: nanoid(),
//       role: 'assistant',
//       content: '',
//       blocks,
//       apiCalls: st.apiCalls,
//       tokenUsage: st.tokenUsage ?? undefined,
//     })
//   }
// }

export const useChatStore = create<ChatState & ChatActions>()(
  immer((set, get) => ({
    // ── Initial state ──
    sessions: [],
    currentSessionId: null,
    messages: [],
    streaming: null,
    streamingBySession: {},
    loadError: null,
    profiles: [],
    selectedProfile: "",
    selectedModel: "",

    // ── Config action ──

    async loadConfig() {
      try {
        const config = await api.getConfig();
        const p = config.profiles.find(
          (p) => p.name === config.current_profile,
        );
        set((s) => {
          s.profiles = config.profiles;
          s.selectedProfile = config.current_profile || config.default_profile;
          s.selectedModel = config.current_model || p?.models[0] || "";
        });
      } catch (err) {
        console.error("加载配置失败:", err);
      }
    },

    // ── Session actions ──

    async loadSessions() {
      try {
        const sessions = await api.listSessions();
        set((s) => {
          s.sessions = sessions;
          s.loadError = null;
        });
      } catch (err) {
        set((s) => {
          s.loadError = err instanceof Error ? err.message : "加载会话失败";
        });
      }
    },

    async createSession(workingDir?: string) {
      const { selectedProfile, selectedModel } = get();
      const { id, working_dir } = await api.createSession(
        workingDir,
        selectedProfile,
        selectedModel,
      );
      set((s) => {
        // 保存当前会话的流式状态到后台（如果有）
        const prevId = s.currentSessionId;
        if (prevId && s.streaming) {
          s.streamingBySession[prevId] = s.streaming;
        }
        s.sessions.unshift({
          id,
          created_at: new Date().toISOString(),
          last_active: new Date().toISOString(),
          message_count: 0,
          preview: null,
          working_dir,
          profile_name: selectedProfile,
          model: selectedModel,
        });
        s.currentSessionId = id;
        s.messages = [];
        s.streaming = null;
      });
    },

    async selectSession(id: string) {
      const prevId = get().currentSessionId;
      set((s) => {
        // 保存当前会话的流式状态到后台 map
        if (prevId && s.streaming) {
          s.streamingBySession[prevId] = s.streaming;
        }
        s.currentSessionId = id;
        // 恢复目标会话的流式状态（不中断后台 SSE）
        s.streaming = s.streamingBySession[id] ?? null;
        // 注意：不立即清空 messages，避免空白闪烁；
        // getMessages 完成后替换即可
      });
      // 从已加载的 sessions 列表中同步 profile/model 到选择器
      // 避免调用 api.getSession（可能被写锁阻塞）
      const sess = get().sessions.find((ss) => ss.id === id);
      if (sess) {
        set((s) => {
          if (sess.profile_name) s.selectedProfile = sess.profile_name;
          if (sess.model) s.selectedModel = sess.model;
        });
      }
      try {
        const msgs = await api.getMessages(id);
        set((s) => {
          s.messages = normalizeApiMessages(msgs, nanoid);
        });
      } catch {
        // 加载失败时保留现有 messages（可能是旧会话的内容）
        // 或者如果是全新会话，messages 自然为空
      }
    },

    async deleteSession(id: string) {
      // 如果正在删除的会话有活跃流，先中断它
      const st = get().streamingBySession[id];
      if (st) st.abort.abort();
      await api.deleteSession(id);
      set((s) => {
        s.sessions = s.sessions.filter((ss) => ss.id !== id);
        delete s.streamingBySession[id];
        if (s.currentSessionId === id) {
          s.currentSessionId = null;
          s.messages = [];
          s.streaming = null;
        }
      });
    },

    async clearCurrent() {
      const id = get().currentSessionId;
      if (!id) return;
      // 中止当前会话的流并清理
      const st = get().streamingBySession[id] ?? get().streaming;
      if (st) {
        st.abort.abort();
        set((s) => {
          delete s.streamingBySession[id];
          if (s.streaming?.abort === st.abort) s.streaming = null;
        });
      }
      await api.clearSession(id);
      set((s) => {
        s.messages = [];
        if (s.currentSessionId === id) s.streaming = null;
      });
    },

    // ── Streaming ──

    async sendMessage(content: string) {
      const sid = get().currentSessionId;
      if (!sid) return;

      // 中止同一会话的旧流（如果存在）
      const oldSt = get().streamingBySession[sid];
      if (oldSt) {
        oldSt.abort.abort();
        set((s) => {
          delete s.streamingBySession[sid];
        });
      }
      // 如果当前展示的是这个会话，也清理 s.streaming
      if (get().streaming?.abort === oldSt?.abort) {
        set((s) => {
          s.streaming = null;
        });
      }

      const abortController = new AbortController();

      // 立即推入用户消息并更新侧边栏
      const now = new Date().toISOString();
      set((s) => {
        s.messages.push({ id: nanoid(), role: "user", content, blocks: [] });
        const si = s.sessions.findIndex((ss) => ss.id === sid);
        if (si >= 0) {
          s.sessions[si].message_count += 1;
          s.sessions[si].preview = content.slice(0, 100);
          s.sessions[si].last_active = now;
          if (si > 0) {
            const [sess] = s.sessions.splice(si, 1);
            s.sessions.unshift(sess);
          }
        }
        const st: StreamingState = {
          active: true,
          assistantText: "",
          thinking: "",
          tools: [],
          error: null,
          retrying: null,
          apiCalls: 0,
          tokenUsage: null,
          abort: abortController,
        };
        s.streamingBySession[sid] = st;
        if (s.currentSessionId === sid) s.streaming = st;
      });

      const isAborted = () => abortController.signal.aborted;

      try {
        const stream = streamSendMessage(sid, content, abortController);

        for await (const evt of stream) {
          if (isAborted()) break;

          set((s) => {
            const target = s.streamingBySession[sid];
            if (!target || target.abort !== abortController) return;

            switch (evt.event) {
              case "text_delta":
                target.assistantText += evt.data.content;
                break;
              case "thinking_delta":
                target.thinking += evt.data.content;
                break;
              case "tool_call":
                pushToolCall(target.tools, evt, nanoid);
                break;
              case "tool_result":
                attachToolResult(target.tools, evt);
                break;
              case "turn_end":
                target.apiCalls = evt.data.api_calls;
                if (evt.data.token_usage) {
                  target.tokenUsage = {
                    input: evt.data.token_usage.input_tokens,
                    output: evt.data.token_usage.output_tokens,
                  };
                }
                break;
              case "error":
                target.error = {
                  code: evt.data.code,
                  message: evt.data.message,
                };
                break;
              case "retrying":
                target.retrying = {
                  attempt: evt.data.attempt,
                  maxRetries: evt.data.max_retries,
                  waitSeconds: evt.data.wait_seconds,
                  detail: evt.data.detail,
                };
                break;
              case "done":
                target.active = false;
                break;
            }

            // 同步到当前展示的 streaming
            if (s.currentSessionId === sid) s.streaming = target;
          });
        }

        // ---- 流结束后的清理与 hydration ----
        const finalize = () => {
          set((s) => {
            const target = s.streamingBySession[sid];
            if (!target || target.abort !== abortController) return;
            const blocks: UIMessage["blocks"] = [];
            if (target.thinking)
              blocks.push({ kind: "thinking", content: target.thinking });
            if (target.assistantText)
              blocks.push({ kind: "text", content: target.assistantText });
            for (const tc of target.tools)
              blocks.push({ kind: "toolCall", toolCall: tc });
            if (target.error)
              blocks.push({
                kind: "error",
                code: target.error.code,
                message: target.error.message,
              });
            if (blocks.length > 0) {
              s.messages.push({
                id: nanoid(),
                role: "assistant",
                content: "",
                blocks,
                apiCalls: target.apiCalls,
                tokenUsage: target.tokenUsage ?? undefined,
              });
            }
            delete s.streamingBySession[sid];
            if (s.currentSessionId === sid) s.streaming = null;
          });
        };

        if (isAborted()) {
          finalize();
          return;
        }

        try {
          const msgs = await api.getMessages(sid);
          set((s) => {
            const target = s.streamingBySession[sid];
            if (!target || target.abort !== abortController) return;
            const hydrated = normalizeApiMessages(msgs, nanoid);
            const prevLen = s.messages.length;
            if (hydrated.length > prevLen) {
              for (let i = prevLen; i < hydrated.length; i++)
                s.messages.push(hydrated[i]);
            } else {
              s.messages = hydrated;
            }
            if (target.error) {
              s.messages.push({
                id: nanoid(),
                role: "assistant",
                content: "",
                blocks: [
                  {
                    kind: "error",
                    code: target.error.code,
                    message: target.error.message,
                  },
                ],
              });
            }
            delete s.streamingBySession[sid];
            if (s.currentSessionId === sid) s.streaming = null;
          });
        } catch {
          finalize();
        }
      } catch (err: unknown) {
        set((s) => {
          const target = s.streamingBySession[sid];
          if (!target || target.abort !== abortController) return;
          const code =
            err instanceof DOMException && err.name === "AbortError"
              ? "ABORT"
              : "NETWORK";
          const message = err instanceof Error ? err.message : "未知错误";
          if (code !== "ABORT") target.error = { code, message };
          const blocks: UIMessage["blocks"] = [];
          if (target.thinking)
            blocks.push({ kind: "thinking", content: target.thinking });
          if (target.assistantText)
            blocks.push({ kind: "text", content: target.assistantText });
          for (const tc of target.tools)
            blocks.push({ kind: "toolCall", toolCall: tc });
          if (target.error)
            blocks.push({
              kind: "error",
              code: target.error.code,
              message: target.error.message,
            });
          if (blocks.length > 0) {
            s.messages.push({
              id: nanoid(),
              role: "assistant",
              content: "",
              blocks,
              apiCalls: target.apiCalls,
              tokenUsage: target.tokenUsage ?? undefined,
            });
          }
          delete s.streamingBySession[sid];
          if (s.currentSessionId === sid) s.streaming = null;
        });
      }
    },

    async handleCommand(cmd: string) {
      if (cmd === "/bots") {
        try {
          const bots = await api.listBots();
          set((s) => {
            if (bots.length === 0) {
              s.messages.push({
                id: nanoid(),
                role: "user",
                content: "/bots",
                blocks: [],
              });
              s.messages.push({
                id: nanoid(),
                role: "assistant",
                content: "",
                blocks: [
                  {
                    kind: "text",
                    content:
                      "暂无已配置的 Bot。将 Bot 定义文件放入 skills/ 或 ~/.rust-agent/skills/ 目录即可。",
                  },
                ],
              });
            } else {
              const lines = bots.map(
                (b) =>
                  `- **${b.nickname || b.name}** (/${b.name}) — ${b.role || "No description"}`,
              );
              s.messages.push({
                id: nanoid(),
                role: "user",
                content: "/bots",
                blocks: [],
              });
              s.messages.push({
                id: nanoid(),
                role: "assistant",
                content: "",
                blocks: [{ kind: "text", content: lines.join("\n") }],
              });
            }
          });
        } catch {
          set((s) => {
            s.messages.push({
              id: nanoid(),
              role: "assistant",
              content: "",
              blocks: [
                {
                  kind: "error",
                  code: "BOTS_ERROR",
                  message: "获取 Bot 列表失败",
                },
              ],
            });
          });
        }
      }
    },

    setSelectedProfile(profile: string) {
      const state = get();
      set((s) => {
        s.selectedProfile = profile;
        const p = s.profiles.find((p) => p.name === profile);
        s.selectedModel = p?.models[0] || "";
      });
      // 如果有当前会话，同步更新
      if (state.currentSessionId) {
        api
          .updateSessionConfig(
            state.currentSessionId,
            profile,
            get().selectedModel,
          )
          .catch(() => {});
      }
    },

    setSelectedModel(model: string) {
      const { currentSessionId } = get();
      set((s) => {
        s.selectedModel = model;
      });
      // 如果有当前会话，同步更新
      if (currentSessionId) {
        api
          .updateSessionConfig(currentSessionId, undefined, model)
          .catch(() => {});
      }
    },

    cancelStream() {
      const state = get();
      const sid = state.currentSessionId;
      if (!sid) return;
      // 优先从 streamingBySession 中 abort（后台流也包含在内）
      const st = state.streamingBySession[sid] ?? state.streaming;
      if (st) {
        st.abort.abort();
        set((s) => {
          delete s.streamingBySession[sid];
          if (s.streaming?.abort === st.abort) s.streaming = null;
        });
      }
    },
  })),
);
