import { useEffect, useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ApiClient } from "../api/client";
import { useChatStore } from "../stores/useChatStore";
import Sidebar from "./Sidebar";
import MessageList from "./MessageList";
import InputBox from "./InputBox";

export default function ChatPage() {
  const {
    sessions,
    activeSessionId,
    isLoading,
    currentReply,
    error,
    addSession,
    removeSession,
    setActiveSession,
    addMessage,
    setCurrentReply,
    appendCurrentReply,
    setIsLoading,
    setError,
    setAbortController,
    clearCurrentReply,
  } = useChatStore();

  const [apiClient, setApiClient] = useState<ApiClient | null>(null);
  const [serverPort, setServerPort] = useState<number | null>(null);

  // 启动后端 server
  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const port = await invoke<number>("start_server");
        if (mounted) {
          setServerPort(port);
          setApiClient(new ApiClient(`http://127.0.0.1:${port}`));
        }
      } catch (e) {
        if (mounted) setError(String(e));
      }
    })();
    return () => {
      mounted = false;
      invoke("stop_server").catch(() => {});
    };
  }, [setError]);

  // 新建会话
  const handleNewSession = useCallback(async () => {
    if (!apiClient) return;
    try {
      const { id, model } = await apiClient.createSession();
      addSession({ id, model, title: "新会话", messages: [] });
    } catch (e) {
      setError(String(e));
    }
  }, [apiClient, addSession, setError]);

  // 发送消息
  const handleSubmit = useCallback(
    async (text: string) => {
      if (!apiClient || !activeSessionId) return;
      setError(null);
      addMessage(activeSessionId, { role: "user", content: text });
      setIsLoading(true);
      clearCurrentReply();

      const controller = new AbortController();
      setAbortController(controller);

      try {
        for await (const event of apiClient.sendMessage(
          activeSessionId,
          text,
          controller.signal
        )) {
          switch (event.event) {
            case "text_delta":
              appendCurrentReply(event.data.content);
              break;
            case "tool_call": {
              const reply = useChatStore.getState().currentReply;
              if (reply) {
                addMessage(activeSessionId, { role: "assistant", content: reply });
              }
              clearCurrentReply();
              addMessage(activeSessionId, {
                role: "tool_call",
                content: JSON.stringify(event.data),
              });
              break;
            }
            case "tool_result":
              addMessage(activeSessionId, {
                role: "tool_result",
                content: event.data.output,
              });
              break;
            case "turn_end": {
              const reply = useChatStore.getState().currentReply;
              if (reply) {
                addMessage(activeSessionId, { role: "assistant", content: reply });
              }
              clearCurrentReply();
              const apiCalls = event.data?.api_calls;
              if (apiCalls) {
                addMessage(activeSessionId, {
                  role: "system",
                  content: `── 完成，API 调用 ${apiCalls} 次 ──`,
                });
              }
              break;
            }
            case "error":
              setError(event.data.message || "未知错误");
              break;
            case "done":
              clearCurrentReply();
              break;
          }
        }
      } catch (err) {
        if (err instanceof Error && err.name === "AbortError") {
          const reply = useChatStore.getState().currentReply;
          if (reply) {
            addMessage(activeSessionId, { role: "assistant", content: reply });
          }
          addMessage(activeSessionId, { role: "system", content: "── 已中断 ──" });
        } else {
          setError(String(err));
        }
      } finally {
        setIsLoading(false);
        clearCurrentReply();
        setAbortController(null);
      }
    },
    [
      apiClient,
      activeSessionId,
      addMessage,
      setIsLoading,
      clearCurrentReply,
      appendCurrentReply,
      setError,
      setAbortController,
    ]
  );

  // 中断对话
  const handleAbort = useCallback(() => {
    const controller = useChatStore.getState().abortController;
    if (controller) {
      controller.abort();
      setAbortController(null);
    }
  }, [setAbortController]);

  // 删除会话
  const handleDelete = useCallback(
    async (id: string) => {
      if (!apiClient) return;
      try {
        await apiClient.deleteSession(id);
        removeSession(id);
      } catch (e) {
        setError(String(e));
      }
    },
    [apiClient, removeSession, setError]
  );

  const activeSession = sessions.find((s) => s.id === activeSessionId);

  return (
    <div className="flex h-full">
      <Sidebar
        sessions={sessions}
        activeId={activeSessionId}
        onSelect={setActiveSession}
        onNew={handleNewSession}
        onDelete={handleDelete}
      />
      <div className="flex flex-1 flex-col">
        {error && (
          <div className="border-b border-red-900/50 bg-red-900/20 px-4 py-2 text-xs text-red-400">
            Error: {error}
          </div>
        )}
        {!serverPort && (
          <div className="flex flex-1 items-center justify-center text-sm text-neutral-500">
            正在启动后端服务...
          </div>
        )}
        {serverPort && !activeSession && (
          <div className="flex flex-1 items-center justify-center text-sm text-neutral-500">
            点击左侧“+”新建会话开始对话
          </div>
        )}
        {serverPort && activeSession && (
          <>
            <div className="border-b border-neutral-800 px-4 py-2 text-xs text-neutral-500">
              {activeSession.model}
            </div>
            <MessageList messages={activeSession.messages} currentReply={currentReply} />
            <InputBox
              onSubmit={handleSubmit}
              onAbort={handleAbort}
              isLoading={isLoading}
              model={activeSession.model}
            />
          </>
        )}
      </div>
    </div>
  );
}
