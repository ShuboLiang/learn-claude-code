import { useRef, useEffect } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { User, Bot, Wrench, WrenchIcon, Info } from "lucide-react";
import type { Message } from "../stores/useChatStore";

interface MessageListProps {
  messages: Message[];
  currentReply: string;
}

const roleConfig: Record<string, { icon: React.ReactNode; color: string; bg: string; label: string }> = {
  user: { icon: <User size={14} />, color: "text-white", bg: "bg-blue-600", label: "你" },
  assistant: { icon: <Bot size={14} />, color: "text-neutral-100", bg: "bg-neutral-700", label: "助手" },
  tool_call: { icon: <Wrench size={14} />, color: "text-amber-200", bg: "bg-amber-900/40", label: "工具调用" },
  tool_result: { icon: <WrenchIcon size={14} />, color: "text-emerald-200", bg: "bg-emerald-900/40", label: "工具结果" },
  system: { icon: <Info size={14} />, color: "text-neutral-400", bg: "bg-transparent", label: "系统" },
};

function MessageItem({ message }: { message: Message }) {
  const config = roleConfig[message.role] ?? roleConfig.system;
  const isUser = message.role === "user";

  return (
    <div className={`flex gap-3 ${isUser ? "flex-row-reverse" : "flex-row"}`}>
      <div
        className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-full ${config.bg} ${config.color}`}
        title={config.label}
      >
        {config.icon}
      </div>
      <div
        className={`max-w-[80%] rounded-xl px-4 py-2 text-sm ${
          isUser
            ? "bg-blue-600 text-white"
            : message.role === "system"
            ? "text-neutral-500 italic"
            : "bg-neutral-800 text-neutral-100"
        }`}
      >
        {message.role === "assistant" || message.role === "user" ? (
          <div className="prose prose-invert prose-sm max-w-none">
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {message.content}
            </ReactMarkdown>
          </div>
        ) : message.role === "tool_call" ? (
          <pre className="overflow-x-auto text-xs">
            <code>{message.content}</code>
          </pre>
        ) : (
          <span>{message.content}</span>
        )}
      </div>
    </div>
  );
}

export default function MessageList({ messages, currentReply }: MessageListProps) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, currentReply]);

  return (
    <div className="flex-1 overflow-y-auto space-y-4 p-4">
      {messages.map((msg, i) => (
        <MessageItem key={i} message={msg} />
      ))}
      {currentReply && (
        <div className="flex gap-3">
          <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-neutral-700 text-neutral-100">
            <Bot size={14} />
          </div>
          <div className="max-w-[80%] rounded-xl bg-neutral-800 px-4 py-2 text-sm text-neutral-100">
            <div className="prose prose-invert prose-sm max-w-none">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>
                {currentReply + "▌"}
              </ReactMarkdown>
            </div>
          </div>
        </div>
      )}
      <div ref={bottomRef} />
    </div>
  );
}
