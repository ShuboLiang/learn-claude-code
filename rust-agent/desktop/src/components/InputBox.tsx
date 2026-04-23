import { useState, useRef, KeyboardEvent } from "react";
import { Send, Square } from "lucide-react";

interface InputBoxProps {
  onSubmit: (text: string) => void;
  onAbort: () => void;
  isLoading: boolean;
  model?: string;
}

export default function InputBox({ onSubmit, onAbort, isLoading, model }: InputBoxProps) {
  const [value, setValue] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (value.trim() && !isLoading) {
        onSubmit(value.replace(/\\n/g, "\n"));
        setValue("");
      }
    }
  };

  const handleSend = () => {
    if (value.trim() && !isLoading) {
      onSubmit(value.replace(/\\n/g, "\n"));
      setValue("");
    }
  };

  return (
    <div className="border-t border-neutral-800 bg-neutral-900 p-4">
      <div className="flex items-end gap-2 rounded-xl border border-neutral-700 bg-neutral-800 px-3 py-2">
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={
            isLoading
              ? "等待响应中..."
              : model
              ? `[${model}] 输入消息，Shift+Enter 换行...`
              : "输入消息，Shift+Enter 换行..."
          }
          disabled={isLoading}
          rows={1}
          className="max-h-32 min-h-[24px] flex-1 resize-none bg-transparent text-sm text-neutral-100 outline-none placeholder:text-neutral-500"
          style={{ fieldSizing: "content" } as React.CSSProperties}
        />
        {isLoading ? (
          <button
            onClick={onAbort}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-red-600 text-white transition hover:bg-red-500"
            title="中断"
          >
            <Square size={14} fill="currentColor" />
          </button>
        ) : (
          <button
            onClick={handleSend}
            disabled={!value.trim()}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-blue-600 text-white transition hover:bg-blue-500 disabled:opacity-40"
            title="发送"
          >
            <Send size={14} />
          </button>
        )}
      </div>
    </div>
  );
}
