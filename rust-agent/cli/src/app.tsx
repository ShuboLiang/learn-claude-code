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
} from "./api";
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
  const [sessionId, setSessionId] = useState("");
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
        setSessionId(id);
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

      // /@botname command: dispatch subagent task
      const botMatch = input.trim().match(/^\/@(\S+)\s+(.*)/);
      if (botMatch) {
        handleBotTask(botMatch[1], botMatch[2]);
        return;
      }

      // /@@botname command (double @): dispatch subagent task
      const botMatch2 = input.trim().match(/^\/@@(\S+)\s+(.*)/);
      if (botMatch2) {
        handleBotTask(botMatch2[1], botMatch2[2]);
        return;
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

      setError(null);
      setMessages((prev) => [...prev, { role: "user", content: input }]);
      setIsLoading(true);
      setCurrentReply("");
      abortControllerRef.current = new AbortController();
      try {
        for await (const event of sendMessage(
          input,
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
              setMessages((prev) => [
                ...prev,
                { role: "tool_call", content: JSON.stringify(event.data) },
              ]);
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
