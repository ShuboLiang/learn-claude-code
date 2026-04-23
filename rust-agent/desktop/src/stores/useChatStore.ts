import { create } from "zustand";

export interface Message {
  role: "user" | "assistant" | "tool_call" | "tool_result" | "system";
  content: string;
}

export interface ChatSession {
  id: string;
  model: string;
  title: string;
  messages: Message[];
}

interface ChatState {
  sessions: ChatSession[];
  activeSessionId: string | null;
  isLoading: boolean;
  currentReply: string;
  error: string | null;
  abortController: AbortController | null;

  setSessions: (sessions: ChatSession[]) => void;
  addSession: (session: ChatSession) => void;
  removeSession: (id: string) => void;
  setActiveSession: (id: string) => void;
  addMessage: (sessionId: string, message: Message) => void;
  setCurrentReply: (reply: string) => void;
  appendCurrentReply: (text: string) => void;
  setIsLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setAbortController: (controller: AbortController | null) => void;
  clearCurrentReply: () => void;
}

export const useChatStore = create<ChatState>((set) => ({
  sessions: [],
  activeSessionId: null,
  isLoading: false,
  currentReply: "",
  error: null,
  abortController: null,

  setSessions: (sessions) => set({ sessions }),
  addSession: (session) =>
    set((state) => ({
      sessions: [...state.sessions, session],
      activeSessionId: session.id,
    })),
  removeSession: (id) =>
    set((state) => ({
      sessions: state.sessions.filter((s) => s.id !== id),
      activeSessionId:
        state.activeSessionId === id
          ? state.sessions.find((s) => s.id !== id)?.id ?? null
          : state.activeSessionId,
    })),
  setActiveSession: (id) => set({ activeSessionId: id }),
  addMessage: (sessionId, message) =>
    set((state) => ({
      sessions: state.sessions.map((s) =>
        s.id === sessionId ? { ...s, messages: [...s.messages, message] } : s
      ),
    })),
  setCurrentReply: (reply) => set({ currentReply: reply }),
  appendCurrentReply: (text) =>
    set((state) => ({ currentReply: state.currentReply + text })),
  setIsLoading: (loading) => set({ isLoading: loading }),
  setError: (error) => set({ error }),
  setAbortController: (controller) => set({ abortController: controller }),
  clearCurrentReply: () => set({ currentReply: "" }),
}));
