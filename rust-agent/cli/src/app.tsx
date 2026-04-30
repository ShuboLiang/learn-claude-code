import React, { useState, useCallback, useEffect, useRef } from "react";
import { Box, Text, useApp, useInput } from "ink";
import {
  sendMessage,
  init,
  createSession,
  clearSession,
  fetchBots,
  sendBotTask,
  BotInfo,
  fetchSessions,
  fetchSessionMessages,
  setSessionId,
} from "./api";
import { transformMessages } from "./session-utils";
import Chat from "./chat";
import Input from "./input";

function formatTokens(n: number): string {
  if (n >= 1_000_000) {
    return (n / 1_000_000).toFixed(1).replace(/\.0$/, "") + "m";
  }
  if (n >= 1_000) {
    return (n / 1_000).toFixed(1).replace(/\.0$/, "") + "k";
  }
  return String(n);
}

export default function App({ serverUrl }: { serverUrl: string }) {
  const { exit } = useApp();
  const [sessionId, setSessionIdState] = useState("");
  const [model, setModel] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [messages, setMessages] = useState<
    Array<{ role: string; content: string }>
  >([]);
  const [currentReply, setCurrentReply] = useState("");
  const [bots, setBots] = useState<BotInfo[]>([]);
  const [botsLoaded, setBotsLoaded] = useState(false);
  // 当前正在执行的 bot 名称（用于 subagent 模式下的 UI 指示）
  const [activeBot, setActiveBot] = useState<string | null>(null);
  // 重试状态提示（API 限流/网络错误时显示）
  const [retryStatus, setRetryStatus] = useState<string | null>(null);

  // 使用 ref 追踪 currentReply，避免 stale closure
  const currentReplyRef = useRef(currentReply);
  currentReplyRef.current = currentReply;

  const abortControllerRef = useRef<AbortController | null>(null);

  // 历史会话列表缓存（供 /load 命令使用序号恢复）
  const sessionListRef = useRef<
    Array<{ id: string; message_count: number; preview: string; last_active: string }>
  >([]);

  // ESC 中断正在进行的对话
  useInput((_input, key) => {
    if (key.escape && abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
  });

  useEffect(() => {
    (async () => {
      init(serverUrl, "");
      try {
        const { id, model } = await createSession();
        setSessionIdState(id);
        setModel(model);
      } catch (err) {
        setError(`会话创建失败: ${err}`);
      }
      // 加载 Bot 列表
      try {
        const botList = await fetchBots();
        setBots(botList);
      } catch {
        // Bot 列表加载失败不阻塞主流程
      }
      setBotsLoaded(true);
    })();
  }, [serverUrl]);

  // 处理 subagent 任务：/@@botname task_description
  const handleBotTask = useCallback(
    async (botName: string, task: string) => {
      if (!task.trim()) {
        setError(`@${botName}: 请提供任务描述`);
        return;
      }
      const bot = bots.find((b) => b.name === botName);
      const displayName = bot?.nickname || botName;

      setMessages((prev) => [
        ...prev,
        { role: "bot_call", content: `@${displayName}: ${task}` },
      ]);
      setIsLoading(true);
      setActiveBot(displayName);
      setCurrentReply("");
      abortControllerRef.current = new AbortController();

      try {
        for await (const event of sendBotTask(
          botName,
          task,
          abortControllerRef.current.signal,
        )) {
          switch (event.event) {
            case "text_delta":
              setCurrentReply((prev) => prev + event.data.content);
              break;
            case "tool_call":
              setMessages((prev) => [
                ...prev,
                { role: "tool_call", content: JSON.stringify(event.data) },
              ]);
              break;
            case "tool_result":
              setMessages((prev) => [
                ...prev,
                { role: "tool_result", content: event.data.output },
              ]);
              break;
            case "turn_end": {
              setRetryStatus(null);
              const reply = currentReplyRef.current;
              setCurrentReply("");
              currentReplyRef.current = "";
              if (reply) {
                setMessages((prev) => [
                  ...prev,
                  { role: "assistant", content: reply },
                ]);
              }
              const apiCalls = event.data?.api_calls;
              const tokenUsage = event.data?.token_usage;
              let info = `── @${displayName} 完成`;
              if (apiCalls) info += `，API 调用 ${apiCalls} 次`;
              if (tokenUsage) {
                info += ` │ Token: ${formatTokens(tokenUsage.input_tokens)}入/${formatTokens(tokenUsage.output_tokens)}出`;
              }
              info += " ──";
              setMessages((prev) => [
                ...prev,
                { role: "system", content: info },
              ]);
              break;
            }
            case "retrying": {
              const d = event.data;
              setRetryStatus(
                `⏳ 正在重试 (${d.attempt + 1}/${d.max_retries + 1}) — ${d.detail}，等待 ${d.wait_seconds}s`,
              );
              break;
            }
            case "error": {
              setError(`[@${displayName}] ${event.data.message || "未知错误"}`);
              setRetryStatus(null);
              break;
            }
          }
        }
      } catch (err) {
        const isAbort = err instanceof Error && err.name === "AbortError";
        const isTerminated =
          err instanceof TypeError && String(err).includes("terminated");
        if (isAbort || isTerminated) {
          const reply = currentReplyRef.current;
          if (reply) {
            setMessages((prev) => [
              ...prev,
              { role: "assistant", content: reply },
            ]);
          }
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: isAbort ? "── 已中断 ──" : "── 连接已断开 ──",
            },
          ]);
        } else {
          setError(String(err));
          setRetryStatus(null);
        }
      } finally {
        setIsLoading(false);
        setCurrentReply("");
        currentReplyRef.current = "";
        setActiveBot(null);
        setRetryStatus(null);
        abortControllerRef.current = null;
      }
    },
    [bots],
  );

  const handleSubmit = useCallback(
    async (input: string) => {
      if (!input.trim() || isLoading || !sessionId) return;

      let actualInput = input;

      // /@botname command: 转为 call_bot 指令通过主 agent 执行（保持 session 上下文）
      const botMatch = input.trim().match(/^\/@(\S+)\s+(.*)/);
      if (botMatch) {
        const botName = botMatch[1];
        const task = botMatch[2];
        const displayName = bots.find((b) => b.name === botName)?.nickname || botName;
        setMessages((prev) => [
          ...prev,
          { role: "bot_call", content: `@${displayName}: ${task}` },
        ]);
        actualInput = `请使用 call_bot 工具，bot 名称为 "${botMatch[1]}"，执行以下任务：\n\n${task}`;
        setActiveBot(displayName);
      }

      // /@@botname command (double @): 同上
      const botMatch2 = input.trim().match(/^\/@@(\S+)\s+(.*)/);
      if (botMatch2) {
        const displayName = bots.find((b) => b.name === botMatch2[1])?.nickname || botMatch2[1];
        setMessages((prev) => [
          ...prev,
          { role: "bot_call", content: `@${displayName}: ${botMatch2[2]}` },
        ]);
        actualInput = `请使用 call_bot 工具，bot 名称为 "${botMatch2[1]}"，执行以下任务：\n\n${botMatch2[2]}`;
        setActiveBot(displayName);
      }

      // /bots command: list available bots
      if (input.trim().toLowerCase() === "/bots") {
        if (bots.length === 0) {
          setError("没有可用的 Bot。请在 bots/ 目录下创建 BOT.md 文件。");
        } else {
          const botList = bots
            .map(
              (b) =>
                `  @${b.name}${b.nickname ? ` (${b.nickname})` : ""} - ${b.role}${b.description ? ": " + b.description : ""}`,
            )
            .join("\n");
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: `可用 Subagent:\n${botList}\n\n使用 /@@botname 任务描述 来委派任务`,
            },
          ]);
        }
        return;
      }

      // /sessions command: list historical sessions
      if (input.trim().toLowerCase() === "/sessions") {
        try {
          const sessions = await fetchSessions();
          sessionListRef.current = sessions;
          if (sessions.length === 0) {
            setMessages((prev) => [
              ...prev,
              { role: "system", content: "═══ 暂无历史会话 ═══" },
            ]);
            return;
          }
          const lines = sessions.map((s, i) => {
            const date = new Date(s.last_active).toLocaleString("zh-CN", {
              month: "2-digit",
              day: "2-digit",
              hour: "2-digit",
              minute: "2-digit",
            });
            return `[${i + 1}] ${date}  (${s.message_count} 条)  ${s.preview}`;
          });
          const text = `═══ 历史会话 ═══\n${lines.join("\n")}\n════════════════\n使用 /load <序号> 或 /load <uuid> 恢复`;
          setMessages((prev) => [...prev, { role: "system", content: text }]);
        } catch (err) {
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: `获取历史会话失败: ${err}`,
            },
          ]);
        }
        return;
      }

      // /load command: recover a historical session
      const loadMatch = input.trim().match(/^\/load\s+(.+)$/);
      if (loadMatch) {
        if (isLoading) {
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: "当前有进行中的对话，请等待结束后再加载。",
            },
          ]);
          return;
        }
        const arg = loadMatch[1].trim();
        let targetId: string | undefined;
        const index = parseInt(arg, 10);
        if (
          !isNaN(index) &&
          index > 0 &&
          index <= sessionListRef.current.length
        ) {
          targetId = sessionListRef.current[index - 1].id;
        } else if (!isNaN(index)) {
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: "无效序号，先用 /sessions 查看列表。",
            },
          ]);
          return;
        } else {
          targetId = arg;
        }

        if (!targetId) {
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: "无效序号或 UUID，先用 /sessions 查看列表。",
            },
          ]);
          return;
        }

        try {
          const apiMessages = await fetchSessionMessages(targetId);
          const newMessages = transformMessages(apiMessages);
          setSessionId(targetId);
          setSessionIdState(targetId);
          const preview =
            sessionListRef.current.find((s) => s.id === targetId)?.preview ||
            targetId.slice(0, 8);
          setMessages([
            ...newMessages,
            { role: "system", content: `═══ 已恢复会话 ${preview} ═══` },
          ]);
        } catch (err) {
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: `加载会话失败: ${err}`,
            },
          ]);
        }
        return;
      }

      setError(null);
      setMessages((prev) => [...prev, { role: "user", content: input }]);
      setIsLoading(true);
      setCurrentReply("");
      abortControllerRef.current = new AbortController();
      try {
        for await (const event of sendMessage(
          actualInput,
          abortControllerRef.current.signal,
        )) {
          switch (event.event) {
            case "text_delta":
              setCurrentReply((prev) => prev + event.data.content);
              break;
            case "tool_call": {
              const reply = currentReplyRef.current;
              setCurrentReply("");
              currentReplyRef.current = "";
              if (reply) {
                setMessages((prev) => [
                  ...prev,
                  { role: "assistant", content: reply },
                ]);
              }
              // call_bot 工具使用 bot 专用样式，其余工具通用显示
              if (event.data.name === "call_bot") {
                const botName = event.data.input?.name || "unknown";
                const task = event.data.input?.task || "";
                setMessages((prev) => [
                  ...prev,
                  { role: "bot_call", content: `@${botName}: ${task}` },
                ]);
              } else {
                setMessages((prev) => [
                  ...prev,
                  { role: "tool_call", content: JSON.stringify(event.data) },
                ]);
              }
              break;
            }
            case "tool_result":
              setMessages((prev) => [
                ...prev,
                { role: "tool_result", content: event.data.output },
              ]);
              break;
            case "turn_end": {
              setRetryStatus(null);
              const reply = currentReplyRef.current;
              setCurrentReply("");
              currentReplyRef.current = "";
              if (reply) {
                setMessages((prev) => [
                  ...prev,
                  { role: "assistant", content: reply },
                ]);
              }
              const apiCalls = event.data?.api_calls;
              const tokenUsage = event.data?.token_usage;
              let info = apiCalls
                ? `── 完成，API 调用 ${apiCalls} 次`
                : "── 完成";
              if (tokenUsage) {
                info += ` │ Token: ${formatTokens(tokenUsage.input_tokens)}入/${formatTokens(tokenUsage.output_tokens)}出`;
                if (tokenUsage.cache_read_tokens > 0) {
                  info += ` (缓存命中 ${formatTokens(tokenUsage.cache_read_tokens)})`;
                }
              }
              info += " ──";
              setMessages((prev) => [
                ...prev,
                { role: "system", content: info },
              ]);
              break;
            }
            case "retrying": {
              const d = event.data;
              setRetryStatus(
                `⏳ 正在重试 (${d.attempt + 1}/${d.max_retries + 1}) — ${d.detail}，等待 ${d.wait_seconds}s`,
              );
              break;
            }
            case "error": {
              setError(event.data.message || "未知错误");
              setRetryStatus(null);
              break;
            }
            case "done": {
              setCurrentReply("");
              currentReplyRef.current = "";
              break;
            }
          }
        }
      } catch (err) {
        const isAbort = err instanceof Error && err.name === "AbortError";
        const isTerminated =
          err instanceof TypeError && String(err).includes("terminated");
        if (isAbort || isTerminated) {
          const reply = currentReplyRef.current;
          if (reply) {
            setMessages((prev) => [
              ...prev,
              { role: "assistant", content: reply },
            ]);
          }
          setMessages((prev) => [
            ...prev,
            {
              role: "system",
              content: isAbort ? "── 已中断 ──" : "── 连接已断开 ──",
            },
          ]);
        } else {
          setError(String(err));
        }
      } finally {
        setIsLoading(false);
        setCurrentReply("");
        currentReplyRef.current = "";
        setActiveBot(null);
        setRetryStatus(null);
        abortControllerRef.current = null;
      }
    },
    [sessionId, isLoading, handleBotTask],
  );

  const handleQuit = useCallback(
    (input: string) => {
      const lower = input.trim().toLowerCase();
      if (
        lower === "q" ||
        lower === "quit" ||
        lower === "exit" ||
        lower === "/exit"
      )
        exit();
    },
    [exit],
  );

  const handleClear = useCallback(() => {
    setMessages((prev) => [
      ...prev,
      { role: "system", content: "═══ 以上对话已被清除 ═══" },
    ]);
    setCurrentReply("");
    clearSession().catch(() => {});
  }, []);

  return (
    <Box flexDirection="column" height="100%">
      <Chat
        messages={messages}
        currentReply={currentReply}
        isLoading={isLoading}
        activeBot={activeBot}
        retryStatus={retryStatus}
      />
      <Input
        onSubmit={handleSubmit}
        onQuit={handleQuit}
        onClear={handleClear}
        isLoading={isLoading}
        model={model}
      />
      {error && (
        <Box>
          <Text color="red">Error: {error}</Text>
        </Box>
      )}
    </Box>
  );
}
