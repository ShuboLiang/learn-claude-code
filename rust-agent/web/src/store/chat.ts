import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import { nanoid } from "nanoid";
import type { ProfileInfo, SessionSummary } from "@/types/wire";
import type { UIMessage, StreamingState } from "@/types/ui";
import * as api from "@/api/client";
import { sendMessageOnly, subscribeSessionStream } from "@/api/sse";
import { normalizeApiMessages } from "@/api/normalize";
import type { UIBlock } from "@/types/ui";

/** 根据 StreamingState 的 blockOrder 构造 UIBlock 数组，保持与事件到达顺序一致 */
export function buildStreamingBlocks(st: {
  blockOrder: ('thinking' | 'text' | `tool:${string}`)[]
  thinking: string
  assistantText: string
  tools: { id: string; name: string; input: unknown; output: string | null; status: 'running' | 'done' | 'error'; parallelIndex: { index: number; total: number } | null; isError?: boolean }[]
  error: { code: string; message: string } | null
}): UIBlock[] {
  const blocks: UIBlock[] = []
  for (const key of st.blockOrder) {
    if (key === 'thinking' && st.thinking) {
      blocks.push({ kind: 'thinking', content: st.thinking })
    } else if (key === 'text' && st.assistantText) {
      blocks.push({ kind: 'text', content: st.assistantText })
    } else if (key.startsWith('tool:')) {
      const toolId = key.slice(5)
      const tc = st.tools.find((t) => t.id === toolId)
      if (tc) blocks.push({ kind: 'toolCall', toolCall: tc })
    }
  }
  if (st.error) {
    blocks.push({ kind: 'error', code: st.error.code, message: st.error.message })
  }
  return blocks
}

// ── SSE 事件循环（供 sendMessage 和 selectSession 复用）──

async function runSSELoop(
  sid: string,
  abortController: AbortController,
  set: (fn: (s: ChatState & ChatActions) => void) => void,
  get: () => ChatState & ChatActions,
) {
  const isAborted = () => abortController.signal.aborted;

  try {
    const stream = subscribeSessionStream(sid, abortController);
    let receivedAnyEvent = false;

    for await (const evt of stream) {
      if (isAborted()) break;
      receivedAnyEvent = true;

      set((s) => {
        const target = s.streamingBySession[sid];
        if (!target || target.abort !== abortController) return;

        switch (evt.event) {
          case "text_delta":
            if (!target.assistantText && !target.blockOrder.includes('text')) {
              target.blockOrder.push('text');
            }
            target.assistantText += evt.data.content;
            break;
          case "thinking_delta":
            if (!target.thinking && !target.blockOrder.includes('thinking')) {
              target.blockOrder.push('thinking');
            }
            target.thinking += evt.data.content;
            break;
          case "tool_call": {
            const tc = {
              id: nanoid(),
              name: evt.data.name,
              input: evt.data.input,
              output: null,
              status: 'running' as const,
              parallelIndex: evt.data.parallel_index ?? null,
            };
            target.tools.push(tc);
            target.blockOrder.push(`tool:${tc.id}`);
            break;
          }
          case "tool_result": {
            const tc = target.tools.find(
              (t) => t.name === evt.data.name && t.output === null,
            );
            if (tc) {
              tc.output = evt.data.output;
              tc.status = 'done';
            }
            break;
          }
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
        const blocks = buildStreamingBlocks(target);
        if (blocks.length > 0) {
          const assistantMsg = {
            id: nanoid(),
            role: "assistant" as const,
            content: "",
            blocks,
            apiCalls: target.apiCalls,
            tokenUsage: target.tokenUsage ?? undefined,
          };
          s.messages.push(assistantMsg);
          // 同步缓存
          if (s.messagesBySession[sid]) {
            s.messagesBySession[sid].push(assistantMsg);
          }
        }
        delete s.streamingBySession[sid];
        if (s.currentSessionId === sid) s.streaming = null;
      });
    };

    if (isAborted()) {
      finalize();
      return;
    }

    // 未收到任何事件（404 空流，会话无活跃流）：直接清理状态，跳过 hydration
    if (!receivedAnyEvent) {
      set((s) => {
        delete s.streamingBySession[sid];
        if (s.currentSessionId === sid) s.streaming = null;
      });
      return;
    }

    try {
      const msgs = await api.getMessages(sid);
      set((s) => {
        const target = s.streamingBySession[sid];
        if (!target || target.abort !== abortController) return;
        const hydrated = normalizeApiMessages(msgs, nanoid);
        
        // 更新当前显示和缓存
        if (s.currentSessionId === sid) s.messages = hydrated;
        s.messagesBySession[sid] = hydrated;

        if (target.error) {
          const errorMsg = {
            id: nanoid(),
            role: "assistant" as const,
            content: "",
            blocks: [
              {
                kind: "error" as const,
                code: target.error.code,
                message: target.error.message,
              },
            ],
          };
          s.messages.push(errorMsg);
          if (s.messagesBySession[sid]) s.messagesBySession[sid].push(errorMsg);
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
      const blocks = buildStreamingBlocks(target);
      if (blocks.length > 0) {
        const assistantMsg = {
          id: nanoid(),
          role: "assistant" as const,
          content: "",
          blocks,
          apiCalls: target.apiCalls,
          tokenUsage: target.tokenUsage ?? undefined,
        };
        s.messages.push(assistantMsg);
        if (s.messagesBySession[sid]) s.messagesBySession[sid].push(assistantMsg);
      }
      delete s.streamingBySession[sid];
      if (s.currentSessionId === sid) s.streaming = null;
    });
  }
}

// ── State shape ──

interface ChatState {
  sessions: SessionSummary[];
  currentSessionId: string | null;
  messages: UIMessage[];
  /** 缓存各会话的消息列表，实现秒切 */
  messagesBySession: Record<string, UIMessage[]>;
  streaming: StreamingState | null;
  /** 按会话存储的后台流式状态，切会话时不中断，切回时恢复 */
  streamingBySession: Record<string, StreamingState>;
  loadError: string | null;
  profiles: ProfileInfo[];
  selectedProfile: string;
  selectedModel: string;
  /** 用于取消正在进行的 getMessages 请求 */
  loadAbortController: AbortController | null;
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
    messagesBySession: {},
    streaming: null,
    streamingBySession: {},
    loadError: null,
    profiles: [],
    selectedProfile: "",
    selectedModel: "",
    loadAbortController: null,

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
        s.messagesBySession[id] = [];
        s.streaming = null;
      });
    },

    async selectSession(id: string) {
      const state = get();
      const prevId = state.currentSessionId;

      // 1. 取消正在进行的加载请求
      if (state.loadAbortController) {
        state.loadAbortController.abort();
      }

      const abortController = new AbortController();

      set((s) => {
        // 保存当前会话的流式状态到后台 map
        if (prevId && s.streaming) {
          s.streamingBySession[prevId] = s.streaming;
        }
        s.currentSessionId = id;
        s.loadAbortController = abortController;

        // 恢复目标会话的流式状态（不中断后台 SSE）
        s.streaming = s.streamingBySession[id] ?? null;

        // 优先从缓存恢复，实现秒切
        s.messages = s.messagesBySession[id] ?? [];
      });

      // 同步 profile/model 选择器
      const sess = get().sessions.find((ss) => ss.id === id);
      if (sess) {
        set((s) => {
          if (sess.profile_name) s.selectedProfile = sess.profile_name;
          if (sess.model) s.selectedModel = sess.model;
        });
      }

      // 2. 异步加载/刷新消息列表
      console.log('[selectSession] 开始加载消息, 当前缓存长度:', get().messagesBySession[id]?.length ?? 0);
      try {
        const msgs = await api.getMessages(id, abortController.signal);
        console.log('[selectSession] getMessages 返回原始消息数量:', msgs.length);
        const normalized = normalizeApiMessages(msgs, nanoid);
        console.log('[selectSession] normalize 后消息数量:', normalized.length);

        set((s) => {
          // 仅在当前会话仍然匹配时更新（虽然有 AbortSignal，但双重保险更安全）
          if (s.currentSessionId === id) {
            s.messages = normalized;
            s.messagesBySession[id] = normalized;
            s.loadAbortController = null;
          }
        });
      } catch (err: any) {
        if (err.name === 'AbortError') {
          console.log('[selectSession] getMessages 被取消');
          return;
        }
        console.error("[selectSession] 加载消息失败:", err);
      }

      // 3. 尝试恢复会话的实时 SSE 流
      const sseAbortController = new AbortController();
      const existingSt = get().streamingBySession[id];

      if (existingSt && existingSt.active) {
        set((s) => {
          const st = s.streamingBySession[id];
          if (st) {
            st.abort = sseAbortController;
            if (s.currentSessionId === id) s.streaming = st;
          }
        });
      } else {
        const recoverSt: StreamingState = {
          active: true,
          assistantText: "",
          thinking: "",
          tools: [],
          blockOrder: [],
          error: null,
          retrying: null,
          apiCalls: 0,
          tokenUsage: null,
          abort: sseAbortController,
        };
        set((s) => {
          s.streamingBySession[id] = recoverSt;
          if (s.currentSessionId === id) s.streaming = recoverSt;
        });
      }

      runSSELoop(id, sseAbortController, set, get).catch(() => {
        set((s) => {
          delete s.streamingBySession[id];
          if (s.currentSessionId === id) s.streaming = null;
        });
      });
    },

    async deleteSession(id: string) {
      // 如果正在删除的会话有活跃流，先中断它
      const st = get().streamingBySession[id];
      if (st) st.abort.abort();
      await api.deleteSession(id);
      set((s) => {
        s.sessions = s.sessions.filter((ss) => ss.id !== id);
        delete s.streamingBySession[id];
        delete s.messagesBySession[id]; // 同步清理缓存
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
        s.messagesBySession[id] = []; // 同步清空缓存
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
        const userMsg = { id: nanoid(), role: "user" as const, content, blocks: [] };
        s.messages.push(userMsg);
        
        // 同步缓存
        if (!s.messagesBySession[sid]) s.messagesBySession[sid] = [];
        s.messagesBySession[sid].push(userMsg);

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
          blockOrder: [],
          error: null,
          retrying: null,
          apiCalls: 0,
          tokenUsage: null,
          abort: abortController,
        };
        s.streamingBySession[sid] = st;
        if (s.currentSessionId === sid) s.streaming = st;
      });

      // 先 POST 发送消息，然后订阅 SSE 流
      try {
        await sendMessageOnly(sid, content, abortController.signal);
      } catch (err: unknown) {
        set((s) => {
          const target = s.streamingBySession[sid];
          if (!target || target.abort !== abortController) return;
          target.error = {
            code: "NETWORK",
            message: err instanceof Error ? err.message : "发送失败",
          };
          delete s.streamingBySession[sid];
          if (s.currentSessionId === sid) s.streaming = null;
        });
        return;
      }

      // 订阅 SSE 实时流
      await runSSELoop(sid, abortController, set, get);
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
      const { currentSessionId } = get();
      set((s) => {
        s.selectedProfile = profile;
        const p = s.profiles.find((p) => p.name === profile);
        const model = p?.models[0] || "";
        s.selectedModel = model;

        if (currentSessionId) {
          const si = s.sessions.findIndex((ss) => ss.id === currentSessionId);
          if (si >= 0) {
            s.sessions[si].profile_name = profile;
            s.sessions[si].model = model;
            s.sessions[si].last_active = new Date().toISOString();
            // 重新排序
            s.sessions.sort(
              (a, b) =>
                new Date(b.last_active).getTime() -
                new Date(a.last_active).getTime(),
            );
          }
        }
      });
      // 如果有当前会话，同步更新
      if (currentSessionId) {
        api
          .updateSessionConfig(
            currentSessionId,
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

        if (currentSessionId) {
          const si = s.sessions.findIndex((ss) => ss.id === currentSessionId);
          if (si >= 0) {
            s.sessions[si].model = model;
            s.sessions[si].last_active = new Date().toISOString();
            // 重新排序
            s.sessions.sort(
              (a, b) =>
                new Date(b.last_active).getTime() -
                new Date(a.last_active).getTime(),
            );
          }
        }
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
